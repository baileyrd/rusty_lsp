//! [`Documents`]: a managed, URI-keyed store of open document text, applying
//! `textDocument/didOpen`/`didChange`/`didClose` edits for you so a backend
//! doesn't have to hand-roll a `HashMap<Uri, String>` and patch incremental
//! edits itself.
//!
//! `Documents` is entirely optional — the framework works the same with or
//! without it. Wire it up by calling [`Documents::did_open`]/
//! [`did_change`](Documents::did_change)/[`did_close`](Documents::did_close)
//! from your [`crate::LanguageServer`] implementation's matching methods.
//!
//! Incremental edits are patched using [`crate::text::position_to_offset`],
//! i.e. UTF-16-positioned (the base-spec default). A server that negotiated
//! a different [`crate::lsp::PositionEncodingKind`] should patch documents
//! itself using [`crate::text::position_to_offset_with`] instead of using
//! this type.

use crate::lsp::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, Uri,
};
use crate::text::position_to_offset;
use std::collections::HashMap;
use tokio::sync::RwLock;

/// A single open document's current text and metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// The document's language identifier (e.g. `"rust"`), as sent on open.
    pub language_id: String,
    /// The document's current version, updated on every change.
    pub version: i32,
    /// The document's current full text.
    pub text: String,
}

/// A managed, concurrency-safe store of open documents, keyed by URI.
///
/// Shares the same "shared via `Arc`, mutated through `&self`" pattern as
/// [`crate::LanguageServer`] backends: hold a `Documents` as a plain field
/// on your backend struct (not wrapped in an `Arc` of its own) and call its
/// methods through `&self`.
#[derive(Debug, Default)]
pub struct Documents {
    inner: RwLock<HashMap<Uri, Document>>,
}

impl Documents {
    /// Build an empty document store.
    pub fn new() -> Self {
        Documents::default()
    }

    /// Record a newly opened document (`textDocument/didOpen`).
    pub async fn did_open(&self, params: &DidOpenTextDocumentParams) {
        let item = &params.text_document;
        self.inner.write().await.insert(
            item.uri.clone(),
            Document {
                language_id: item.language_id.clone(),
                version: item.version,
                text: item.text.clone(),
            },
        );
    }

    /// Apply a document change (`textDocument/didChange`), patching in the
    /// full-document or incremental edits in order. A change for a document
    /// that isn't open is silently ignored (matching how the rest of the
    /// framework treats messages referencing unknown state).
    pub async fn did_change(&self, params: &DidChangeTextDocumentParams) {
        let mut documents = self.inner.write().await;
        let Some(document) = documents.get_mut(&params.text_document.uri) else {
            return;
        };
        for change in &params.content_changes {
            match change.range {
                Some(range) => {
                    let start = position_to_offset(&document.text, range.start);
                    let end = position_to_offset(&document.text, range.end);
                    document.text.replace_range(start..end, &change.text);
                }
                None => document.text.clone_from(&change.text),
            }
        }
        document.version = params.text_document.version;
    }

    /// Forget a closed document (`textDocument/didClose`).
    pub async fn did_close(&self, params: &DidCloseTextDocumentParams) {
        self.inner.write().await.remove(&params.text_document.uri);
    }

    /// Get a clone of a document's current state, if it's open.
    pub async fn get(&self, uri: &str) -> Option<Document> {
        self.inner.read().await.get(uri).cloned()
    }

    /// Get a clone of a document's current text, if it's open.
    pub async fn text(&self, uri: &str) -> Option<String> {
        self.inner.read().await.get(uri).map(|d| d.text.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::{
        Position, Range, TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
        VersionedTextDocumentIdentifier,
    };

    fn open(uri: &str, text: &str) -> DidOpenTextDocumentParams {
        DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.to_owned(),
                language_id: "plaintext".to_owned(),
                version: 1,
                text: text.to_owned(),
            },
        }
    }

    #[tokio::test]
    async fn open_then_get_round_trips() {
        let documents = Documents::new();
        documents.did_open(&open("file:///a", "hello")).await;

        let doc = documents.get("file:///a").await.expect("open");
        assert_eq!(doc.text, "hello");
        assert_eq!(doc.version, 1);
        assert_eq!(doc.language_id, "plaintext");
    }

    #[tokio::test]
    async fn full_sync_replaces_whole_text() {
        let documents = Documents::new();
        documents.did_open(&open("file:///a", "hello")).await;

        documents
            .did_change(&DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: "file:///a".to_owned(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    text: "goodbye".to_owned(),
                }],
            })
            .await;

        assert_eq!(
            documents.text("file:///a").await.as_deref(),
            Some("goodbye")
        );
    }

    #[tokio::test]
    async fn incremental_edits_apply_in_order_using_utf16_positions() {
        let documents = Documents::new();
        documents.did_open(&open("file:///a", "héllo world")).await;

        // "héllo world" -- replace "llo" (UTF-16 columns 2..5, since é is 1
        // unit) with "y", then replace "world" with "there" using the
        // position that only makes sense *after* the first edit landed.
        documents
            .did_change(&DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: "file:///a".to_owned(),
                    version: 2,
                },
                content_changes: vec![
                    TextDocumentContentChangeEvent {
                        range: Some(Range::new(Position::new(0, 2), Position::new(0, 5))),
                        text: "y".to_owned(),
                    },
                    TextDocumentContentChangeEvent {
                        range: Some(Range::new(Position::new(0, 4), Position::new(0, 9))),
                        text: "there".to_owned(),
                    },
                ],
            })
            .await;

        assert_eq!(
            documents.text("file:///a").await.as_deref(),
            Some("héy there")
        );
    }

    #[tokio::test]
    async fn close_removes_the_document() {
        let documents = Documents::new();
        documents.did_open(&open("file:///a", "hello")).await;
        documents
            .did_close(&DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier {
                    uri: "file:///a".to_owned(),
                },
            })
            .await;

        assert!(documents.get("file:///a").await.is_none());
    }

    #[tokio::test]
    async fn change_to_unopened_document_is_ignored() {
        let documents = Documents::new();
        documents
            .did_change(&DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: "file:///never-opened".to_owned(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    text: "x".to_owned(),
                }],
            })
            .await;

        assert!(documents.get("file:///never-opened").await.is_none());
    }
}
