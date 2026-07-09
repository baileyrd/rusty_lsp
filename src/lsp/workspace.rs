//! `workspace/*` types: multi-root workspace folders, pulling client
//! configuration, applying edits back to the client's buffers, watched-file
//! change notifications, and command execution.

use super::base::{Range, Uri};
use super::enums::FileChangeType;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// A single root folder in a (possibly multi-root) workspace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceFolder {
    /// The folder's URI.
    pub uri: Uri,
    /// The folder's display name.
    pub name: String,
}

/// Parameters of the `workspace/didChangeWorkspaceFolders` notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DidChangeWorkspaceFoldersParams {
    /// The added/removed folders.
    pub event: WorkspaceFoldersChangeEvent,
}

/// The folders added and removed since the last known state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WorkspaceFoldersChangeEvent {
    /// Folders added to the workspace.
    pub added: Vec<WorkspaceFolder>,
    /// Folders removed from the workspace.
    pub removed: Vec<WorkspaceFolder>,
}

/// Parameters of a server-to-client `workspace/configuration` request: asks
/// for the value of one or more configuration sections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationParams {
    /// The sections being requested.
    pub items: Vec<ConfigurationItem>,
}

/// One configuration section to look up, optionally scoped to a resource.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationItem {
    /// Restrict the lookup to this resource, if given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_uri: Option<Uri>,
    /// The dotted configuration section to read (e.g. `"editor.tabSize"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

/// A single textual edit: replace [`range`](Self::range) with
/// [`new_text`](Self::new_text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    /// The range to replace.
    pub range: Range,
    /// The replacement text (empty string to delete the range).
    pub new_text: String,
}

impl TextEdit {
    /// Build a text edit from a range and its replacement text.
    pub fn new(range: Range, new_text: impl Into<String>) -> Self {
        TextEdit {
            range,
            new_text: new_text.into(),
        }
    }
}

/// A set of edits across one or more documents, sent to
/// `workspace/applyEdit`.
///
/// Only the `changes` form (a flat per-document edit list) is modelled as a
/// named field; the richer `documentChanges` form (versioned edits, file
/// creates/renames/deletes) is preserved verbatim in
/// [`extra`](Self::extra) rather than dropped, mirroring
/// [`crate::lsp::ServerCapabilities::extra`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WorkspaceEdit {
    /// Per-document lists of edits to apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes: Option<HashMap<Uri, Vec<TextEdit>>>,
    /// Any additional fields not modelled above (e.g. `documentChanges`,
    /// `changeAnnotations`).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl WorkspaceEdit {
    /// Build a `changes`-form edit from a single document's edit list.
    pub fn for_document(uri: Uri, edits: Vec<TextEdit>) -> Self {
        let mut changes = HashMap::new();
        changes.insert(uri, edits);
        WorkspaceEdit {
            changes: Some(changes),
            extra: Map::new(),
        }
    }
}

/// Parameters of the server-to-client `workspace/applyEdit` request.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ApplyWorkspaceEditParams {
    /// An optional label describing the edit, for undo history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The edit to apply.
    pub edit: WorkspaceEdit,
}

/// Result of a `workspace/applyEdit` request.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyWorkspaceEditResult {
    /// Whether the edit was applied. May be `false` if the client rejected
    /// it or a document's content did not match expectations.
    pub applied: bool,
    /// A human-readable explanation when `applied` is `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    /// The index of the change that failed to apply, if identifiable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_change: Option<u32>,
}

/// Parameters of the `workspace/didChangeConfiguration` notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DidChangeConfigurationParams {
    /// The new configuration settings. The shape is entirely server-defined,
    /// so it round-trips as raw JSON.
    pub settings: Value,
}

/// Parameters of the `workspace/didChangeWatchedFiles` notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DidChangeWatchedFilesParams {
    /// The files that changed.
    pub changes: Vec<FileEvent>,
}

/// One watched file's change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEvent {
    /// The file's URI.
    pub uri: Uri,
    /// How the file changed.
    #[serde(rename = "type")]
    pub typ: FileChangeType,
}

/// Parameters of the `workspace/executeCommand` request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecuteCommandParams {
    /// The command identifier, one of the strings the server advertised in
    /// [`ExecuteCommandOptions::commands`].
    pub command: String,
    /// Arguments for the command, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<Value>>,
}

/// Options describing the server's `workspace/executeCommand` support,
/// advertised in [`crate::lsp::ServerCapabilities::execute_command_provider`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteCommandOptions {
    /// The command identifiers the server accepts.
    pub commands: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::super::base::Position;
    use super::*;
    use serde_json::json;

    #[test]
    fn workspace_edit_preserves_document_changes_in_extra() {
        let value = json!({
            "documentChanges": [{"textDocument": {"uri": "file:///a", "version": 1}, "edits": []}],
        });
        let edit: WorkspaceEdit = serde_json::from_value(value.clone()).unwrap();
        assert!(edit.changes.is_none());
        assert_eq!(
            edit.extra.get("documentChanges"),
            value.get("documentChanges")
        );

        let round_tripped = serde_json::to_value(&edit).unwrap();
        assert_eq!(round_tripped["documentChanges"], value["documentChanges"]);
    }

    #[test]
    fn workspace_edit_for_document_round_trips() {
        let edit = WorkspaceEdit::for_document(
            "file:///a".to_owned(),
            vec![TextEdit::new(
                Range::new(Position::new(0, 0), Position::new(0, 1)),
                "x",
            )],
        );
        let value = serde_json::to_value(&edit).unwrap();
        assert_eq!(value["changes"]["file:///a"][0]["newText"], json!("x"));
    }

    #[test]
    fn file_event_type_uses_type_keyword() {
        let event = FileEvent {
            uri: "file:///a".to_owned(),
            typ: FileChangeType::Changed,
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value, json!({"uri": "file:///a", "type": 2}));
    }
}
