//! Lifecycle messages: `initialize` / `initialized` / `shutdown` and the
//! capability negotiation that rides along with them.

use super::base::Uri;
use super::code_lens::CodeLensOptions;
use super::diagnostics::DiagnosticOptions;
use super::enums::PositionEncodingKind;
use super::file_operations::FileOperationsServerCapabilities;
use super::formatting::DocumentOnTypeFormattingOptions;
use super::hierarchy::{CallHierarchyOptions, TypeHierarchyOptions};
use super::inlay_hint::InlayHintOptions;
use super::links::DocumentLinkOptions;
use super::notebook::NotebookDocumentSyncOptions;
use super::semantic_tokens::SemanticTokensOptions;
use super::symbols::WorkspaceSymbolOptions;
use super::workspace::{ExecuteCommandOptions, WorkspaceFolder};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Parameters of the `initialize` request.
///
/// Only the broadly useful fields are modelled as named fields; anything else
/// the client sends (`trace`, `locale`, …) is preserved verbatim in
/// [`extra`](Self::extra) rather than dropped, mirroring
/// [`ServerCapabilities::extra`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// The process id of the parent process that started the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_id: Option<i32>,
    /// Information about the client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_info: Option<ClientInfo>,
    /// The root URI of the workspace, if any. Deprecated by the spec in
    /// favour of [`workspace_folders`](Self::workspace_folders), but still
    /// sent by some clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_uri: Option<Uri>,
    /// The workspace folders configured, for multi-root workspaces. `None`
    /// when the client doesn't support multi-root and sent `root_uri`
    /// instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_folders: Option<Vec<WorkspaceFolder>>,
    /// Capabilities advertised by the client.
    #[serde(default)]
    pub capabilities: ClientCapabilities,
    /// Server-defined initialization options passed by the client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initialization_options: Option<Value>,
    /// Any fields not modelled above (e.g. `trace`, `locale`), preserved so
    /// backends can still read them.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl InitializeParams {
    /// The workspace's root folders, unified across the two ways clients
    /// report them: [`workspace_folders`](Self::workspace_folders) when the
    /// client supports multi-root, else a single folder synthesized from the
    /// deprecated [`root_uri`](Self::root_uri). Empty when the client sent
    /// neither (e.g. a single loose file is open).
    pub fn workspace_roots(&self) -> Vec<WorkspaceFolder> {
        if let Some(folders) = &self.workspace_folders {
            return folders.clone();
        }
        self.root_uri
            .as_ref()
            .map(|uri| {
                let name = uri.rsplit('/').next().unwrap_or("").to_owned();
                vec![WorkspaceFolder {
                    uri: uri.clone(),
                    name,
                }]
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extra_fields_round_trip() {
        let value = json!({
            "capabilities": {},
            "trace": "off",
            "locale": "en-US",
        });
        let params: InitializeParams = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(params.extra.get("trace"), Some(&json!("off")));
        assert_eq!(params.extra.get("locale"), Some(&json!("en-US")));

        let round_tripped = serde_json::to_value(&params).unwrap();
        assert_eq!(round_tripped["trace"], value["trace"]);
        assert_eq!(round_tripped["locale"], value["locale"]);
    }

    #[test]
    fn workspace_folders_are_a_typed_field() {
        let value = json!({
            "capabilities": {},
            "workspaceFolders": [{"uri": "file:///a", "name": "a"}],
        });
        let params: InitializeParams = serde_json::from_value(value).unwrap();
        assert_eq!(
            params.workspace_folders,
            Some(vec![WorkspaceFolder {
                uri: "file:///a".into(),
                name: "a".to_owned(),
            }])
        );
        // Not duplicated into `extra`.
        assert!(params.extra.get("workspaceFolders").is_none());
    }

    #[test]
    fn client_capabilities_get_and_supports_walk_dotted_paths() {
        let caps: ClientCapabilities = serde_json::from_value(json!({
            "textDocument": {
                "definition": {"linkSupport": true},
                "publishDiagnostics": {"versionSupport": false},
                "hover": {},
            },
        }))
        .unwrap();

        assert_eq!(
            caps.get("textDocument.definition.linkSupport"),
            Some(&json!(true))
        );
        assert!(caps.supports("textDocument.definition.linkSupport"));
        assert!(caps.supports("textDocument.hover")); // present, non-bool -> "yes"
        assert!(!caps.supports("textDocument.publishDiagnostics.versionSupport"));
        assert!(!caps.supports("textDocument.codeAction")); // missing entirely
        assert!(!caps.supports("workspace.applyEdit")); // missing top-level segment
    }
}

/// Information about the client implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    /// The client's name (e.g. `"Visual Studio Code"`).
    pub name: String,
    /// The client's version string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Capabilities advertised by the client.
///
/// The capability tree is large and evolves with the spec, so it is kept as a
/// raw JSON object. Backends that need to branch on a specific capability can
/// inspect [`ClientCapabilities::raw`] directly, or use
/// [`get`](Self::get)/[`supports`](Self::supports) to walk a dotted path
/// without hand-rolling the `Map`/`Value` traversal; the whole structure
/// round-trips losslessly either way.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientCapabilities {
    /// The full, untyped capability object as sent by the client.
    pub raw: Map<String, Value>,
}

impl ClientCapabilities {
    /// Look up a dotted path into the raw capability tree, e.g.
    /// `"textDocument.definition.linkSupport"`. Returns `None` if any
    /// segment is missing.
    pub fn get(&self, path: &str) -> Option<&Value> {
        let mut segments = path.split('.');
        let mut value = self.raw.get(segments.next()?)?;
        for segment in segments {
            value = value.as_object()?.get(segment)?;
        }
        Some(value)
    }

    /// Whether `path` (see [`get`](Self::get)) resolves to a "yes" — either
    /// the JSON literal `true`, or any present, non-`null`, non-`false`
    /// value (the spec often signals "supported" with a nested options
    /// object rather than a bare boolean, e.g.
    /// `textDocument.publishDiagnostics.versionSupport`).
    pub fn supports(&self, path: &str) -> bool {
        !matches!(
            self.get(path),
            None | Some(Value::Null) | Some(Value::Bool(false))
        )
    }
}

/// Result of the `initialize` request.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// The capabilities the server provides.
    pub capabilities: ServerCapabilities,
    /// Information about the server implementation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_info: Option<ServerInfo>,
}

