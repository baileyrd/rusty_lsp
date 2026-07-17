//! `client/registerCapability` and `client/unregisterCapability` types —
//! lets a server register interest in a method after `initialize` (e.g.
//! scoped to a `documentSelector`) instead of only declaring capabilities
//! statically in [`crate::lsp::ServerCapabilities`].

use super::base::Uri;
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

/// Bit flags for [`FileSystemWatcher::kind`]: which file events the server
/// wants reported. Combine with `|`; the spec default (all three) applies
/// when the field is omitted.
pub mod watch_kind {
    /// Report file creations.
    pub const CREATE: u32 = 1;
    /// Report file content changes.
    pub const CHANGE: u32 = 2;
    /// Report file deletions.
    pub const DELETE: u32 = 4;
    /// All of create, change, and delete (the spec default).
    pub const ALL: u32 = CREATE | CHANGE | DELETE;
}

/// The pattern a [`FileSystemWatcher`] watches: a bare glob (matched
/// against absolute paths) or a glob relative to a base URI (LSP 3.17).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GlobPattern {
    /// A glob such as `"**/*.toml"` (`*`, `**`, `?`, `{a,b}`, `[...]`).
    Pattern(String),
    /// A glob interpreted relative to a base folder.
    Relative(RelativePattern),
}

impl From<&str> for GlobPattern {
    fn from(pattern: &str) -> Self {
        GlobPattern::Pattern(pattern.to_owned())
    }
}

impl From<String> for GlobPattern {
    fn from(pattern: String) -> Self {
        GlobPattern::Pattern(pattern)
    }
}

/// A glob interpreted relative to a base folder (LSP 3.17).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelativePattern {
    /// The folder the pattern is relative to.
    pub base_uri: Uri,
    /// The glob itself.
    pub pattern: String,
}

/// One watch subscription inside
/// [`DidChangeWatchedFilesRegistrationOptions`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSystemWatcher {
    /// What to watch.
    pub glob_pattern: GlobPattern,
    /// Which events to report, as [`watch_kind`] flags; omitted means all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<u32>,
}

impl FileSystemWatcher {
    /// Watch `pattern` for all event kinds.
    pub fn new(pattern: impl Into<GlobPattern>) -> Self {
        FileSystemWatcher {
            glob_pattern: pattern.into(),
            kind: None,
        }
    }

    /// Restrict which events are reported (see [`watch_kind`]).
    #[must_use]
    pub fn with_kind(mut self, kind: u32) -> Self {
        self.kind = Some(kind);
        self
    }
}

/// Registration options of `workspace/didChangeWatchedFiles`: the file
/// patterns the client should watch on the server's behalf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DidChangeWatchedFilesRegistrationOptions {
    /// The watch subscriptions.
    pub watchers: Vec<FileSystemWatcher>,
}

impl Registration {
    /// Build a `workspace/didChangeWatchedFiles` registration asking the
    /// client to watch the given patterns — the standard `initialized`-time
    /// call for servers that track files beyond the open set:
    ///
    /// ```rust
    /// use rusty_lsp::lsp::{FileSystemWatcher, Registration, watch_kind};
    ///
    /// let registration = Registration::for_watched_files(
    ///     "watch-manifests",
    ///     vec![
    ///         FileSystemWatcher::new("**/Cargo.toml"),
    ///         FileSystemWatcher::new("**/*.lock").with_kind(watch_kind::CHANGE),
    ///     ],
    /// );
    /// ```
    pub fn for_watched_files(id: impl Into<String>, watchers: Vec<FileSystemWatcher>) -> Self {
        let options = DidChangeWatchedFilesRegistrationOptions { watchers };
        Registration {
            id: id.into(),
            method: "workspace/didChangeWatchedFiles".to_owned(),
            register_options: Some(
                serde_json::to_value(options).expect("watcher options serialize"),
            ),
        }
    }
}

#[cfg(test)]
mod watcher_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn for_watched_files_builds_the_registration() {
        let registration = Registration::for_watched_files(
            "w1",
            vec![
                FileSystemWatcher::new("**/*.toml"),
                FileSystemWatcher::new(GlobPattern::Relative(RelativePattern {
                    base_uri: "file:///ws".into(),
                    pattern: "src/**".to_owned(),
                }))
                .with_kind(watch_kind::CREATE | watch_kind::DELETE),
            ],
        );
        assert_eq!(
            serde_json::to_value(&registration).unwrap(),
            json!({
                "id": "w1",
                "method": "workspace/didChangeWatchedFiles",
                "registerOptions": {
                    "watchers": [
                        {"globPattern": "**/*.toml"},
                        {
                            "globPattern": {"baseUri": "file:///ws", "pattern": "src/**"},
                            "kind": 5,
                        },
                    ],
                },
            })
        );
    }
}
