//! The [`Server`]: the runtime that drives an LSP connection.
//!
//! [`Server::serve`] takes ownership of a read/write byte stream, builds your
//! [`LanguageServer`] backend, and runs the message loop until the client
//! disconnects or sends `exit`. It owns everything protocol-related:
//!
//! - **Framing & JSON-RPC** via [`crate::transport`].
//! - **Lifecycle**: enforces `initialize` first, rejects work after `shutdown`,
//!   stops on `exit`.
//! - **Concurrency**: notifications run in receipt order (so document state
//!   stays consistent), while requests are spawned so slow handlers never block
//!   the loop.
//! - **Cancellation**: `$/cancelRequest` aborts the in-flight handler and
//!   replies with [`REQUEST_CANCELLED`](crate::error::codes::REQUEST_CANCELLED),
//!   with the bookkeeping arranged so a request is answered exactly once.

use crate::cancel::{self, CancelToken};
use crate::client::{Client, Outbound};
use crate::error::{Error, ResponseError, Result, codes};
use crate::jsonrpc::{Message, Notification, Request, RequestId, Response};
use crate::lsp::{
    CallHierarchyIncomingCallsParams, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    CodeAction, CodeActionParams, CodeLens, CodeLensParams, ColorPresentationParams,
    CompletionItem, CompletionParams, CreateFilesParams, DefinitionParams, DeleteFilesParams,
    DidChangeConfigurationParams, DidChangeNotebookDocumentParams, DidChangeTextDocumentParams,
    DidChangeWatchedFilesParams, DidChangeWorkspaceFoldersParams, DidCloseNotebookDocumentParams,
    DidCloseTextDocumentParams, DidOpenNotebookDocumentParams, DidOpenTextDocumentParams,
    DidSaveNotebookDocumentParams, DidSaveTextDocumentParams, DocumentColorParams,
    DocumentDiagnosticParams, DocumentFormattingParams, DocumentLink, DocumentLinkParams,
    DocumentOnTypeFormattingParams, DocumentRangeFormattingParams, DocumentSymbolParams,
    ExecuteCommandParams, FoldingRangeParams, HoverParams, InitializeParams, InlayHint,
    InlayHintParams, InlineCompletionParams, InlineValueParams, LinkedEditingRangeParams,
    MessageType, MonikerParams, ReferenceParams, RenameFilesParams, RenameParams,
    SelectionRangeParams, SemanticTokensDeltaParams, SemanticTokensParams,
    SemanticTokensRangeParams, SetTraceParams, SignatureHelpParams, TextDocumentPositionParams,
    TypeHierarchyPrepareParams, TypeHierarchySubtypesParams, TypeHierarchySupertypesParams,
    WillSaveTextDocumentParams, WorkDoneProgressCancelParams, WorkspaceDiagnosticParams,
    WorkspaceSymbol, WorkspaceSymbolParams,
};
use crate::service::LanguageServer;
use crate::transport;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::AbortHandle;

/// Bookkeeping for one running request handler.
struct InFlightEntry {
    /// Aborts the handler task at its next `.await` point.
    abort: AbortHandle,
    /// The cooperative signal visible to the handler via
    /// [`crate::cancel::current`], reaching work an abort cannot (blocking
    /// pools, helper tasks, CPU-bound stretches).
    token: CancelToken,
}

/// Map of request ids to their running handlers' bookkeeping.
type InFlight = Arc<Mutex<HashMap<RequestId, InFlightEntry>>>;

/// Params of the `$/cancelRequest` notification.
#[derive(serde::Deserialize)]
struct CancelParams {
    id: RequestId,
}

/// An LSP server bound to a byte transport.
///
/// Construct with [`Server::new`] over any async reader/writer, or
/// [`Server::stdio`] for the conventional stdin/stdout transport, then call
/// [`serve`](Server::serve).
pub struct Server<R, W> {
    reader: R,
    writer: W,
    max_concurrent_requests: Option<usize>,
    outbound_queue_limit: Option<usize>,
}

impl Server<tokio::io::Stdin, tokio::io::Stdout> {
    /// Build a server over the process's standard input and output.
    ///
    /// This is the transport editors use when they launch a language server as
    /// a child process.
    pub fn stdio() -> Self {
        Server::new(tokio::io::stdin(), tokio::io::stdout())
    }
}

