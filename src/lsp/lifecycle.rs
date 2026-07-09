//! Lifecycle messages: `initialize` / `initialized` / `shutdown` and the
//! capability negotiation that rides along with them.

use super::base::Uri;
use super::enums::TextDocumentSyncKind;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Parameters of the `initialize` request.
///
/// Only the broadly useful fields are modelled as named fields; anything else
/// the client sends (`workspaceFolders`, `trace`, `locale`, …) is preserved
/// verbatim in [`extra`](Self::extra) rather than dropped, mirroring
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
    /// The root URI of the workspace, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_uri: Option<Uri>,
    /// Capabilities advertised by the client.
    #[serde(default)]
    pub capabilities: ClientCapabilities,
    /// Server-defined initialization options passed by the client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initialization_options: Option<Value>,
    /// Any fields not modelled above (e.g. `workspaceFolders`, `trace`,
    /// `locale`), preserved so backends can still read them.
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
            "workspaceFolders": [{"uri": "file:///a", "name": "a"}],
            "trace": "off",
        });
        let params: InitializeParams = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(
            params.extra.get("workspaceFolders"),
            Some(&value["workspaceFolders"])
        );
        assert_eq!(params.extra.get("trace"), Some(&json!("off")));

        let round_tripped = serde_json::to_value(&params).unwrap();
        assert_eq!(round_tripped["workspaceFolders"], value["workspaceFolders"]);
        assert_eq!(round_tripped["trace"], value["trace"]);
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
