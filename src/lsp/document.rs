//! Text-document synchronisation notification parameters.

use super::base::{
    Range, TextDocumentIdentifier, TextDocumentItem, VersionedTextDocumentIdentifier,
};
use serde::{Deserialize, Serialize};

/// Parameters of `textDocument/didOpen`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidOpenTextDocumentParams {
    /// The document that was opened.
    pub text_document: TextDocumentItem,
}

/// Parameters of `textDocument/didChange`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidChangeTextDocumentParams {
    /// The document that changed, with its new version.
    pub text_document: VersionedTextDocumentIdentifier,
    /// The content changes, applied in order.
    pub content_changes: Vec<TextDocumentContentChangeEvent>,
}

/// A single change to a text document.
///
/// When [`range`](Self::range) is `Some`, only that range was replaced
/// (incremental sync). When it is `None`, [`text`](Self::text) is the entire
/// new document content (full sync).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentContentChangeEvent {
    /// The range that was replaced, or `None` for a full-document update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
    /// The new text for the range (or the whole document).
    pub text: String,
}

/// Parameters of `textDocument/didClose`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidCloseTextDocumentParams {
    /// The document that was closed.
    pub text_document: TextDocumentIdentifier,
}

/// Parameters of `textDocument/didSave`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidSaveTextDocumentParams {
    /// The document that was saved.
    pub text_document: TextDocumentIdentifier,
    /// The full document content, present only if the client is configured to
    /// include text on save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}