#[cfg(feature = "tcp")]
impl Server<tokio::net::tcp::OwnedReadHalf, tokio::net::tcp::OwnedWriteHalf> {
    /// Build a server over an accepted TCP connection (requires the `tcp`
    /// feature), for clients that connect over a socket instead of spawning
    /// the server as a child process.
    pub fn from_tcp(stream: tokio::net::TcpStream) -> Self {
        let (reader, writer) = stream.into_split();
        Server::new(reader, writer)
    }
}

impl<R, W> Server<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    /// Build a server over an arbitrary reader and writer.
    pub fn new(reader: R, writer: W) -> Self {
        Server {
            reader,
            writer,
            max_concurrent_requests: None,
            outbound_queue_limit: None,
        }
    }

    /// Cap how many request handlers may execute concurrently (default:
    /// unlimited). Excess requests still get their own task immediately —
    /// so cancellation and lifecycle bookkeeping work as usual — but wait
    /// their (FIFO) turn before the handler body runs. This bounds the
    /// damage a misbehaving client flooding requests can do.
    #[must_use]
    pub fn with_max_concurrent_requests(mut self, limit: usize) -> Self {
        self.max_concurrent_requests = Some(limit);
        self
    }

    /// Cap how many outbound messages may sit unwritten in the output queue
    /// (default: unlimited). The cap applies to [`Client`]-originated
    /// traffic (notifications and server→client requests), which fails with
    /// an error once the queue is full — a backpressure signal that the
    /// editor is not draining its end. Responses the protocol owes the
    /// client are never dropped and do not observe the cap.
    #[must_use]
    pub fn with_outbound_queue_limit(mut self, limit: usize) -> Self {
        self.outbound_queue_limit = Some(limit);
        self
    }

    /// Run the server until the connection closes or the client sends `exit`.
    ///
    /// `build` receives a [`Client`] and returns the backend; store the client
    /// so handlers can talk back to the editor. Returns `Ok(())` on a clean
    /// shutdown, or an error if the transport failed irrecoverably.
    pub async fn serve<B, F>(self, build: F) -> Result<()>
    where
        B: LanguageServer,
        F: FnOnce(Client) -> B,
    {
        let Server {
            reader,
            mut writer,
            max_concurrent_requests,
            outbound_queue_limit,
        } = self;

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let queue_depth = Arc::new(AtomicUsize::new(0));
        let outbound = Outbound::new(out_tx, Arc::clone(&queue_depth), outbound_queue_limit);
        let client = Client::new(outbound.clone());
        let backend = Arc::new(build(client.clone()));
        let request_permits = max_concurrent_requests.map(|limit| Arc::new(Semaphore::new(limit)));

        // Single writer task: serializes all outbound traffic and is the sole
        // owner of the write half. It ends once every sender is dropped.
        let writer_task = tokio::spawn(async move {
            while let Some(message) = out_rx.recv().await {
                queue_depth.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                transport::write_message(&mut writer, &message).await?;
            }
            Ok::<(), Error>(())
        });

        let result = run_loop(reader, client, outbound, request_permits, backend).await;

        // `run_loop` has dropped its senders and aborted in-flight handlers on
        // return; await the writer so buffered output is flushed before exit.
        match writer_task.await {
            Ok(write_result) => result.and(write_result),
            Err(join_err) if join_err.is_panic() => {
                Err(Error::internal(format!("writer task panicked: {join_err}")))
            }
            Err(_) => result,
        }
    }
}

