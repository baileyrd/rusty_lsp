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
    CodeAction, CodeActionOrCommand, CodeActionParams, CodeLens, CodeLensParams, ColorInformation,
    ColorPresentation, ColorPresentationParams, CompletionItem, CompletionParams,
    CompletionResponse, DefinitionParams, DidChangeConfigurationParams,
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams, DidChangeWorkspaceFoldersParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentColorParams, DocumentDiagnosticParams, DocumentDiagnosticReport,
    DocumentFormattingParams, DocumentLink, DocumentLinkParams, DocumentOnTypeFormattingParams,
    DocumentRangeFormattingParams, DocumentSymbolParams, DocumentSymbolResponse,
    ExecuteCommandParams, FoldingRange, FoldingRangeParams, FullDocumentDiagnosticReport,
    GotoDefinitionResponse, Hover, HoverParams, InitializeParams, InitializeResult, InlayHint,
    InlayHintParams, Location, PrepareRenameResponse, ReferenceParams, RenameParams,
    SelectionRange, SelectionRangeParams, SemanticTokens, SemanticTokensDeltaParams,
    SemanticTokensDeltaResult, SemanticTokensParams, SemanticTokensRangeParams, SignatureHelp,
    SignatureHelpParams, SymbolInformation, TextDocumentPositionParams, TextEdit,
    WorkDoneProgressCancelParams, WorkspaceDiagnosticParams, WorkspaceDiagnosticReport,
    WorkspaceEdit, WorkspaceSymbolParams,
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

    /// Handle `textDocument/declaration`.
    fn declaration(
        &self,
        params: TextDocumentPositionParams,
    ) -> impl Future<Output = Result<Option<GotoDefinitionResponse>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/typeDefinition`.
    fn type_definition(
        &self,
        params: TextDocumentPositionParams,
    ) -> impl Future<Output = Result<Option<GotoDefinitionResponse>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/implementation`.
    fn implementation(
        &self,
        params: TextDocumentPositionParams,
    ) -> impl Future<Output = Result<Option<GotoDefinitionResponse>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/references`.
    fn references(
        &self,
        params: ReferenceParams,
    ) -> impl Future<Output = Result<Option<Vec<Location>>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `completionItem/resolve`.
    ///
    /// The default returns `item` unchanged, which is only correct if
    /// completion's `resolve_provider` is left unset/`false` in
    /// [`crate::lsp::CompletionOptions`] — override this alongside
    /// advertising resolve support.
    fn completion_resolve(
        &self,
        item: CompletionItem,
    ) -> impl Future<Output = Result<CompletionItem>> + Send {
        async { Ok(item) }
    }

    /// Handle `textDocument/documentSymbol`.
    fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> impl Future<Output = Result<Option<DocumentSymbolResponse>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `workspace/symbol`.
    fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> impl Future<Output = Result<Option<Vec<SymbolInformation>>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/signatureHelp`.
    fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> impl Future<Output = Result<Option<SignatureHelp>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/codeAction`.
    fn code_action(
        &self,
        params: CodeActionParams,
    ) -> impl Future<Output = Result<Option<Vec<CodeActionOrCommand>>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `codeAction/resolve`.
    ///
    /// The default returns `action` unchanged, which is only correct if
    /// [`crate::lsp::ServerCapabilities::code_action_provider`]'s resolve
    /// support is left unset/`false` — override this alongside advertising
    /// resolve support.
    fn code_action_resolve(
        &self,
        action: CodeAction,
    ) -> impl Future<Output = Result<CodeAction>> + Send {
        async { Ok(action) }
    }

    /// Handle `textDocument/rename`.
    fn rename(
        &self,
        params: RenameParams,
    ) -> impl Future<Output = Result<Option<WorkspaceEdit>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/prepareRename`.
    fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> impl Future<Output = Result<Option<PrepareRenameResponse>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `workspace/didChangeConfiguration`.
    fn did_change_configuration(
        &self,
        params: DidChangeConfigurationParams,
    ) -> impl Future<Output = ()> + Send {
        let _ = params;
        async {}
    }

    /// Handle `workspace/didChangeWatchedFiles`.
    fn did_change_watched_files(
        &self,
        params: DidChangeWatchedFilesParams,
    ) -> impl Future<Output = ()> + Send {
        let _ = params;
        async {}
    }

    /// Handle `workspace/executeCommand`.
    fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> impl Future<Output = Result<Option<Value>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `workspace/didChangeWorkspaceFolders`, sent when the client
    /// adds or removes root folders in a multi-root workspace.
    fn did_change_workspace_folders(
        &self,
        params: DidChangeWorkspaceFoldersParams,
    ) -> impl Future<Output = ()> + Send {
        let _ = params;
        async {}
    }

    /// Handle `window/workDoneProgress/cancel`, sent when the user cancels a
    /// [`Client`](crate::Client)-reported progress sequence from the client
    /// UI. The default is a no-op; override to actually abort the work.
    fn work_done_progress_cancel(
        &self,
        params: WorkDoneProgressCancelParams,
    ) -> impl Future<Output = ()> + Send {
        let _ = params;
        async {}
    }

    /// Handle `textDocument/formatting`.
    fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> impl Future<Output = Result<Option<Vec<TextEdit>>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/rangeFormatting`.
    fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> impl Future<Output = Result<Option<Vec<TextEdit>>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/onTypeFormatting`.
    fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> impl Future<Output = Result<Option<Vec<TextEdit>>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/foldingRange`.
    fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> impl Future<Output = Result<Option<Vec<FoldingRange>>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/selectionRange`.
    fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> impl Future<Output = Result<Option<Vec<SelectionRange>>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/codeLens`.
    fn code_lens(
        &self,
        params: CodeLensParams,
    ) -> impl Future<Output = Result<Option<Vec<CodeLens>>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `codeLens/resolve`.
    ///
    /// The default returns `lens` unchanged, which is only correct if
    /// [`crate::lsp::ServerCapabilities::code_lens_provider`]'s resolve
    /// support is left unset/`false` — override this alongside advertising
    /// resolve support.
    fn code_lens_resolve(&self, lens: CodeLens) -> impl Future<Output = Result<CodeLens>> + Send {
        async { Ok(lens) }
    }

    /// Handle `textDocument/documentLink`.
    fn document_link(
        &self,
        params: DocumentLinkParams,
    ) -> impl Future<Output = Result<Option<Vec<DocumentLink>>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `documentLink/resolve`.
    ///
    /// The default returns `link` unchanged, which is only correct if
    /// [`crate::lsp::ServerCapabilities::document_link_provider`]'s resolve
    /// support is left unset/`false` — override this alongside advertising
    /// resolve support.
    fn document_link_resolve(
        &self,
        link: DocumentLink,
    ) -> impl Future<Output = Result<DocumentLink>> + Send {
        async { Ok(link) }
    }

    /// Handle `textDocument/documentColor`.
    fn document_color(
        &self,
        params: DocumentColorParams,
    ) -> impl Future<Output = Result<Vec<ColorInformation>>> + Send {
        let _ = params;
        async { Ok(Vec::new()) }
    }

    /// Handle `textDocument/colorPresentation`.
    fn color_presentation(
        &self,
        params: ColorPresentationParams,
    ) -> impl Future<Output = Result<Vec<ColorPresentation>>> + Send {
        let _ = params;
        async { Ok(Vec::new()) }
    }

    /// Handle `textDocument/semanticTokens/full`.
    fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> impl Future<Output = Result<Option<SemanticTokens>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/semanticTokens/full/delta`.
    fn semantic_tokens_full_delta(
        &self,
        params: SemanticTokensDeltaParams,
    ) -> impl Future<Output = Result<Option<SemanticTokensDeltaResult>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/semanticTokens/range`.
    fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> impl Future<Output = Result<Option<SemanticTokens>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `textDocument/inlayHint`.
    fn inlay_hint(
        &self,
        params: InlayHintParams,
    ) -> impl Future<Output = Result<Option<Vec<InlayHint>>>> + Send {
        let _ = params;
        async { Ok(None) }
    }

    /// Handle `inlayHint/resolve`.
    ///
    /// The default returns `hint` unchanged, which is only correct if
    /// [`crate::lsp::ServerCapabilities::inlay_hint_provider`]'s resolve
    /// support is left unset/`false` — override this alongside advertising
    /// resolve support.
    fn inlay_hint_resolve(
        &self,
        hint: InlayHint,
    ) -> impl Future<Output = Result<InlayHint>> + Send {
        async { Ok(hint) }
    }

    /// Handle `textDocument/diagnostic`.
    ///
    /// The default reports an empty, fresh
    /// [`DocumentDiagnosticReport::Full`] result.
    fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> impl Future<Output = Result<DocumentDiagnosticReport>> + Send {
        let _ = params;
        async {
            Ok(DocumentDiagnosticReport::Full(
                FullDocumentDiagnosticReport::default(),
            ))
        }
    }

    /// Handle `workspace/diagnostic`.
    ///
    /// The default reports no items; override alongside setting
    /// [`crate::lsp::ServerCapabilities::diagnostic_provider`]'s
    /// `workspace_diagnostics` to advertise real support.
    fn workspace_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> impl Future<Output = Result<WorkspaceDiagnosticReport>> + Send {
        let _ = params;
        async { Ok(WorkspaceDiagnosticReport::default()) }
    }

    /// Fallback for request methods the framework does not model.
    ///
    /// Override to support additional typed requests (call hierarchy,
    /// notebook sync, …). Deserialize `params` yourself and return a JSON
    /// value to be sent as the result. The default reports
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
