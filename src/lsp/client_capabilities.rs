//! Typed views over sub-trees of [`crate::lsp::ClientCapabilities`].
//!
//! [`ClientCapabilities`](crate::lsp::ClientCapabilities) itself stays a raw
//! JSON tree by design (see its module doc comment in `lifecycle.rs`) — a
//! server author can always fall back to
//! [`get`](crate::lsp::ClientCapabilities::get)/[`supports`](crate::lsp::ClientCapabilities::supports)
//! for anything not modelled here. The types in this module are an additive,
//! typed convenience layer on top of that same data: methods like
//! [`ClientCapabilities::workspace`](crate::lsp::ClientCapabilities::workspace)
//! parse the relevant sub-tree into one of these structs, tolerating
//! malformed or absent data by falling back to the type's `Default` rather
//! than erroring — client-supplied data is untrusted, and a slightly-off
//! capability announcement shouldn't be able to crash a server.

use super::enums::{SymbolKind, SymbolTag};
use serde::{Deserialize, Serialize};

/// The common `{ dynamicRegistration?: boolean }` shape shared by many
/// capability groups that support nothing beyond opting into dynamic
/// registration via `client/registerCapability`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicRegistrationCapability {
    /// Whether the client supports dynamic registration for this method.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_registration: Option<bool>,
}

/// The common `{ refreshSupport?: boolean }` shape shared by capability
/// groups that support a server asking the client to re-pull results (e.g.
/// `workspace/semanticTokens/refresh`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshSupportCapability {
    /// Whether the client honors the corresponding `workspace/*/refresh` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_support: Option<bool>,
}

/// The common `{ properties: string[] }` shape shared by capability groups
/// that support lazily resolving additional item properties (e.g.
/// `workspaceSymbol/resolve`, `codeAction/resolve`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveSupportCapability {
    /// The property names the client can resolve lazily.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<String>,
}

/// The common `{ valueSet: T[] }` shape shared by capability groups that
/// enumerate which values of a tag-like enum the client understands (e.g.
/// `workspace.symbol.tagSupport`, `textDocument.documentSymbol.tagSupport`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
// serde's automatic bound inference would otherwise require `T: Default`
// because of the `#[serde(default)]` field below, even though `Vec<T>`
// implements `Default` unconditionally — spelled out explicitly instead.
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct TagSupportCapability<T> {
    /// The tag values the client can render.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_set: Vec<T>,
}

// Manual `Default` instead of `#[derive(Default)]`: the derive would require
// `T: Default`, but the tag enums this is used with (e.g. `SymbolTag`) don't
// implement it — an empty `Vec<T>` needs no bound on `T` at all.
impl<T> Default for TagSupportCapability<T> {
    fn default() -> Self {
        TagSupportCapability {
            value_set: Vec::new(),
        }
    }
}

/// `ClientCapabilities.workspace`: capabilities that apply to the workspace
/// as a whole rather than to a single document.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceClientCapabilities {
    /// The client supports applying batch edits via `workspace/applyEdit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_edit: Option<bool>,
    /// Capabilities specific to `WorkspaceEdit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_edit: Option<WorkspaceEditClientCapabilities>,
    /// `workspace/didChangeConfiguration` capabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_change_configuration: Option<DynamicRegistrationCapability>,
    /// `workspace/didChangeWatchedFiles` capabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_change_watched_files: Option<DidChangeWatchedFilesClientCapabilities>,
    /// `workspace/symbol` capabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<WorkspaceSymbolClientCapabilities>,
    /// `workspace/executeCommand` capabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execute_command: Option<DynamicRegistrationCapability>,
    /// The client supports `workspace/workspaceFolders` (LSP 3.6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_folders: Option<bool>,
    /// The client supports `workspace/configuration` (LSP 3.6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<bool>,
    /// The client honors `workspace/semanticTokens/refresh` (LSP 3.16).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_tokens: Option<RefreshSupportCapability>,
    /// The client honors `workspace/codeLens/refresh` (LSP 3.16).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_lens: Option<RefreshSupportCapability>,
    /// File-operation notification/request support (LSP 3.16).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_operations: Option<FileOperationClientCapabilities>,
    /// The client honors `workspace/inlineValue/refresh` (LSP 3.17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_value: Option<RefreshSupportCapability>,
    /// The client honors `workspace/inlayHint/refresh` (LSP 3.17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inlay_hint: Option<RefreshSupportCapability>,
    /// The client honors `workspace/diagnostic/refresh` (LSP 3.17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<RefreshSupportCapability>,
}