/// The message loop. Owns the loop-side handles so they drop on return.
async fn run_loop<R, B>(
    reader: R,
    client: Client,
    out_tx: Outbound,
    request_permits: Option<Arc<Semaphore>>,
    backend: Arc<B>,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    B: LanguageServer,
{
    let mut reader = transport::buffered(reader);
    let in_flight: InFlight = Arc::new(Mutex::new(HashMap::new()));
    let mut initialized = false;
    let mut shutdown_requested = false;

    let loop_result = loop {
        let message = match transport::read_message(&mut reader).await {
            Ok(Some(message)) => message,
            Ok(None) => break Ok(()), // clean EOF at a frame boundary
            // The frame itself was read intact (exactly `Content-Length` bytes
            // consumed) but its body was not valid JSON, or classified to an
            // invalid `Message` shape (e.g. a non-numeric/string id). The
            // stream is not desynchronised, so report a Parse error for this
            // message and keep the connection alive, per JSON-RPC/LSP.
            Err(Error::Serde(err)) => {
                let _ = out_tx.send(
                    Response::error(
                        None,
                        ResponseError {
                            code: codes::PARSE_ERROR,
                            message: err.to_string(),
                            data: None,
                        },
                    )
                    .into(),
                );
                continue;
            }
            Err(err) => {
                // A framing error (bad headers, mid-frame EOF, ...)
                // desynchronises the stream; we cannot safely continue.
                // Surface it to the caller.
                break Err(err);
            }
        };

        #[cfg(feature = "tracing")]
        match &message {
            Message::Request(req) => {
                tracing::debug!(method = %req.method, id = %req.id, "<-- request");
            }
            Message::Notification(note) => {
                tracing::debug!(method = %note.method, "<-- notification");
            }
            Message::Response(response) => {
                tracing::debug!(id = ?response.id, ok = response.error.is_none(), "<-- response");
            }
        }

        match message {
            Message::Response(response) => client.resolve(response),

            Message::Notification(note) => match note.method.as_str() {
                "exit" => break Ok(()),
                "$/cancelRequest" => cancel_request(&note, &in_flight, &out_tx),
                "initialized" => {
                    if initialized {
                        backend.initialized().await;
                    }
                }
                _ => {
                    // Per spec, drop notifications that arrive before
                    // `initialize` (other than `exit`, handled above).
                    if initialized {
                        dispatch_notification(backend.as_ref(), &client, &note.method, note.params)
                            .await;
                    }
                }
            },

            Message::Request(req) => match req.method.as_str() {
                "initialize" => {
                    if initialized {
                        send_error(
                            &out_tx,
                            req.id,
                            Error::invalid_request("server already initialized"),
                        );
                    } else {
                        let outcome = match parse_params::<InitializeParams>(req.params) {
                            Ok(params) => backend
                                .initialize(params)
                                .await
                                .and_then(|result| to_json(&result)),
                            Err(err) => Err(err),
                        };
                        match outcome {
                            Ok(value) => {
                                initialized = true;
                                let _ = out_tx.send(Response::success(req.id, value).into());
                            }
                            Err(err) => send_error(&out_tx, req.id, err),
                        }
                    }
                }
                "shutdown" => {
                    if !initialized {
                        send_error(&out_tx, req.id, Error::server_not_initialized());
                    } else {
                        shutdown_requested = true;
                        match backend.shutdown().await {
                            Ok(()) => {
                                let _ = out_tx.send(Response::success(req.id, Value::Null).into());
                            }
                            Err(err) => send_error(&out_tx, req.id, err),
                        }
                    }
                }
                _ => {
                    if !initialized {
                        send_error(&out_tx, req.id, Error::server_not_initialized());
                    } else if shutdown_requested {
                        send_error(
                            &out_tx,
                            req.id,
                            Error::invalid_request("server is shutting down"),
                        );
                    } else {
                        spawn_request(&backend, &out_tx, &in_flight, &request_permits, req);
                    }
                }
            },
        }
    };

    // Tear down: abort any handlers still running so their captured senders and
    // backend references drop, letting the writer task wind down.
    for (_, entry) in lock(&in_flight).drain() {
        entry.token.cancel();
        entry.abort.abort();
    }

    loop_result
}

/// Spawn a feature request handler as its own task.
///
/// The task, any concurrent `$/cancelRequest`, and the panic watcher below
/// race to remove the id from [`InFlight`]; whoever wins sends the single
/// response, guaranteeing a request is answered exactly once.
///
/// A client must not reuse the id of a request it has not yet received a
/// response for (JSON-RPC/LSP); a request whose id is already in
/// [`InFlight`] is rejected outright rather than spawned, since letting it
/// through would silently steal the original request's `InFlight` entry —
/// corrupting `$/cancelRequest` targeting and dropping whichever response
/// loses the race. This function only ever runs on the single-threaded
/// message loop (never concurrently with itself), so the check-then-insert
/// below has no race window of its own.
fn spawn_request<B: LanguageServer>(
    backend: &Arc<B>,
    out_tx: &Outbound,
    in_flight: &InFlight,
    request_permits: &Option<Arc<Semaphore>>,
    req: Request,
) {
    let request_id = req.id;
    let method = req.method;
    let params = req.params;

    if lock(in_flight).contains_key(&request_id) {
        send_error(
            out_tx,
            request_id,
            Error::invalid_request("request id is already in flight"),
        );
        return;
    }

    let backend = Arc::clone(backend);
    let handler_out_tx = out_tx.clone();
    let in_flight_for_task = Arc::clone(in_flight);
    let response_id = request_id.clone();
    let token = CancelToken::new();
    let task_token = token.clone();
    let permits = request_permits.clone();

    let join = tokio::spawn(cancel::scope(task_token, async move {
        // Under a concurrency cap, wait for a slot before running the
        // handler body. The task exists either way, so cancellation (which
        // aborts this task) and the exactly-once bookkeeping are unaffected
        // by queueing. `acquire_owned` on an unclosed semaphore cannot fail.
        let _permit = match permits {
            Some(semaphore) => semaphore.acquire_owned().await.ok(),
            None => None,
        };
        // Catch panics in-task so the panic itself becomes the response —
        // there is no separate watcher task racing for the entry.
        let outcome = CatchUnwind(AssertUnwindSafe(dispatch_request(
            backend.as_ref(),
            &method,
            params,
        )))
        .await;
        // Claim the right to respond. If the entry is already gone, a cancel
        // beat us to it and has sent the cancellation response.
        if lock(&in_flight_for_task).remove(&response_id).is_some() {
            let response = match outcome {
                Ok(Ok(value)) => Response::success(response_id, value),
                Ok(Err(err)) => Response::error(Some(response_id), err.into_response_error()),
                Err(payload) => Response::error(
                    Some(response_id),
                    Error::internal(format!("handler panicked: {}", panic_message(&payload)))
                        .into_response_error(),
                ),
            };
            let _ = handler_out_tx.send(response.into());
        }
    }));

    lock(in_flight).insert(
        request_id,
        InFlightEntry {
            abort: join.abort_handle(),
            token,
        },
    );
}

/// A future adapter that catches panics from the inner future's `poll`,
/// resolving to `Err(payload)` instead of unwinding into the executor.
struct CatchUnwind<F>(F);

impl<F: Future> Future for CatchUnwind<F> {
    type Output = std::result::Result<F::Output, Box<dyn std::any::Any + Send>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: structural pinning of the sole field; it is never moved.
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.0) };
        match std::panic::catch_unwind(AssertUnwindSafe(|| inner.poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(value)) => Poll::Ready(Ok(value)),
            Err(payload) => Poll::Ready(Err(payload)),
        }
    }
}

