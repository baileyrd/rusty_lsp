//! `textDocument/documentSymbol` and `workspace/symbol` types.

use super::base::{Location, Range, TextDocumentIdentifier};
use super::enums::SymbolKind;
use super::progress::{PartialResultParams, WorkDoneProgressParams};
use serde::{Deserialize, Serialize};

/// Parameters of `textDocument/documentSymbol`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSymbolParams {
    /// The document to list symbols for.
    pub text_document: TextDocumentIdentifier,
    #[serde(flatten)]
    pub work_done: WorkDoneProgressParams,
    #[serde(flatten)]
    pub partial_result: PartialResultParams,
}

/// The result of a `textDocument/documentSymbol` request: either the modern
/// hierarchical form ([`DocumentSymbol`], preferred — nests methods inside
/// classes, etc.) or the older flat form ([`SymbolInformation`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DocumentSymbolResponse {
    /// Hierarchical symbols, nested via [`DocumentSymbol::children`].
    Nested(Vec<DocumentSymbol>),
    /// A flat list of symbols, each with its own [`Location`].
    Flat(Vec<SymbolInformation>),
}

impl From<Vec<DocumentSymbol>> for DocumentSymbolResponse {
    fn from(symbols: Vec<DocumentSymbol>) -> Self {
        DocumentSymbolResponse::Nested(symbols)
    }
}

impl From<Vec<SymbolInformation>> for DocumentSymbolResponse {
    fn from(symbols: Vec<SymbolInformation>) -> Self {
        DocumentSymbolResponse::Flat(symbols)
    }
}

/// A symbol within a document, optionally nesting child symbols (e.g. a
/// class's methods and fields).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSymbol {
    /// The symbol's name.
    pub name: String,
    /// Additional detail, e.g. a function's signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The symbol's kind.
    pub kind: SymbolKind,
    /// Whether the symbol is deprecated. Prefer `tags` where the client
    /// supports it; kept for older clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,
    /// The symbol's full range (e.g. a function's entire body), used for
    /// operations like "expand selection".
    pub range: Range,
    /// The range that should be selected/highlighted when navigating to this
    /// symbol (typically just its name), a sub-range of [`range`](Self::range).
    pub selection_range: Range,
    /// Nested child symbols.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<DocumentSymbol>>,
}

impl DocumentSymbol {
    /// Build a leaf symbol (no children) from its name, kind, and ranges.
    pub fn new(
        name: impl Into<String>,
        kind: SymbolKind,
        range: Range,
        selection_range: Range,
    ) -> Self {
        DocumentSymbol {
            name: name.into(),
            detail: None,
            kind,
            deprecated: None,
            range,
            selection_range,
            children: None,
        }
    }
}

/// A symbol anchored to a specific document location — the flat,
/// non-hierarchical symbol representation used by both
/// `textDocument/documentSymbol` (older clients) and `workspace/symbol`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInformation {
    /// The symbol's name.
    pub name: String,
    /// The symbol's kind.
    pub kind: SymbolKind,
    /// Whether the symbol is deprecated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,
    /// The symbol's location.
    pub location: Location,
    /// The name of the symbol containing this one (e.g. a method's class),
    /// if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
}

impl SymbolInformation {
    /// Build a symbol from its name, kind, and location.
    pub fn new(name: impl Into<String>, kind: SymbolKind, location: Location) -> Self {
        SymbolInformation {
            name: name.into(),
            kind,
            deprecated: None,
            location,
            container_name: None,
        }
    }
}

/// Parameters of `workspace/symbol`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSymbolParams {
    /// The user's search query. An empty string requests all symbols the
    /// server is willing to return.
    pub query: String,
    #[serde(flatten)]
    pub work_done: WorkDoneProgressParams,
    #[serde(flatten)]
    pub partial_result: PartialResultParams,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn range() -> Range {
        Range::new(
            super::super::base::Position::new(0, 0),
            super::super::base::Position::new(0, 1),
        )
    }

    #[test]
    fn document_symbol_response_is_untagged() {
        let nested = DocumentSymbolResponse::Nested(vec![DocumentSymbol::new(
            "foo",
            SymbolKind::Function,
            range(),
            range(),
        )]);
        let value = serde_json::to_value(&nested).unwrap();
        assert_eq!(value[0]["name"], json!("foo"));
        assert_eq!(value[0]["kind"], json!(12));
        assert!(value[0].get("location").is_none());

        let flat = DocumentSymbolResponse::Flat(vec![SymbolInformation::new(
            "bar",
            SymbolKind::Variable,
            Location {
                uri: "file:///a".to_owned(),
                range: range(),
            },
        )]);
        let value = serde_json::to_value(&flat).unwrap();
        assert_eq!(value[0]["name"], json!("bar"));
        assert_eq!(value[0]["location"]["uri"], json!("file:///a"));
    }

    #[test]
    fn document_symbol_nests_children() {
        let class = DocumentSymbol {
            children: Some(vec![DocumentSymbol::new(
                "method",
                SymbolKind::Method,
                range(),
                range(),
            )]),
            ..DocumentSymbol::new("Class", SymbolKind::Class, range(), range())
        };
        let value = serde_json::to_value(&class).unwrap();
        assert_eq!(value["children"][0]["name"], json!("method"));
    }

    #[test]
    fn document_symbol_params_uses_camel_case() {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier {
                uri: "file:///a".to_owned(),
            },
            work_done: Default::default(),
            partial_result: Default::default(),
        };
        let value = serde_json::to_value(&params).unwrap();
        assert_eq!(value, json!({"textDocument": {"uri": "file:///a"}}));
    }
}