/// `ClientCapabilities.workspace.workspaceEdit`: capabilities specific to
/// `WorkspaceEdit` construction and application.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEditClientCapabilities {
    /// The client supports versioned document changes
    /// (`WorkspaceEdit::documentChanges`) over the plain `changes` map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_changes: Option<bool>,
    /// Which resource operations (create/rename/delete) the client supports
    /// in a `WorkspaceEdit` (LSP 3.13).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_operations: Vec<ResourceOperationKind>,
    /// How the client behaves if a resource operation in a `WorkspaceEdit`
    /// fails partway through (LSP 3.13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_handling: Option<FailureHandlingKind>,
    /// The client normalizes line endings when applying edits (LSP 3.16).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalizes_line_endings: Option<bool>,
    /// The client's support for `ChangeAnnotation`-grouped edits (LSP 3.16).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_annotation_support: Option<ChangeAnnotationSupportCapability>,
}

/// One kind of resource operation a `WorkspaceEdit` can contain (LSP 3.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceOperationKind {
    /// A file/folder creation.
    Create,
    /// A file/folder rename.
    Rename,
    /// A file/folder deletion.
    Delete,
}

/// How the client behaves if a resource operation in a `WorkspaceEdit` fails
/// partway through (LSP 3.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureHandlingKind {
    /// Applying stops; earlier changes are left as-is.
    Abort,
    /// All changes are applied transactionally: all-or-nothing.
    Transactional,
    /// The client applies changes then undoes them all if any operation
    /// fails.
    Undo,
    /// Text edits are applied transactionally; resource operations are
    /// best-effort.
    TextOnlyTransactional,
}

/// `ClientCapabilities.workspace.workspaceEdit.changeAnnotationSupport`
/// (LSP 3.16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeAnnotationSupportCapability {
    /// The client groups edits by `ChangeAnnotation::label` in its UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups_on_label: Option<bool>,
}

/// `ClientCapabilities.workspace.didChangeWatchedFiles`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidChangeWatchedFilesClientCapabilities {
    /// Whether the client supports dynamic registration for this method.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_registration: Option<bool>,
    /// The client supports [`super::registration::RelativePattern`] globs
    /// (LSP 3.17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_pattern_support: Option<bool>,
}

/// `ClientCapabilities.workspace.symbol`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSymbolClientCapabilities {
    /// Whether the client supports dynamic registration for this method.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_registration: Option<bool>,
    /// Which `SymbolKind`s the client understands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_kind: Option<SymbolKindCapability>,
    /// Which `SymbolTag`s the client understands (LSP 3.16).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_support: Option<TagSupportCapability<SymbolTag>>,
    /// Which `WorkspaceSymbol` properties the client can resolve lazily via
    /// `workspaceSymbol/resolve` (LSP 3.17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolve_support: Option<ResolveSupportCapability>,
}

/// `ClientCapabilities.workspace.symbol.symbolKind`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolKindCapability {
    /// The `SymbolKind`s the client understands. Absent means the client
    /// only understands the original LSP 1.0 range (`File` through
    /// `Array`); a server should degrade unknown kinds outside the set the
    /// client reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_set: Option<Vec<SymbolKind>>,
}