/// Best-effort extraction of a panic payload's message.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> &str {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s
    } else {
        "non-string panic payload"
    }
}

/// Handle `$/cancelRequest`: signal the handler's [`CancelToken`], abort its
/// task, and reply with a cancellation error, but only if the request is
/// still in flight.
fn cancel_request(note: &Notification, in_flight: &InFlight, out_tx: &Outbound) {
    let Some(params) = note.params.clone() else {
        return;
    };
    let Ok(CancelParams { id }) = serde_json::from_value(params) else {
        return;
    };
    // Removing here both stops the handler and prevents it from later sending
    // its own response (it will find its entry already gone).
    if let Some(entry) = lock(in_flight).remove(&id) {
        #[cfg(feature = "tracing")]
        tracing::debug!(id = %id, "request cancelled by client");
        // Trip the cooperative token first, so work the abort cannot reach
        // (spawn_blocking, helper tasks) observes the cancellation too.
        entry.token.cancel();
        entry.abort.abort();
        let _ = out_tx.send(
            Response::error(Some(id), Error::request_cancelled().into_response_error()).into(),
        );
    }
}

/// Route a feature request to the matching typed handler, falling back to the
/// untyped [`LanguageServer::handle_request`] escape hatch.
async fn dispatch_request<B: LanguageServer>(
    backend: &B,
    method: &str,
    params: Option<Value>,
) -> Result<Value> {
    match method {
        "textDocument/hover" => {
            to_json(&backend.hover(parse_params::<HoverParams>(params)?).await?)
        }
        "textDocument/completion" => to_json(
            &backend
                .completion(parse_params::<CompletionParams>(params)?)
                .await?,
        ),
        "textDocument/definition" => to_json(
            &backend
                .definition(parse_params::<DefinitionParams>(params)?)
                .await?,
        ),
        "textDocument/declaration" => to_json(
            &backend
                .declaration(parse_params::<TextDocumentPositionParams>(params)?)
                .await?,
        ),
        "textDocument/typeDefinition" => to_json(
            &backend
                .type_definition(parse_params::<TextDocumentPositionParams>(params)?)
                .await?,
        ),
        "textDocument/implementation" => to_json(
            &backend
                .implementation(parse_params::<TextDocumentPositionParams>(params)?)
                .await?,
        ),
        "textDocument/references" => to_json(
            &backend
                .references(parse_params::<ReferenceParams>(params)?)
                .await?,
        ),
        "completionItem/resolve" => to_json(
            &backend
                .completion_resolve(parse_params::<CompletionItem>(params)?)
                .await?,
        ),
        "textDocument/documentSymbol" => to_json(
            &backend
                .document_symbol(parse_params::<DocumentSymbolParams>(params)?)
                .await?,
        ),
        "workspace/symbol" => to_json(
            &backend
                .symbol(parse_params::<WorkspaceSymbolParams>(params)?)
                .await?,
        ),
        "workspaceSymbol/resolve" => to_json(
            &backend
                .workspace_symbol_resolve(parse_params::<WorkspaceSymbol>(params)?)
                .await?,
        ),
        "textDocument/signatureHelp" => to_json(
            &backend
                .signature_help(parse_params::<SignatureHelpParams>(params)?)
                .await?,
        ),
        "textDocument/codeAction" => to_json(
            &backend
                .code_action(parse_params::<CodeActionParams>(params)?)
                .await?,
        ),
        "codeAction/resolve" => to_json(
            &backend
                .code_action_resolve(parse_params::<CodeAction>(params)?)
                .await?,
        ),
        "textDocument/rename" => to_json(
            &backend
                .rename(parse_params::<RenameParams>(params)?)
                .await?,
        ),
        "textDocument/prepareRename" => to_json(
            &backend
                .prepare_rename(parse_params::<TextDocumentPositionParams>(params)?)
                .await?,
        ),
        "workspace/executeCommand" => to_json(
            &backend
                .execute_command(parse_params::<ExecuteCommandParams>(params)?)
                .await?,
        ),
        "textDocument/formatting" => to_json(
            &backend
                .formatting(parse_params::<DocumentFormattingParams>(params)?)
                .await?,
        ),
        "textDocument/rangeFormatting" => to_json(
            &backend
                .range_formatting(parse_params::<DocumentRangeFormattingParams>(params)?)
                .await?,
        ),
        "textDocument/onTypeFormatting" => to_json(
            &backend
                .on_type_formatting(parse_params::<DocumentOnTypeFormattingParams>(params)?)
                .await?,
        ),
        "textDocument/foldingRange" => to_json(
            &backend
                .folding_range(parse_params::<FoldingRangeParams>(params)?)
                .await?,
        ),
        "textDocument/selectionRange" => to_json(
            &backend
                .selection_range(parse_params::<SelectionRangeParams>(params)?)
                .await?,
        ),
        "textDocument/codeLens" => to_json(
            &backend
                .code_lens(parse_params::<CodeLensParams>(params)?)
                .await?,
        ),
        "codeLens/resolve" => to_json(
            &backend
                .code_lens_resolve(parse_params::<CodeLens>(params)?)
                .await?,
        ),
        "textDocument/documentLink" => to_json(
            &backend
                .document_link(parse_params::<DocumentLinkParams>(params)?)
                .await?,
        ),
        "documentLink/resolve" => to_json(
            &backend
                .document_link_resolve(parse_params::<DocumentLink>(params)?)
                .await?,
        ),
        "textDocument/documentColor" => to_json(
            &backend
                .document_color(parse_params::<DocumentColorParams>(params)?)
                .await?,
        ),
        "textDocument/colorPresentation" => to_json(
            &backend
                .color_presentation(parse_params::<ColorPresentationParams>(params)?)
                .await?,
        ),
        "textDocument/semanticTokens/full" => to_json(
            &backend
                .semantic_tokens_full(parse_params::<SemanticTokensParams>(params)?)
                .await?,
        ),
        "textDocument/semanticTokens/full/delta" => to_json(
            &backend
                .semantic_tokens_full_delta(parse_params::<SemanticTokensDeltaParams>(params)?)
                .await?,
        ),
        "textDocument/semanticTokens/range" => to_json(
            &backend
                .semantic_tokens_range(parse_params::<SemanticTokensRangeParams>(params)?)
                .await?,
        ),
        "textDocument/inlayHint" => to_json(
            &backend
                .inlay_hint(parse_params::<InlayHintParams>(params)?)
                .await?,
        ),
        "inlayHint/resolve" => to_json(
            &backend
                .inlay_hint_resolve(parse_params::<InlayHint>(params)?)
                .await?,
        ),
        "textDocument/diagnostic" => to_json(
            &backend
                .diagnostic(parse_params::<DocumentDiagnosticParams>(params)?)
                .await?,
        ),
        "workspace/diagnostic" => to_json(
            &backend
                .workspace_diagnostic(parse_params::<WorkspaceDiagnosticParams>(params)?)
                .await?,
        ),
        "textDocument/willSaveWaitUntil" => to_json(
            &backend
                .will_save_wait_until(parse_params::<WillSaveTextDocumentParams>(params)?)
                .await?,
        ),
        "workspace/willCreateFiles" => to_json(
            &backend
                .will_create_files(parse_params::<CreateFilesParams>(params)?)
                .await?,
        ),
        "workspace/willRenameFiles" => to_json(
            &backend
                .will_rename_files(parse_params::<RenameFilesParams>(params)?)
                .await?,
        ),
        "workspace/willDeleteFiles" => to_json(
            &backend
                .will_delete_files(parse_params::<DeleteFilesParams>(params)?)
                .await?,
        ),
        "textDocument/prepareCallHierarchy" => to_json(
            &backend
                .prepare_call_hierarchy(parse_params::<CallHierarchyPrepareParams>(params)?)
                .await?,
        ),
        "callHierarchy/incomingCalls" => to_json(
            &backend
                .incoming_calls(parse_params::<CallHierarchyIncomingCallsParams>(params)?)
                .await?,
        ),
        "callHierarchy/outgoingCalls" => to_json(
            &backend
                .outgoing_calls(parse_params::<CallHierarchyOutgoingCallsParams>(params)?)
                .await?,
        ),
        "textDocument/prepareTypeHierarchy" => to_json(
            &backend
                .prepare_type_hierarchy(parse_params::<TypeHierarchyPrepareParams>(params)?)
                .await?,
        ),
        "typeHierarchy/supertypes" => to_json(
            &backend
                .supertypes(parse_params::<TypeHierarchySupertypesParams>(params)?)
                .await?,
        ),
        "typeHierarchy/subtypes" => to_json(
            &backend
                .subtypes(parse_params::<TypeHierarchySubtypesParams>(params)?)
                .await?,
        ),
        "textDocument/moniker" => to_json(
            &backend
                .moniker(parse_params::<MonikerParams>(params)?)
                .await?,
        ),
        "textDocument/linkedEditingRange" => to_json(
            &backend
                .linked_editing_range(parse_params::<LinkedEditingRangeParams>(params)?)
                .await?,
        ),
        "textDocument/inlineValue" => to_json(
            &backend
                .inline_value(parse_params::<InlineValueParams>(params)?)
                .await?,
        ),
        "textDocument/inlineCompletion" => to_json(
            &backend
                .inline_completion(parse_params::<InlineCompletionParams>(params)?)
                .await?,
        ),
        _ => backend.handle_request(method, params).await,
    }
}

