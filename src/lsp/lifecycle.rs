//! Lifecycle messages: `initialize` / `initialized` / `shutdown` and the
//! capability negotiation that rides along with them.

use super::base::Uri;
use super::enums::{PositionEncodingKind, TextDocumentSyncKind};
use super::workspace::WorkspaceFolder;
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
                uri: "file:///a".to_owned(),
                name: "a".to_owned(),
            }])
        );
        // Not duplicated into `extra`.
        assert!(params.extra.get("workspaceFolders").is_none());
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
/// inspect [`ClientCapabilities::raw`] directly; the whole structure round-trips
/// losslessly.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientCapabilities {
    /// The full, untyped capability object as sent by the client.
    pub raw: Map<String, Value>,
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
    /// How the server wants document content synchronised.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_document_sync: Option<TextDocumentSyncKind>,
    /// Whether the server provides hover support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hover_provider: Option<bool>,
    /// Completion support and its options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_provider: Option<CompletionOptions>,
    /// Whether the server provides goto-definition support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_provider: Option<bool>,
    /// Any additional capabilities not modelled above.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
