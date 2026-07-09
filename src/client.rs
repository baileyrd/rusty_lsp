//! The [`Client`] handle: the server's outbound channel to the editor.
//!
//! A `Client` is handed to your backend when the server starts. It is cheap to
//! [`Clone`] and safe to share across tasks. Use it to push notifications
//! (diagnostics, log/show-message) and to issue server-to-client requests
//! (e.g. `workspace/configuration`).

use crate::error::{Error, Result};
use crate::jsonrpc::{Message, Notification, Request, RequestId, Response};
use crate::lsp::{
    ApplyWorkspaceEditParams, ApplyWorkspaceEditResult, ConfigurationItem, ConfigurationParams,
    Diagnostic, LogMessageParams, MessageType, ProgressParams, ProgressToken,
    PublishDiagnosticsParams, ShowMessageParams, Uri, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCreateParams, WorkDoneProgressEnd, WorkDoneProgressReport, WorkspaceEdit,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, MutexGuard};
use tokio::sync::{mpsc, oneshot};

/// Map of in-flight server-to-client requests awaiting their responses.
type PendingResponses = Arc<Mutex<HashMap<RequestId, oneshot::Sender<Response>>>>;

/// A cloneable handle for sending messages from the server to the client.
#[derive(Clone)]
pub struct Client {
    sender: mpsc::UnboundedSender<Message>,
    next_id: Arc<AtomicI64>,
    pending: PendingResponses,
}