/// Route a notification to the matching typed handler, falling back to the
/// untyped [`LanguageServer::handle_notification`] escape hatch.
///
/// Malformed params for a known notification are logged (notifications have no
/// response channel) rather than silently dropped.
async fn dispatch_notification<B: LanguageServer>(
    backend: &B,
    client: &Client,
    method: &str,
    params: Option<Value>,
) {
    match method {
        "textDocument/didOpen" => match parse_params::<DidOpenTextDocumentParams>(params) {
            Ok(p) => backend.did_open(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "textDocument/didChange" => match parse_params::<DidChangeTextDocumentParams>(params) {
            Ok(p) => backend.did_change(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "textDocument/didClose" => match parse_params::<DidCloseTextDocumentParams>(params) {
            Ok(p) => backend.did_close(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "textDocument/didSave" => match parse_params::<DidSaveTextDocumentParams>(params) {
            Ok(p) => backend.did_save(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "workspace/didChangeWorkspaceFolders" => {
            match parse_params::<DidChangeWorkspaceFoldersParams>(params) {
                Ok(p) => backend.did_change_workspace_folders(p).await,
                Err(err) => log_bad_params(client, method, &err),
            }
        }
        "window/workDoneProgress/cancel" => {
            match parse_params::<WorkDoneProgressCancelParams>(params) {
                Ok(p) => backend.work_done_progress_cancel(p).await,
                Err(err) => log_bad_params(client, method, &err),
            }
        }
        "workspace/didChangeConfiguration" => {
            match parse_params::<DidChangeConfigurationParams>(params) {
                Ok(p) => backend.did_change_configuration(p).await,
                Err(err) => log_bad_params(client, method, &err),
            }
        }
        "workspace/didChangeWatchedFiles" => {
            match parse_params::<DidChangeWatchedFilesParams>(params) {
                Ok(p) => backend.did_change_watched_files(p).await,
                Err(err) => log_bad_params(client, method, &err),
            }
        }
        "textDocument/willSave" => match parse_params::<WillSaveTextDocumentParams>(params) {
            Ok(p) => backend.will_save(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "workspace/didCreateFiles" => match parse_params::<CreateFilesParams>(params) {
            Ok(p) => backend.did_create_files(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "workspace/didRenameFiles" => match parse_params::<RenameFilesParams>(params) {
            Ok(p) => backend.did_rename_files(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "workspace/didDeleteFiles" => match parse_params::<DeleteFilesParams>(params) {
            Ok(p) => backend.did_delete_files(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "$/setTrace" => match parse_params::<SetTraceParams>(params) {
            Ok(p) => backend.set_trace(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "notebookDocument/didOpen" => match parse_params::<DidOpenNotebookDocumentParams>(params) {
            Ok(p) => backend.did_open_notebook_document(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "notebookDocument/didChange" => {
            match parse_params::<DidChangeNotebookDocumentParams>(params) {
                Ok(p) => backend.did_change_notebook_document(p).await,
                Err(err) => log_bad_params(client, method, &err),
            }
        }
        "notebookDocument/didSave" => match parse_params::<DidSaveNotebookDocumentParams>(params) {
            Ok(p) => backend.did_save_notebook_document(p).await,
            Err(err) => log_bad_params(client, method, &err),
        },
        "notebookDocument/didClose" => {
            match parse_params::<DidCloseNotebookDocumentParams>(params) {
                Ok(p) => backend.did_close_notebook_document(p).await,
                Err(err) => log_bad_params(client, method, &err),
            }
        }
        _ => backend.handle_notification(method, params).await,
    }
}

/// Log a parameter-decoding failure for a notification.
fn log_bad_params(client: &Client, method: &str, err: &Error) {
    let _ = client.log_message(
        MessageType::Warning,
        format!("ignoring `{method}` with invalid params: {err}"),
    );
}

/// Deserialize request/notification params, mapping failures to
/// [`Error::invalid_params`]. Per JSON-RPC, `params` is optional; absent
/// params are treated as an empty JSON object so structs whose fields are
/// all optional/defaultable still decode successfully.
fn parse_params<T: DeserializeOwned>(params: Option<Value>) -> Result<T> {
    serde_json::from_value(params.unwrap_or_else(|| Value::Object(Default::default())))
        .map_err(|e| Error::invalid_params(e.to_string()))
}

/// Serialize a handler result into the JSON value sent on the wire.
fn to_json<T: Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(Error::from)
}

/// Enqueue an error response for `id`.
fn send_error(out_tx: &Outbound, id: RequestId, err: Error) {
    let _ = out_tx.send(Response::error(Some(id), err.into_response_error()).into());
}

/// Lock a mutex, recovering the guard if a panicking task poisoned it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, PartialEq, Deserialize)]
    struct AllOptionalParams {
        #[serde(default)]
        foo: Option<i32>,
    }

    #[test]
    fn parse_params_treats_absent_params_as_empty_object() {
        let parsed: AllOptionalParams = parse_params(None).expect("absent params should default");
        assert_eq!(parsed, AllOptionalParams::default());
    }

    #[test]
    fn parse_params_still_rejects_wrong_shape() {
        let err = parse_params::<AllOptionalParams>(Some(Value::Bool(true)))
            .expect_err("a bool is not a valid params object");
        assert!(matches!(err, Error::Response(e) if e.code == crate::error::codes::INVALID_PARAMS));
    }
}
