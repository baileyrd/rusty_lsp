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
pub mod code_action;
pub mod diagnostics;
pub mod document;
pub mod enums;
pub mod features;
pub mod lifecycle;
pub mod progress;
pub mod rename;
pub mod signature;
pub mod symbols;
pub mod window;
pub mod workspace;

pub use base::{
    Location, Position, Range, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, VersionedTextDocumentIdentifier,
};
pub use code_action::{
    CodeAction, CodeActionContext, CodeActionDisabled, CodeActionKind, CodeActionOptions,
    CodeActionOrCommand, CodeActionParams, Command, code_action_kind,
};
pub use diagnostics::{Diagnostic, PublishDiagnosticsParams};
pub use document::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, TextDocumentContentChangeEvent,
};
pub use enums::{
    CodeActionTriggerKind, CompletionItemKind, CompletionTriggerKind, DiagnosticSeverity,
    FileChangeType, MarkupKind, MessageType, PositionEncodingKind, SignatureHelpTriggerKind,
    SymbolKind, SymbolTag, TextDocumentSyncKind,
};
pub use features::{
    CompletionContext, CompletionItem, CompletionList, CompletionParams, CompletionResponse,
    DefinitionParams, GotoDefinitionResponse, Hover, HoverParams, MarkupContent, ReferenceContext,
    ReferenceParams,
};
pub use lifecycle::{
    ClientCapabilities, ClientInfo, CodeActionProviderCapability, CompletionOptions,
    InitializeParams, InitializeResult, RenameProviderCapability, ServerCapabilities, ServerInfo,
    SignatureHelpOptions,
};
pub use progress::{
    ProgressParams, ProgressToken, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCancelParams, WorkDoneProgressCreateParams, WorkDoneProgressEnd,
    WorkDoneProgressReport,
};
pub use rename::{PrepareRenameResponse, RenameOptions, RenameParams};
pub use signature::{
    Documentation, ParameterInformation, ParameterLabel, SignatureHelp, SignatureHelpContext,
    SignatureHelpParams, SignatureInformation,
};
pub use symbols::{
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, SymbolInformation,
    WorkspaceSymbolParams,
};
pub use window::{LogMessageParams, ShowMessageParams};
pub use workspace::{
    ApplyWorkspaceEditParams, ApplyWorkspaceEditResult, ConfigurationItem, ConfigurationParams,
    DidChangeConfigurationParams, DidChangeWatchedFilesParams, DidChangeWorkspaceFoldersParams,
    ExecuteCommandOptions, ExecuteCommandParams, FileEvent, TextEdit, WorkspaceEdit,
    WorkspaceFolder, WorkspaceFoldersChangeEvent,
};
