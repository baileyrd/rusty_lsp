//! Foundational LSP value types shared across requests.

use serde::{Deserialize, Serialize};

/// A document URI, e.g. `file:///home/user/main.rs`.
///
/// LSP transmits URIs as opaque strings; this alias documents intent at call
/// sites without imposing a parsing dependency.
pub type Uri = String;

/// A zero-based position inside a text document.
///
/// `character` is, per the base LSP spec, an offset in **UTF-16 code units**
/// from the start of the line — not bytes and not Unicode scalar values.
/// Servers must convert accordingly when indexing UTF-8 buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position {
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based UTF-16 code-unit offset within the line.
    pub character: u32,
}

impl Position {
    /// Construct a position from a line and character offset.
    pub fn new(line: u32, character: u32) -> Self {
        Position { line, character }
    }
}

/// A contiguous range `[start, end)` within a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Range {
    /// The range's start position (inclusive).
    pub start: Position,
    /// The range's end position (exclusive).
    pub end: Position,
}

impl Range {
    /// Construct a range from explicit start/end positions.
    pub fn new(start: Position, end: Position) -> Self {
        Range { start, end }
    }
}

/// A location: a [`Range`] within a specific document.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    /// The document URI.
    pub uri: Uri,
    /// The range within that document.
    pub range: Range,
}

/// Identifies a text document by URI.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextDocumentIdentifier {
    /// The document URI.
    pub uri: Uri,
}

/// Identifies a text document together with the version it is expected to be at.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VersionedTextDocumentIdentifier {
    /// The document URI.
    pub uri: Uri,
    /// The version number after the associated change was applied.
    pub version: i32,
}

/// An item representing a document the client just opened.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentItem {
    /// The document URI.
    pub uri: Uri,
    /// The document's language identifier (e.g. `"rust"`).
    pub language_id: String,
    /// The version number of this document.
    pub version: i32,
    /// The full content of the opened document.
    pub text: String,
}

/// A document position parameter pair, flattened into feature request params.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentPositionParams {
    /// The document the request applies to.
    pub text_document: TextDocumentIdentifier,
    /// The position inside that document.
    pub position: Position,
}