/// Information about the server implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    /// The server's name.
    pub name: String,
    /// The server's version string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Capabilities the server provides, advertised in [`InitializeResult`].
///
/// The modelled fields cover the features this framework dispatches to typed
/// trait methods. Anything else — semantic tokens, code actions, formatting,
/// and so on — can be advertised through [`ServerCapabilities::extra`], which is
/// flattened into the same JSON object.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    /// The character encoding the server chose for [`crate::lsp::Position`],
    /// selected from the client's `capabilities.general.positionEncodings`
    /// (LSP 3.17). `None` means UTF-16, the base-spec default, and matches
    /// the encoding every conversion in [`crate::text`] assumes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_encoding: Option<PositionEncodingKind>,
    /// How the server wants document content synchronised: a bare
    /// [`TextDocumentSyncKind`](super::enums::TextDocumentSyncKind)
    /// (converts via `.into()`) or the full
    /// [`TextDocumentSyncOptions`](super::document::TextDocumentSyncOptions)
    /// form (open/close, will-save, save-with-text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_document_sync: Option<super::document::TextDocumentSyncCapability>,
    /// Whether the server provides hover support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hover_provider: Option<bool>,
    /// Completion support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_provider: Option<CompletionOptions>,
    /// Whether the server provides goto-definition support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_provider: Option<bool>,
    /// Whether the server provides goto-declaration support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration_provider: Option<bool>,
    /// Whether the server provides goto-type-definition support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_definition_provider: Option<bool>,
    /// Whether the server provides goto-implementation support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation_provider: Option<bool>,
    /// Whether the server provides find-references support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references_provider: Option<bool>,
    /// Whether the server provides document-highlight support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_highlight_provider: Option<bool>,
    /// Whether the server provides document-symbol support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_symbol_provider: Option<bool>,
    /// Whether the server provides workspace-symbol support (optionally with
    /// `workspaceSymbol/resolve`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_symbol_provider: Option<WorkspaceSymbolProviderCapability>,
    /// Signature-help support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_help_provider: Option<SignatureHelpOptions>,
    /// Code-action support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_action_provider: Option<CodeActionProviderCapability>,
    /// Rename support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rename_provider: Option<RenameProviderCapability>,
    /// Command-execution support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execute_command_provider: Option<ExecuteCommandOptions>,
    /// Whether the server provides whole-document formatting support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_formatting_provider: Option<bool>,
    /// Whether the server provides range-formatting support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_range_formatting_provider: Option<bool>,
    /// On-type formatting support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_on_type_formatting_provider: Option<DocumentOnTypeFormattingOptions>,
    /// Whether the server provides folding-range support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folding_range_provider: Option<bool>,
    /// Whether the server provides selection-range support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_range_provider: Option<bool>,
    /// Code-lens support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_lens_provider: Option<CodeLensOptions>,
    /// Document-link support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_link_provider: Option<DocumentLinkOptions>,
    /// Whether the server provides document-color/color-presentation
    /// support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_provider: Option<bool>,
    /// Semantic-tokens support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_tokens_provider: Option<SemanticTokensOptions>,
    /// Inlay-hint support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inlay_hint_provider: Option<InlayHintOptions>,
    /// Diagnostic-pull-model support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_provider: Option<DiagnosticOptions>,
    /// Workspace-scoped capabilities that nest under `workspace` on the
    /// wire instead of being top-level fields (currently just
    /// [`file_operations`](WorkspaceServerCapabilities::file_operations)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceServerCapabilities>,
    /// Call-hierarchy support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_hierarchy_provider: Option<CallHierarchyProviderCapability>,
    /// Type-hierarchy support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_hierarchy_provider: Option<TypeHierarchyProviderCapability>,
    /// Notebook-document sync support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notebook_document_sync: Option<NotebookDocumentSyncOptions>,
    /// Whether the server provides `textDocument/moniker` support (LSP 3.16).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moniker_provider: Option<bool>,
    /// Whether the server provides `textDocument/linkedEditingRange` support
    /// (LSP 3.16).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_editing_range_provider: Option<bool>,
    /// Whether the server provides `textDocument/inlineValue` support
    /// (LSP 3.17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_value_provider: Option<bool>,
    /// Whether the server provides `textDocument/inlineCompletion` support
    /// (LSP 3.18, proposed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_completion_provider: Option<bool>,
    /// Any additional capabilities not modelled above.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The `workspace` sub-object of [`ServerCapabilities`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceServerCapabilities {
    /// Which file-operation hooks (`willCreateFiles`, `didRenameFiles`, …)
    /// the server wants to be told about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_operations: Option<FileOperationsServerCapabilities>,
}

