//! `client/registerCapability` and `client/unregisterCapability` types —
//! lets a server register interest in a method after `initialize` (e.g.
//! scoped to a `documentSelector`) instead of only declaring capabilities
//! statically in [`crate::lsp::ServerCapabilities`].

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One capability registration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    /// An id for this registration, unique among this server's active
    /// registrations, used to unregister it later.
    pub id: String,
    /// The method being registered for, e.g. `"textDocument/formatting"`.
    pub method: String,
    /// Method-specific registration options (e.g. a `documentSelector`),
    /// if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub register_options: Option<Value>,
}

impl Registration {
    /// Build a registration from its id, method, and options.
    pub fn new(
        id: impl Into<String>,
        method: impl Into<String>,
        register_options: Option<Value>,
    ) -> Self {
        Registration {
            id: id.into(),
            method: method.into(),
            register_options,
        }
    }
}

/// Parameters of `client/registerCapability`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistrationParams {
    /// The registrations to add.
    pub registrations: Vec<Registration>,
}

/// One capability unregistration, referencing an earlier
/// [`Registration::id`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unregistration {
    /// The [`Registration::id`] to undo.
    pub id: String,
    /// The method that was registered for.
    pub method: String,
}

/// Parameters of `client/unregisterCapability`.
///
/// The `unregisterations` field name (missing an "r") is not a typo in this
/// crate — it matches the LSP specification's field name verbatim, which is
/// itself a long-standing, wire-compatibility-frozen misspelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnregistrationParams {
    /// The unregistrations to apply.
    pub unregisterations: Vec<Unregistration>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registration_params_round_trips() {
        let params = RegistrationParams {
            registrations: vec![Registration::new(
                "1",
                "textDocument/formatting",
                Some(json!({"documentSelector": [{"language": "rust"}]})),
            )],
        };
        let value = serde_json::to_value(&params).unwrap();
        assert_eq!(
            value["registrations"][0]["method"],
            json!("textDocument/formatting")
        );
        assert_eq!(
            value["registrations"][0]["registerOptions"]["documentSelector"][0]["language"],
            json!("rust")
        );
    }

    #[test]
    fn unregistration_params_uses_spec_field_name() {
        let params = UnregistrationParams {
            unregisterations: vec![Unregistration {
                id: "1".to_owned(),
                method: "textDocument/formatting".to_owned(),
            }],
        };
        let value = serde_json::to_value(&params).unwrap();
        assert!(value.get("unregisterations").is_some());
        assert!(value.get("unregistrations").is_none());
    }
}

/// A filter selecting documents by language, URI scheme, and/or glob
/// pattern. At least one field should be set.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DocumentFilter {
    /// Match documents with this language id (e.g. `"rust"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Match documents with this URI scheme (e.g. `"file"`, `"untitled"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    /// Match documents whose path matches this glob (e.g. `"**/*.toml"`;
    /// `*`, `**`, `?`, `{a,b}`, `[...]` per the spec).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

impl DocumentFilter {
    /// Filter by language id alone.
    pub fn language(language: impl Into<String>) -> Self {
        DocumentFilter {
            language: Some(language.into()),
            ..Default::default()
        }
    }

    /// Filter by glob pattern alone.
    pub fn pattern(pattern: impl Into<String>) -> Self {
        DocumentFilter {
            pattern: Some(pattern.into()),
            ..Default::default()
        }
    }
}

/// The set of documents a dynamic registration applies to: a document
/// matches if any filter matches.
pub type DocumentSelector = Vec<DocumentFilter>;

/// The registration options common to all `textDocument/*` methods: which
/// documents the registration applies to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentRegistrationOptions {
    /// The documents the registration applies to. `None` falls back to the
    /// selector the client provides on the server's behalf.
    pub document_selector: Option<DocumentSelector>,
}

impl Registration {
    /// Build a registration scoped to the documents matching `selector`,
    /// the common case for dynamic `textDocument/*` registrations:
    ///
    /// ```rust
    /// use rusty_lsp::lsp::{DocumentFilter, Registration};
    ///
    /// let registration = Registration::for_documents(
    ///     "fmt-toml",
    ///     "textDocument/formatting",
    ///     vec![DocumentFilter::language("toml")],
    /// );
    /// ```
    pub fn for_documents(
        id: impl Into<String>,
        method: impl Into<String>,
        selector: DocumentSelector,
    ) -> Self {
        let options = TextDocumentRegistrationOptions {
            document_selector: Some(selector),
        };
        Registration {
            id: id.into(),
            method: method.into(),
            register_options: Some(
                serde_json::to_value(options).expect("registration options serialize"),
            ),
        }
    }
}

#[cfg(test)]
mod registration_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn for_documents_builds_a_document_selector() {
        let registration = Registration::for_documents(
            "fmt-1",
            "textDocument/formatting",
            vec![
                DocumentFilter::language("toml"),
                DocumentFilter::pattern("**/*.lock"),
            ],
        );
        assert_eq!(
            serde_json::to_value(&registration).unwrap(),
            json!({
                "id": "fmt-1",
                "method": "textDocument/formatting",
                "registerOptions": {
                    "documentSelector": [
                        {"language": "toml"},
                        {"pattern": "**/*.lock"},
                    ],
                },
            })
        );
    }
}
