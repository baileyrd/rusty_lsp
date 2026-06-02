//! The [`LanguageServer`] trait — the single extension point applications
//! implement to provide language behaviour.
//!
//! The framework owns transport, framing, JSON-RPC dispatch, lifecycle, and
//! cancellation; an implementor only fills in the handlers it cares about.
//! Every method except [`initialize`](LanguageServer::initialize) has a
//! sensible default (no-op for notifications, "unsupported" for requests), so a
//! minimal server is just an `initialize` returning its capabilities.
//!
//! ## Why `-> impl Future + Send` instead of `async fn`
//!
//! The trait declares async methods in their desugared form,
//! `fn m(&self, ..) -> impl Future<Output = T> + Send`, rather than
//! `async fn m(&self, ..) -> T`. The explicit `+ Send` bound lets the
//! [`crate::Server`] spawn request handlers onto a multi-threaded runtime
//! without an `async-trait`-style boxing layer. Implementors may still write
//! the bodies as ordinary `async fn` — an `async fn` whose body is `Send`
//! satisfies the desugared signature.

use crate::error::{Error, Result};
use crate::lsp::{
    CompletionParams, CompletionResponse, DefinitionParams, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    GotoDefinitionResponse, Hover, HoverParams, InitializeParams, InitializeResult,
};
use serde_json::Value;

/// Implement this trait to define a language server's behaviour.
///
/// Construct your backend from a [`crate::Client`] (handed to you by
/// [`crate::Server::serve`]) so handlers can push diagnostics and messages back
/// to the editor. The backend is shared across concurrent requests behind an
/// `Arc`, so all methods take `&self`; use interior mutability
/// (`tokio::sync::RwLock`, `Mutex`, atomics, …) for any state you mutate.
pub trait LanguageServer: Send + Sync + 'static {
    /// Handle the `initialize` request.
    ///
    /// Called exactly once, before any other request. Return the server's
    /// [`InitializeResult`], primarily its
    /// [`ServerCapabilities`](crate::lsp::ServerCapabilities). This is the only
    /// method without a default, since a server must advertise what it does.
    fn initialize(
        &self,
        params: InitializeParams,
    ) -> impl Future<Output = Result<InitializeResult>> + Send;

    /// Handle the `initialized` notification (the client is ready).
    ///
    /// A good place to register dynamic capabilities or start background work.
    fn initialized(&self) -> impl Future<Output = ()> + Send {
        async {}
    }

    /// Handle the `shutdown` request.
    ///
    /// The server should release resources but must keep running until it
    /// receives the subsequent `exit` notification.
    fn shutdown(&self) -> impl Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }

    /// Handle `textDocument/didOpen`.
    fn did_open(&self, params: DidOpenTextDocumentParams) -> impl Future<Output = ()> + Send {
        let _ = params;
        async {}
    }

    /// Handle `textDocument/didChange`.
    fn did_change(&self, params: DidChangeTextDocumentParams) -> impl Future<Output = ()> + Send {
        let _ = params;
        async {}
    }

    /// Handle `textDocument/didClose`.
    fn did_close(&self, params: DidCloseTextDocumentParams) -> impl Future<Output = ()> + Send {
        let _ = params;
        async {}
    }

    /// Handle `textDocument/didSave`.
    fn did_save(&self, params: DidSaveTextDocumentParams) -> impl Future<Output = ()> + Send {
        let _ = params;
        async {}
    }

    /// Handle `textDocument/hover`. Return `Ok(None)` for "no hover here".
    fn hover(&self, params: HoverParams) -> impl Future<Output = Result<Option<Hover>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/completion`.
    fn completion(
        &self,
        params: CompletionParams,
    ) -> impl Future<Output = Result<Option<CompletionResponse>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/definition`.
    fn definition(
        &self,
        params: DefinitionParams,
    ) -> impl Future<Output = Result<Option<GotoDefinitionResponse>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Fallback for request methods the framework does not model.
    ///
    /// Override to support additional typed requests (formatting, code actions,
    /// references, …). Deserialize `params` yourself and return a JSON value to
    /// be sent as the result. The default reports
    /// [`METHOD_NOT_FOUND`](crate::error::codes::METHOD_NOT_FOUND).
    fn handle_request(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> impl Future<Output = Result<Value>> + Send {
        let _ = params;
        let method = method.to_owned();
        async move {
            Err(Error::method_not_found(format!(
                "method not found: {method}"
            )))
        }
    }

    /// Fallback for notification methods the framework does not model.
    ///
    /// The default silently ignores the notification, which is the correct
    /// behaviour for unknown notifications per the LSP spec.
    fn handle_notification(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> impl Future<Output = ()> + Send {
        let _ = (method, params);
        async {}
    }
}
