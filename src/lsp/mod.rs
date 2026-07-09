//! LSP protocol data types.
//!
//! These are plain serde structs/enums modelling the subset of the Language
//! Server Protocol that the framework dispatches to typed handlers, plus the
//! notifications a server commonly sends back to the client (diagnostics, log
//! and show-message). Anything outside this subset can still be handled through
//! the untyped escape hatches on [`crate::LanguageServer`] and advertised via
//! [`ServerCapabilities::extra`].
//!
//! All types follow the LSP JSON conventions: `camelCase` field names and
//! integer discriminants for the enums in [`enums`].

pub mod base;
pub mod diagnostics;
pub mod document;
pub mod enums;
pub mod features;
pub mod lifecycle;
pub mod progress;
pub mod window;
pub mod workspace;

pub use base::{
    Location, Position, Range, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, VersionedTextDocumentIdentifier,
};
pub use diagnostics::{Diagnostic, PublishDiagnosticsParams};
pub use document::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, TextDocumentContentChangeEvent,
};
pub use enums::{
    CompletionItemKind, CompletionTriggerKind, DiagnosticSeverity, MarkupKind, MessageType,
    PositionEncodingKind, TextDocumentSyncKind,
};
pub use features::{
    CompletionContext, CompletionItem, CompletionList, CompletionParams, CompletionResponse,
    DefinitionParams, GotoDefinitionResponse, Hover, HoverParams, MarkupContent,
};
pub use lifecycle::{
    ClientCapabilities, ClientInfo, CompletionOptions, InitializeParams, InitializeResult,
    ServerCapabilities, ServerInfo,
};
pub use progress::{
    ProgressParams, ProgressToken, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCancelParams, WorkDoneProgressCreateParams, WorkDoneProgressEnd,
    WorkDoneProgressReport,
};
pub use window::{LogMessageParams, ShowMessageParams};
pub use workspace::{
    ApplyWorkspaceEditParams, ApplyWorkspaceEditResult, ConfigurationItem, ConfigurationParams,
    DidChangeWorkspaceFoldersParams, TextEdit, WorkspaceEdit, WorkspaceFolder,
    WorkspaceFoldersChangeEvent,
};
