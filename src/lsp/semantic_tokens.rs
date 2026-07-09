//! `textDocument/semanticTokens/{full,full/delta,range}` types.
//!
//! Semantic tokens let a server highlight code more accurately than a
//! TextMate grammar (which only sees syntax) by classifying each token with
//! real semantic information (e.g. "this identifier is a mutable local
//! variable"). The wire format encodes each token as 5 integers relative to
//! the previous one — see [`SemanticTokens::data`] — which this module does
//! not interpret; servers typically build it with a small local encoder
//! keyed by their own token/modifier tables.

use super::base::{Range, TextDocumentIdentifier};
use serde::{Deserialize, Serialize};

/// Declares the token types and modifiers a server's semantic-token indices
/// refer to. Sent once, in
/// [`crate::lsp::ServerCapabilities::semantic_tokens_provider`]; every
/// token's `tokenType`/`tokenModifiers` index into these arrays.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTokensLegend {
    /// Token type names, indexed by a token's type field.
    pub token_types: Vec<String>,
    /// Token modifier names, indexed by the set bits of a token's modifiers
    /// bitmask.
    pub token_modifiers: Vec<String>,
}

/// Parameters of `textDocument/semanticTokens/full`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTokensParams {
    /// The document to compute tokens for.
    pub text_document: TextDocumentIdentifier,
}

/// Parameters of `textDocument/semanticTokens/full/delta`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTokensDeltaParams {
    /// The document to compute a token delta for.
    pub text_document: TextDocumentIdentifier,
    /// The [`SemanticTokens::result_id`] of the previous full result this
    /// delta is relative to.
    pub previous_result_id: String,
}

/// Parameters of `textDocument/semanticTokens/range`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTokensRangeParams {
    /// The document to compute tokens for.
    pub text_document: TextDocumentIdentifier,
    /// The range to compute tokens within.
    pub range: Range,
}

/// The full set of semantic tokens for a document (or range).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTokens {
    /// An opaque id for this result, passed back in a later
    /// [`SemanticTokensDeltaParams::previous_result_id`] to request a delta
    /// instead of a full re-scan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_id: Option<String>,
    /// The encoded tokens: a flat array of relative 5-tuples
    /// `[deltaLine, deltaStart, length, tokenType, tokenModifiers]` per the
    /// spec's delta encoding.
    pub data: Vec<u32>,
}

/// The result of `textDocument/semanticTokens/full/delta`: either a full
/// re-scan (if the server decides a delta isn't worth computing) or an
/// incremental edit list against the previous result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SemanticTokensDeltaResult {
    /// A full token set, same shape as `semanticTokens/full`.
    Tokens(SemanticTokens),
    /// An incremental delta against the previous result.
    Delta(SemanticTokensDelta),
}

/// An incremental edit against a previous [`SemanticTokens::data`] array.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTokensDelta {
    /// The new result's id, for further deltas to build on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_id: Option<String>,
    /// The edits to apply, in order.
    pub edits: Vec<SemanticTokensEdit>,
}

/// One splice into a previous [`SemanticTokens::data`] array: remove
/// `delete_count` `u32`s starting at `start`, then insert `data` (if any) in
/// their place.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTokensEdit {
    /// The index into the previous data array to start editing at.
    pub start: u32,
    /// How many `u32`s to remove starting at `start`.
    pub delete_count: u32,
    /// The `u32`s to insert in their place, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<u32>>,
}

/// Options describing the server's `full` semantic-tokens support: either a
/// bare `true`, or an object opting into `full/delta` as well.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SemanticTokensFullOptions {
    /// `true`/`false`: full-document tokens are (not) supported, with no
    /// delta support.
    Simple(bool),
    /// Full-document tokens are supported, with delta support as given.
    Delta {
        /// Whether `semanticTokens/full/delta` is supported.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delta: Option<bool>,
    },
}

/// Options describing the server's `textDocument/semanticTokens` support.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTokensOptions {
    /// The token/modifier legend every token's indices refer to.
    pub legend: SemanticTokensLegend,
    /// Whether `textDocument/semanticTokens/range` is supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<bool>,
    /// Whether `textDocument/semanticTokens/full` (and optionally
    /// `full/delta`) is supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<SemanticTokensFullOptions>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn semantic_tokens_delta_result_is_untagged() {
        let tokens = SemanticTokensDeltaResult::Tokens(SemanticTokens {
            result_id: Some("1".to_owned()),
            data: vec![0, 0, 3, 0, 0],
        });
        let value = serde_json::to_value(&tokens).unwrap();
        assert_eq!(value["data"], json!([0, 0, 3, 0, 0]));

        let delta = SemanticTokensDeltaResult::Delta(SemanticTokensDelta {
            result_id: Some("2".to_owned()),
            edits: vec![SemanticTokensEdit {
                start: 0,
                delete_count: 5,
                data: Some(vec![0, 0, 4, 1, 0]),
            }],
        });
        let value = serde_json::to_value(&delta).unwrap();
        assert_eq!(value["edits"][0]["deleteCount"], json!(5));
        assert!(value.get("data").is_none());
    }

    #[test]
    fn semantic_tokens_legend_uses_camel_case() {
        let legend = SemanticTokensLegend {
            token_types: vec!["variable".to_owned()],
            token_modifiers: vec!["readonly".to_owned()],
        };
        let value = serde_json::to_value(&legend).unwrap();
        assert_eq!(
            value,
            json!({"tokenTypes": ["variable"], "tokenModifiers": ["readonly"]})
        );
    }

    #[test]
    fn semantic_tokens_full_options_is_untagged() {
        assert_eq!(
            serde_json::to_value(SemanticTokensFullOptions::Simple(true)).unwrap(),
            json!(true)
        );
        assert_eq!(
            serde_json::to_value(SemanticTokensFullOptions::Delta { delta: Some(true) }).unwrap(),
            json!({"delta": true})
        );
    }
}