/// Options describing the server's completion support.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionOptions {
    /// Characters that trigger completion automatically.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trigger_characters: Vec<String>,
    /// Whether the server resolves additional information for a selected item
    /// via `completionItem/resolve`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolve_provider: Option<bool>,
}

/// Options describing the server's `textDocument/signatureHelp` support.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureHelpOptions {
    /// Characters that trigger signature help automatically.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trigger_characters: Vec<String>,
    /// Characters that re-trigger signature help while it is already showing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retrigger_characters: Vec<String>,
}

/// Either a plain boolean or [`CodeActionOptions`](super::code_action::CodeActionOptions),
/// matching the spec's `boolean | CodeActionOptions` shape for `codeActionProvider`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CodeActionProviderCapability {
    /// `true`/`false`: code actions are (not) supported, with no further
    /// filtering or resolve support.
    Simple(bool),
    /// Code actions are supported with the given options.
    Options(super::code_action::CodeActionOptions),
}

impl Default for CodeActionProviderCapability {
    fn default() -> Self {
        CodeActionProviderCapability::Simple(false)
    }
}

/// Either a plain boolean or [`RenameOptions`](super::rename::RenameOptions),
/// matching the spec's `boolean | RenameOptions` shape for `renameProvider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RenameProviderCapability {
    /// `true`/`false`: rename is (not) supported, with no `prepareRename`.
    Simple(bool),
    /// Rename is supported with the given options.
    Options(super::rename::RenameOptions),
}

impl Default for RenameProviderCapability {
    fn default() -> Self {
        RenameProviderCapability::Simple(false)
    }
}

/// Either a plain boolean or [`WorkspaceSymbolOptions`], matching the spec's
/// `boolean | WorkspaceSymbolOptions` shape for `workspaceSymbolProvider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkspaceSymbolProviderCapability {
    /// `true`/`false`: workspace symbols are (not) supported, with no
    /// `workspaceSymbol/resolve`.
    Simple(bool),
    /// Workspace symbols are supported with the given options.
    Options(WorkspaceSymbolOptions),
}

impl Default for WorkspaceSymbolProviderCapability {
    fn default() -> Self {
        WorkspaceSymbolProviderCapability::Simple(false)
    }
}

/// Either a plain boolean or [`CallHierarchyOptions`], matching the spec's
/// `boolean | CallHierarchyOptions | CallHierarchyRegistrationOptions` shape
/// for `callHierarchyProvider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CallHierarchyProviderCapability {
    /// `true`/`false`: call hierarchy is (not) supported.
    Simple(bool),
    /// Call hierarchy is supported with the given options.
    Options(CallHierarchyOptions),
}

impl Default for CallHierarchyProviderCapability {
    fn default() -> Self {
        CallHierarchyProviderCapability::Simple(false)
    }
}

/// Either a plain boolean or [`TypeHierarchyOptions`], matching the spec's
/// `boolean | TypeHierarchyOptions | TypeHierarchyRegistrationOptions` shape
/// for `typeHierarchyProvider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TypeHierarchyProviderCapability {
    /// `true`/`false`: type hierarchy is (not) supported.
    Simple(bool),
    /// Type hierarchy is supported with the given options.
    Options(TypeHierarchyOptions),
}

impl Default for TypeHierarchyProviderCapability {
    fn default() -> Self {
        TypeHierarchyProviderCapability::Simple(false)
    }
}