impl Client {
    /// Build a client over an outbound message channel.
    ///
    /// Each [`Client`] owns the map of in-flight server-to-client requests;
    /// clones share it, so a response delivered to any clone resolves the
    /// original caller.
    pub(crate) fn new(sender: mpsc::UnboundedSender<Message>) -> Self {
        Client {
            sender,
            next_id: Arc::new(AtomicI64::new(1)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Enqueue a notification with typed params.
    ///
    /// Returns an error only if the params fail to serialize or the connection
    /// has already closed. The send itself does not await — notifications are
    /// fire-and-forget.
    pub fn notify<P: Serialize>(&self, method: &str, params: P) -> Result<()> {
        let params = serde_json::to_value(params)?;
        self.send(Message::Notification(Notification {
            method: method.to_owned(),
            params: Some(params),
        }))
    }

    /// Send a request to the client and await its response.
    ///
    /// The result is deserialized into `R`. A client error response becomes
    /// [`Error::Response`]; a dropped connection becomes an internal error.
    /// Params that serialize to `null` (e.g. `()`, for parameterless
    /// methods like the `workspace/*/refresh` family) are sent with the
    /// `params` member omitted entirely, matching what those methods expect
    /// on the wire.
    pub async fn send_request<P, R>(&self, method: &str, params: P) -> Result<R>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let id = RequestId::Number(self.next_id.fetch_add(1, Ordering::Relaxed));
        let params = serde_json::to_value(params)?;
        let (tx, rx) = oneshot::channel();
        self.lock_pending().insert(id.clone(), tx);

        if let Err(err) = self.send(Message::Request(Request {
            id: id.clone(),
            method: method.to_owned(),
            params: if params.is_null() { None } else { Some(params) },
        })) {
            // Roll back the pending entry so it cannot leak.
            self.lock_pending().remove(&id);
            return Err(err);
        }

        let response = rx
            .await
            .map_err(|_| Error::internal("server-to-client response channel dropped"))?;
        if let Some(error) = response.error {
            return Err(Error::Response(error));
        }
        let value = response.result.unwrap_or(Value::Null);
        Ok(serde_json::from_value(value)?)
    }

    /// Publish diagnostics for a document, replacing any previous set.
    ///
    /// Pass an empty `diagnostics` vector to clear them.
    pub fn publish_diagnostics(
        &self,
        uri: Uri,
        diagnostics: Vec<Diagnostic>,
        version: Option<i32>,
    ) -> Result<()> {
        self.notify(
            "textDocument/publishDiagnostics",
            PublishDiagnosticsParams {
                uri,
                version,
                diagnostics,
            },
        )
    }

    /// Send a `window/logMessage` notification (routed to the client's log).
    pub fn log_message(&self, typ: MessageType, message: impl Into<String>) -> Result<()> {
        self.notify(
            "window/logMessage",
            LogMessageParams {
                typ,
                message: message.into(),
            },
        )
    }

    /// Send a `window/showMessage` notification (shown to the user).
    pub fn show_message(&self, typ: MessageType, message: impl Into<String>) -> Result<()> {
        self.notify(
            "window/showMessage",
            ShowMessageParams {
                typ,
                message: message.into(),
            },
        )
    }

    /// Ask the client to reserve `token` for a subsequent work-done-progress
    /// sequence not tied to a specific client request (e.g. background
    /// indexing). Await the result before calling
    /// [`progress_begin`](Self::progress_begin).
    pub async fn create_progress(&self, token: impl Into<ProgressToken>) -> Result<()> {
        self.send_request(
            "window/workDoneProgress/create",
            WorkDoneProgressCreateParams {
                token: token.into(),
            },
        )
        .await
    }

    /// Start a work-done-progress sequence for `token` (reserved beforehand
    /// via [`create_progress`](Self::create_progress), or a token supplied by
    /// the client on a request's `workDoneToken`).
    pub fn progress_begin(
        &self,
        token: impl Into<ProgressToken>,
        begin: WorkDoneProgressBegin,
    ) -> Result<()> {
        self.notify(
            "$/progress",
            ProgressParams {
                token: token.into(),
                value: WorkDoneProgress::Begin(begin),
            },
        )
    }

    /// Report incremental progress within a sequence started with
    /// [`progress_begin`](Self::progress_begin).
    pub fn progress_report(
        &self,
        token: impl Into<ProgressToken>,
        report: WorkDoneProgressReport,
    ) -> Result<()> {
        self.notify(
            "$/progress",
            ProgressParams {
                token: token.into(),
                value: WorkDoneProgress::Report(report),
            },
        )
    }

    /// End a progress sequence started with
    /// [`progress_begin`](Self::progress_begin).
    pub fn progress_end(
        &self,
        token: impl Into<ProgressToken>,
        end: WorkDoneProgressEnd,
    ) -> Result<()> {
        self.notify(
            "$/progress",
            ProgressParams {
                token: token.into(),
                value: WorkDoneProgress::End(end),
            },
        )
    }

    /// Ask the client for the value of one or more configuration sections
    /// (`workspace/configuration`). The result has one entry per item in
    /// `items`, in the same order; a section the client has no value for
    /// comes back as `Value::Null`.
    pub async fn configuration(&self, items: Vec<ConfigurationItem>) -> Result<Vec<Value>> {
        self.send_request("workspace/configuration", ConfigurationParams { items })
            .await
    }

    /// Ask the client to apply a [`WorkspaceEdit`] to its buffers
    /// (`workspace/applyEdit`).
    pub async fn apply_edit(
        &self,
        edit: WorkspaceEdit,
        label: Option<String>,
    ) -> Result<ApplyWorkspaceEditResult> {
        self.send_request(
            "workspace/applyEdit",
            ApplyWorkspaceEditParams { label, edit },
        )
        .await
    }

    /// Send a raw `$/progress` notification carrying `value` on `token`.
    ///
    /// Unlike [`progress_begin`](Self::progress_begin)/
    /// [`progress_report`](Self::progress_report)/
    /// [`progress_end`](Self::progress_end) (which wrap `value` in the
    /// [`WorkDoneProgress`] begin/report/end shape), this sends `value`
    /// as-is — the shape a client streaming a request's
    /// `partialResultToken` expects (e.g. a chunk of a `references` result:
    /// `Vec<Location>`).
    pub fn send_progress<T: Serialize>(
        &self,
        token: impl Into<ProgressToken>,
        value: T,
    ) -> Result<()> {
        #[derive(Serialize)]
        struct RawProgressParams<T> {
            token: ProgressToken,
            value: T,
        }

        self.notify(
            "$/progress",
            RawProgressParams {
                token: token.into(),
                value,
            },
        )
    }

    /// Ask the client to re-pull semantic tokens for all open documents
    /// (`workspace/semanticTokens/refresh`), e.g. after a change that
    /// invalidates previously reported tokens.
    pub async fn refresh_semantic_tokens(&self) -> Result<()> {
        self.send_request("workspace/semanticTokens/refresh", ())
            .await
    }

    /// Ask the client to re-pull code lenses for all open documents
    /// (`workspace/codeLens/refresh`).
    pub async fn refresh_code_lenses(&self) -> Result<()> {
        self.send_request("workspace/codeLens/refresh", ()).await
    }

    /// Ask the client to re-pull inlay hints for all open documents
    /// (`workspace/inlayHint/refresh`).
    pub async fn refresh_inlay_hints(&self) -> Result<()> {
        self.send_request("workspace/inlayHint/refresh", ()).await
    }

    /// Ask the client to re-pull diagnostics (`workspace/diagnostic/refresh`),
    /// e.g. after a configuration change that alters which diagnostics a
    /// server reports.
    pub async fn refresh_diagnostics(&self) -> Result<()> {
        self.send_request("workspace/diagnostic/refresh", ()).await
    }

    /// Deliver a response received from the client to its waiting caller.
    ///
    /// Called by the server loop when a [`Message::Response`] arrives. Unknown
    /// or already-resolved ids are dropped silently.
    pub(crate) fn resolve(&self, response: Response) {
        let Some(id) = response.id.clone() else {
            return;
        };
        if let Some(tx) = self.lock_pending().remove(&id) {
            // The receiver may have been dropped (caller gave up); ignore.
            let _ = tx.send(response);
        }
    }

    /// Send a raw message over the outbound channel.
    fn send(&self, message: Message) -> Result<()> {
        self.sender
            .send(message)
            .map_err(|_| Error::internal("server output channel closed"))
    }

    /// Lock the pending map, recovering from a poisoned mutex.
    ///
    /// Poisoning means a task panicked while holding the lock; the map itself
    /// stays consistent, so recovering the guard is preferable to wedging every
    /// future request.
    fn lock_pending(&self) -> MutexGuard<'_, HashMap<RequestId, oneshot::Sender<Response>>> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
