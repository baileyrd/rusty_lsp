//! A small but complete language server built on `rusty_lsp`.
//!
//! It treats every open document as plain text and demonstrates the full
//! round trip an editor exercises:
//!
//! - **Lifecycle & capabilities** — advertises full-document sync, hover, and
//!   completion in `initialize`.
//! - **Document sync** — tracks open buffers in an in-memory store.
//! - **Diagnostics** (server → client) — flags `TODO` / `FIXME` / `XXX`
//!   markers, republished on every edit.
//! - **Hover** — reports the word under the cursor and how often it occurs.
//! - **Completion** — proposes the distinct words already present in the buffer.
//!
//! Run it as an editor would (it speaks LSP over stdio):
//!
//! ```text
//! cargo run --example text_server
//! ```
//!
//! Position handling converts the protocol's UTF-16 columns to UTF-8 byte
//! offsets, so multi-byte characters on a line do not skew ranges.

use rusty_lsp::error::Result;
use rusty_lsp::lsp::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, Hover, HoverParams, InitializeParams, InitializeResult, MessageType,
    Position, Range, ServerCapabilities, ServerInfo, TextDocumentSyncKind, Uri,
};
use rusty_lsp::text::{byte_to_utf16_column, utf16_column_to_byte};
use rusty_lsp::{Client, LanguageServer, Server};
use std::collections::{BTreeSet, HashMap};
use tokio::sync::RwLock;

/// Markers the server reports as warnings.
const MARKERS: [&str; 3] = ["TODO", "FIXME", "XXX"];
/// Minimum length for a word to be offered as a completion.
const MIN_COMPLETION_LEN: usize = 3;

/// An open document: its current text and the version it was last set to.
struct Document {
    text: String,
    version: i32,
}

/// The language server backend.
struct TextServer {
    client: Client,
    documents: RwLock<HashMap<Uri, Document>>,
}

impl TextServer {
    fn new(client: Client) -> Self {
        TextServer {
            client,
            documents: RwLock::new(HashMap::new()),
        }
    }

    /// Recompute and publish diagnostics for `uri` from the stored buffer.
    async fn publish_diagnostics(&self, uri: &Uri) {
        let (diagnostics, version) = {
            let documents = self.documents.read().await;
            match documents.get(uri) {
                Some(doc) => (compute_diagnostics(&doc.text), Some(doc.version)),
                // Document was closed; clear any diagnostics for it.
                None => (Vec::new(), None),
            }
        };
        let _ = self
            .client
            .publish_diagnostics(uri.clone(), diagnostics, version);
    }
}

impl LanguageServer for TextServer {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncKind::Full),
                hover_provider: Some(true),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Vec::new(),
                    resolve_provider: Some(false),
                }),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "rusty-text-server".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
        })
    }

    async fn initialized(&self) {
        let _ = self
            .client
            .log_message(MessageType::Info, "rusty-text-server ready");
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let item = params.text_document;
        let uri = item.uri.clone();
        self.documents.write().await.insert(
            uri.clone(),
            Document {
                text: item.text,
                version: item.version,
            },
        );
        self.publish_diagnostics(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        {
            let mut documents = self.documents.write().await;
            let Some(doc) = documents.get_mut(&uri) else {
                return;
            };
            // Full-sync mode: a change without a range carries the whole buffer.
            // (Incremental ranges are intentionally unsupported in this demo.)
            for change in params.content_changes {
                if change.range.is_none() {
                    doc.text = change.text;
                }
            }
            doc.version = params.text_document.version;
        }
        self.publish_diagnostics(&uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.write().await.remove(&uri);
        // Clear diagnostics for the now-closed document.
        let _ = self.client.publish_diagnostics(uri, Vec::new(), None);
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let position = params.text_document_position.position;
        let uri = params.text_document_position.text_document.uri;
        let documents = self.documents.read().await;
        let Some(doc) = documents.get(&uri) else {
            return Ok(None);
        };
        Ok(hover_at(&doc.text, position))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let documents = self.documents.read().await;
        let Some(doc) = documents.get(&uri) else {
            return Ok(None);
        };
        let items = completion_items(&doc.text);
        Ok(Some(CompletionResponse::Array(items)))
    }
}

/// Byte ranges of word tokens (`[A-Za-z0-9_]+`) within a single line.
fn word_tokens(line: &str) -> Vec<(usize, usize)> {
    let mut tokens = Vec::new();
    let mut start: Option<usize> = None;
    for (offset, ch) in line.char_indices() {
        let is_word = ch.is_alphanumeric() || ch == '_';
        match (is_word, start) {
            (true, None) => start = Some(offset),
            (false, Some(s)) => {
                tokens.push((s, offset));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        tokens.push((s, line.len()));
    }
    tokens
}

/// Count whole-word occurrences of `word` across the document.
fn count_word(text: &str, word: &str) -> usize {
    let mut count = 0;
    for line in text.lines() {
        for (s, e) in word_tokens(line) {
            if &line[s..e] == word {
                count += 1;
            }
        }
    }
    count
}

/// Build a hover for the word under `position`, if any.
fn hover_at(text: &str, position: Position) -> Option<Hover> {
    let line = text.lines().nth(position.line as usize)?;
    let byte = utf16_column_to_byte(line, position.character);
    let (start, end) = word_tokens(line)
        .into_iter()
        .find(|&(s, e)| byte >= s && byte <= e)?;
    let word = &line[start..end];
    if word.is_empty() {
        return None;
    }
    let occurrences = count_word(text, word);
    let range = Range::new(
        Position::new(position.line, byte_to_utf16_column(line, start)),
        Position::new(position.line, byte_to_utf16_column(line, end)),
    );
    let body = format!(
        "**{word}**\n\nWord appears {occurrences} time{} in this document.",
        if occurrences == 1 { "" } else { "s" },
    );
    Some(Hover::markdown(body).with_range(range))
}

/// Diagnostics for every marker (`TODO`/`FIXME`/`XXX`) in the document.
fn compute_diagnostics(text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        for (start, end) in word_tokens(line) {
            let word = &line[start..end];
            if MARKERS.contains(&word) {
                let range = Range::new(
                    Position::new(line_no as u32, byte_to_utf16_column(line, start)),
                    Position::new(line_no as u32, byte_to_utf16_column(line, end)),
                );
                diagnostics.push(
                    Diagnostic::new(
                        range,
                        DiagnosticSeverity::Warning,
                        format!("`{word}` marker found"),
                    )
                    .with_source("text-server"),
                );
            }
        }
    }
    diagnostics
}

/// Distinct, sorted words eligible to be offered as completions.
fn completion_items(text: &str) -> Vec<CompletionItem> {
    let mut words = BTreeSet::new();
    for line in text.lines() {
        for (start, end) in word_tokens(line) {
            let word = &line[start..end];
            let starts_with_alpha = word
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_');
            if word.len() >= MIN_COMPLETION_LEN && starts_with_alpha {
                words.insert(word.to_owned());
            }
        }
    }
    words
        .into_iter()
        .map(|word| CompletionItem::new(word).with_kind(CompletionItemKind::Text))
        .collect()
}

#[tokio::main]
async fn main() -> Result<()> {
    Server::stdio().serve(TextServer::new).await
}
