//! Diagnostic types and the `textDocument/publishDiagnostics` notification.

use super::base::{Range, Uri};
use super::enums::DiagnosticSeverity;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A diagnostic — an error, warning, or hint anchored to a document range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// The range the diagnostic applies to.
    pub range: Range,
    /// The diagnostic's severity. Clients may render differently when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<DiagnosticSeverity>,
    /// A machine-readable code (number or string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<Value>,
    /// A human-readable origin, e.g. the linter name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The diagnostic message.
    pub message: String,
}

impl Diagnostic {
    /// Build a diagnostic with a range, severity, and message.
    pub fn new(range: Range, severity: DiagnosticSeverity, message: impl Into<String>) -> Self {
        Diagnostic {
            range,
            severity: Some(severity),
            code: None,
            source: None,
            message: message.into(),
        }
    }

    /// Set the diagnostic's source label.
    #[must_use]
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

/// Parameters of `textDocument/publishDiagnostics`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishDiagnosticsParams {
    /// The document the diagnostics belong to.
    pub uri: Uri,
    /// The document version the diagnostics were computed against, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i32>,
    /// The full set of diagnostics for the document (replaces any previous set).
    pub diagnostics: Vec<Diagnostic>,
}