/// `ClientCapabilities.workspace.fileOperations` (LSP 3.16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationClientCapabilities {
    /// Whether the client supports dynamic registration for these methods.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_registration: Option<bool>,
    /// The client sends `workspace/didCreateFiles`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_create: Option<bool>,
    /// The client sends `workspace/willCreateFiles`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub will_create: Option<bool>,
    /// The client sends `workspace/didRenameFiles`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_rename: Option<bool>,
    /// The client sends `workspace/willRenameFiles`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub will_rename: Option<bool>,
    /// The client sends `workspace/didDeleteFiles`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_delete: Option<bool>,
    /// The client sends `workspace/willDeleteFiles`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub will_delete: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn workspace_client_capabilities_parses_a_realistic_payload() {
        let value = json!({
            "applyEdit": true,
            "workspaceEdit": {
                "documentChanges": true,
                "resourceOperations": ["create", "rename", "delete"],
                "failureHandling": "textOnlyTransactional",
                "changeAnnotationSupport": {"groupsOnLabel": true},
            },
            "didChangeWatchedFiles": {"dynamicRegistration": true, "relativePatternSupport": true},
            "symbol": {
                "symbolKind": {"valueSet": [1, 2, 3]},
                "tagSupport": {"valueSet": [1]},
                "resolveSupport": {"properties": ["location.range"]},
            },
            "workspaceFolders": true,
            "configuration": true,
            "semanticTokens": {"refreshSupport": true},
            "fileOperations": {"didCreate": true, "willRename": true},
        });
        let caps: WorkspaceClientCapabilities = serde_json::from_value(value).unwrap();

        assert_eq!(caps.apply_edit, Some(true));
        let edit = caps.workspace_edit.unwrap();
        assert_eq!(edit.document_changes, Some(true));
        assert_eq!(
            edit.resource_operations,
            vec![
                ResourceOperationKind::Create,
                ResourceOperationKind::Rename,
                ResourceOperationKind::Delete,
            ]
        );
        assert_eq!(
            edit.failure_handling,
            Some(FailureHandlingKind::TextOnlyTransactional)
        );
        assert_eq!(
            edit.change_annotation_support.unwrap().groups_on_label,
            Some(true)
        );
        let watched_files = caps.did_change_watched_files.unwrap();
        assert_eq!(watched_files.dynamic_registration, Some(true));
        assert_eq!(watched_files.relative_pattern_support, Some(true));
        let symbol = caps.symbol.unwrap();
        assert_eq!(
            symbol.symbol_kind.unwrap().value_set,
            Some(vec![
                SymbolKind::File,
                SymbolKind::Module,
                SymbolKind::Namespace
            ])
        );
        assert_eq!(
            symbol.tag_support.unwrap().value_set,
            vec![SymbolTag::Deprecated]
        );
        assert_eq!(
            symbol.resolve_support.unwrap().properties,
            vec!["location.range".to_owned()]
        );
        assert_eq!(caps.workspace_folders, Some(true));
        assert_eq!(caps.configuration, Some(true));
        assert_eq!(caps.semantic_tokens.unwrap().refresh_support, Some(true));
        let file_ops = caps.file_operations.unwrap();
        assert_eq!(file_ops.did_create, Some(true));
        assert_eq!(file_ops.will_rename, Some(true));
        assert_eq!(file_ops.did_rename, None);
    }

    #[test]
    fn workspace_client_capabilities_defaults_on_empty_object() {
        let caps: WorkspaceClientCapabilities = serde_json::from_value(json!({})).unwrap();
        assert_eq!(caps, WorkspaceClientCapabilities::default());
    }

    #[test]
    fn resource_operation_kind_uses_lowercase_strings() {
        assert_eq!(
            serde_json::to_value(ResourceOperationKind::Rename).unwrap(),
            json!("rename")
        );
    }

    #[test]
    fn failure_handling_kind_uses_camel_case_strings() {
        assert_eq!(
            serde_json::to_value(FailureHandlingKind::TextOnlyTransactional).unwrap(),
            json!("textOnlyTransactional")
        );
    }
}
