//! Language-feature request parameters and results: hover, completion, and
//! goto-definition.

use super::base::{Location, Range, TextDocumentPositionParams};
use super::enums::{CompletionItemKind, CompletionTriggerKind, MarkupKind};
use serde::{Deserialize, Serialize};

/// Parameters of `textDocument/hover`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoverParams {
    /// The document and position the hover was requested at.
    #[serde(flatten)]
    pub text_document_position: TextDocumentPositionParams,
}

/// The result of a hover request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hover {
    /// The hover's rendered content.
    pub contents: MarkupContent,
    /// The range the hover applies to, used by clients to highlight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

impl Hover {
    /// Build a plain-text hover.
    pub fn plain_text(value: impl Into<String>) -> Self {
        Hover {
            contents: MarkupContent::plain_text(value),
            range: None,
        }
    }

    /// Build a Markdown hover.
    pub fn markdown(value: impl Into<String>) -> Self {
        Hover {
            contents: MarkupContent::markdown(value),
            range: None,
        }
    }

    /// Attach a highlight range to this hover.
    #[must_use]
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = Some(range);
        self
    }
}

/// A string rendered by the client, either plain text or Markdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkupContent {
    /// Whether `value` is plain text or Markdown.
    pub kind: MarkupKind,
    /// The content itself.
    pub value: String,
}

impl MarkupContent {
    /// Build plain-text markup.
    pub fn plain_text(value: impl Into<String>) -> Self {
        MarkupContent {
            kind: MarkupKind::PlainText,
            value: value.into(),
        }
    }

    /// Build Markdown markup.
    pub fn markdown(value: impl Into<String>) -> Self {
        MarkupContent {
            kind: MarkupKind::Markdown,
            value: value.into(),
        }
    }
}

/// Parameters of `textDocument/completion`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionParams {
    /// The document and position completion was requested at.
    #[serde(flatten)]
    pub text_document_position: TextDocumentPositionParams,
    /// Additional information about the completion trigger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<CompletionContext>,
}

/// Additional completion-trigger context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionContext {
    /// How completion was triggered.
    pub trigger_kind: CompletionTriggerKind,
    /// The trigger character, when `trigger_kind` is
    /// [`CompletionTriggerKind::TriggerCharacter`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_character: Option<String>,
}

/// The result of a completion request: either a bare list of items or a
/// [`CompletionList`] that can flag itself incomplete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompletionResponse {
    /// A simple array of items (treated as complete).
    Array(Vec<CompletionItem>),
    /// A list that may be marked incomplete to request re-querying.
    List(CompletionList),
}

impl From<Vec<CompletionItem>> for CompletionResponse {
    fn from(items: Vec<CompletionItem>) -> Self {
        CompletionResponse::Array(items)
    }
}

impl From<CompletionList> for CompletionResponse {
    fn from(list: CompletionList) -> Self {
        CompletionResponse::List(list)
    }
}

/// A list of completion items with an incompleteness flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionList {
    /// When `true`, the client should re-query as the user keeps typing.
    pub is_incomplete: bool,
    /// The completion items.
    pub items: Vec<CompletionItem>,
}

/// A single completion proposal.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    /// The label shown in the completion UI; also the default insert text.
    pub label: String,
    /// The item's semantic kind, used to choose an icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<CompletionItemKind>,
    /// A short detail string shown next to the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Documentation rendered when the item is highlighted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    /// Text inserted instead of `label`, when they differ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
}

impl CompletionItem {
    /// Build a completion item from just a label.
    pub fn new(label: impl Into<String>) -> Self {
        CompletionItem {
            label: label.into(),
            ..Default::default()
        }
    }

    /// Set the item's kind.
    #[must_use]
    pub fn with_kind(mut self, kind: CompletionItemKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Set the item's detail string.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// Parameters of `textDocument/definition`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionParams {
    /// The document and position definition was requested at.
    #[serde(flatten)]
    pub text_document_position: TextDocumentPositionParams,
}

/// The result of a goto-definition request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GotoDefinitionResponse {
    /// A single definition location.
    Scalar(Location),
    /// Several candidate locations.
    Array(Vec<Location>),
}

impl From<Location> for GotoDefinitionResponse {
    fn from(location: Location) -> Self {
        GotoDefinitionResponse::Scalar(location)
    }
}

impl From<Vec<Location>> for GotoDefinitionResponse {
    fn from(locations: Vec<Location>) -> Self {
        GotoDefinitionResponse::Array(locations)
    }
}
