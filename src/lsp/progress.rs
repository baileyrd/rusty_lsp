//! `$/progress` and work-done-progress types (LSP 3.15+).
//!
//! A server reports progress on a long-running operation (indexing, a slow
//! computation) by reserving a token via `window/workDoneProgress/create`,
//! then sending a `begin` / any number of `report` / one `end` sequence of
//! `$/progress` notifications carrying that token. [`crate::Client`] exposes
//! typed helpers for both halves of this handshake
//! ([`create_progress`](crate::Client::create_progress),
//! [`progress_begin`](crate::Client::progress_begin),
//! [`progress_report`](crate::Client::progress_report),
//! [`progress_end`](crate::Client::progress_end)).

use serde::{Deserialize, Serialize};

/// Identifies one progress sequence, chosen by whichever side creates it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProgressToken {
    /// Numeric token.
    Number(i64),
    /// String token.
    String(String),
}

impl From<i64> for ProgressToken {
    fn from(n: i64) -> Self {
        ProgressToken::Number(n)
    }
}

impl From<String> for ProgressToken {
    fn from(s: String) -> Self {
        ProgressToken::String(s)
    }
}

impl From<&str> for ProgressToken {
    fn from(s: &str) -> Self {
        ProgressToken::String(s.to_owned())
    }
}

/// Parameters of the `$/progress` notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressParams {
    /// The token identifying the progress sequence this update belongs to.
    pub token: ProgressToken,
    /// The progress payload.
    pub value: WorkDoneProgress,
}

/// One update in a work-done-progress sequence: exactly one
/// [`WorkDoneProgress::Begin`], any number of [`WorkDoneProgress::Report`],
/// then exactly one [`WorkDoneProgress::End`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WorkDoneProgress {
    /// Starts a progress sequence.
    Begin(WorkDoneProgressBegin),
    /// Reports incremental progress within a sequence.
    Report(WorkDoneProgressReport),
    /// Ends a progress sequence.
    End(WorkDoneProgressEnd),
}

/// Starts a work-done-progress sequence.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkDoneProgressBegin {
    /// A short title for the operation.
    pub title: String,
    /// Whether the client may offer a cancel button for this operation. A
    /// server that supports cancellation should watch for
    /// `window/workDoneProgress/cancel` (see
    /// [`LanguageServer::work_done_progress_cancel`](crate::LanguageServer::work_done_progress_cancel)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellable: Option<bool>,
    /// A short, user-facing progress message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Progress percentage, `0..=100`. Omit if the total work isn't known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percentage: Option<u32>,
}

/// Reports incremental progress within a sequence.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkDoneProgressReport {
    /// Whether the client may offer a cancel button for this operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellable: Option<bool>,
    /// A short, user-facing progress message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Progress percentage, `0..=100`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percentage: Option<u32>,
}

/// Ends a work-done-progress sequence.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkDoneProgressEnd {
    /// A final, user-facing message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Parameters of the `window/workDoneProgress/create` request: asks the
/// client to reserve `token` for a subsequent progress sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkDoneProgressCreateParams {
    /// The token to reserve.
    pub token: ProgressToken,
}

/// Parameters of the `window/workDoneProgress/cancel` notification: the
/// client telling the server the user cancelled the operation behind
/// `token`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkDoneProgressCancelParams {
    /// The token identifying the operation to cancel.
    pub token: ProgressToken,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn begin_report_end_serialize_with_kind_tag() {
        let begin = WorkDoneProgress::Begin(WorkDoneProgressBegin {
            title: "Indexing".to_owned(),
            cancellable: Some(true),
            message: None,
            percentage: Some(0),
        });
        assert_eq!(
            serde_json::to_value(&begin).unwrap(),
            json!({"kind": "begin", "title": "Indexing", "cancellable": true, "percentage": 0})
        );

        let end = WorkDoneProgress::End(WorkDoneProgressEnd {
            message: Some("done".to_owned()),
        });
        assert_eq!(
            serde_json::to_value(&end).unwrap(),
            json!({"kind": "end", "message": "done"})
        );
    }

    #[test]
    fn progress_token_is_untagged_number_or_string() {
        assert_eq!(
            serde_json::to_value(ProgressToken::Number(1)).unwrap(),
            json!(1)
        );
        assert_eq!(
            serde_json::to_value(ProgressToken::String("a".into())).unwrap(),
            json!("a")
        );
        let token: ProgressToken = serde_json::from_value(json!(42)).unwrap();
        assert_eq!(token, ProgressToken::Number(42));
    }
}
