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
pub mod code_lens;
pub mod diagnostics;
pub mod document;
pub mod enums;
pub mod features;
pub mod file_operations;
pub mod formatting;
pub mod inlay_hint;
pub mod lifecycle;
pub mod links;
pub mod progress;
pub mod ranges;
pub mod rename;
pub mod semantic_tokens;
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
pub use code_lens::{CodeLens, CodeLensOptions, CodeLensParams};
pub use diagnostics::{
    Diagnostic, DiagnosticOptions, DocumentDiagnosticParams, DocumentDiagnosticReport,
    FullDocumentDiagnosticReport, PreviousResultId, PublishDiagnosticsParams,
    UnchangedDocumentDiagnosticReport, WorkspaceDiagnosticParams, WorkspaceDiagnosticReport,
    WorkspaceDocumentDiagnosticReport, WorkspaceFullDocumentDiagnosticReport,
    WorkspaceUnchangedDocumentDiagnosticReport,
};
pub use document::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, TextDocumentContentChangeEvent, WillSaveTextDocumentParams,
};
pub use enums::{
    CodeActionTriggerKind, CompletionItemKind, CompletionTriggerKind, DiagnosticSeverity,
    FileChangeType, InlayHintKind, MarkupKind, MessageType, PositionEncodingKind,
    SignatureHelpTriggerKind, SymbolKind, SymbolTag, TextDocumentSaveReason, TextDocumentSyncKind,
};
pub use features::{
    CompletionContext, CompletionItem, CompletionList, CompletionParams, CompletionResponse,
    DefinitionParams, GotoDefinitionResponse, Hover, HoverParams, MarkupContent, ReferenceContext,
    ReferenceParams,
};
pub use file_operations::{
    CreateFilesParams, DeleteFilesParams, FileCreate, FileDelete, FileOperationFilter,
    FileOperationPattern, FileOperationRegistrationOptions, FileOperationsServerCapabilities,
    FileRename, RenameFilesParams,
};
pub use formatting::{
    DocumentFormattingParams, DocumentOnTypeFormattingOptions, DocumentOnTypeFormattingParams,
    DocumentRangeFormattingParams, FormattingOptions,
};
pub use inlay_hint::{
    InlayHint, InlayHintLabel, InlayHintLabelPart, InlayHintOptions, InlayHintParams,
};
pub use lifecycle::{
    ClientCapabilities, ClientInfo, CodeActionProviderCapability, CompletionOptions,
    InitializeParams, InitializeResult, RenameProviderCapability, ServerCapabilities, ServerInfo,
    SignatureHelpOptions, WorkspaceServerCapabilities,
};
pub use links::{
    Color, ColorInformation, ColorPresentation, ColorPresentationParams, DocumentColorParams,
    DocumentLink, DocumentLinkOptions, DocumentLinkParams,
};
pub use progress::{
    PartialResultParams, ProgressParams, ProgressToken, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCancelParams, WorkDoneProgressCreateParams, WorkDoneProgressEnd,
    WorkDoneProgressParams, WorkDoneProgressReport,
};
pub use ranges::{
    FoldingRange, FoldingRangeKind, FoldingRangeParams, SelectionRange, SelectionRangeParams,
    folding_range_kind,
};
pub use rename::{PrepareRenameResponse, RenameOptions, RenameParams};
pub use semantic_tokens::{
    SemanticTokens, SemanticTokensDelta, SemanticTokensDeltaParams, SemanticTokensDeltaResult,
    SemanticTokensEdit, SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
    SemanticTokensParams, SemanticTokensRangeParams,
};
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
