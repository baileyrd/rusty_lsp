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
