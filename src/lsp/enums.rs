//! Integer- and string-coded LSP enumerations.
//!
//! Several LSP enums are transmitted as JSON integers (`1`, `2`, …) rather than
//! strings. serde derives string discriminants for fieldless enums, so these
//! types get hand-written codecs via the [`int_enum!`] macro, which keeps the
//! numeric mapping explicit and reversible.

use serde::{Deserialize, Serialize};

/// Generate a fieldless enum that (de)serializes as a JSON integer.
macro_rules! int_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident = $value:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u8)]
        $vis enum $name {
            $(
                $(#[$vmeta])*
                $variant = $value,
            )+
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_u8(*self as u8)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                let value = u8::deserialize(deserializer)?;
                match value {
                    $($value => Ok($name::$variant),)+
                    other => Err(serde::de::Error::custom(format!(
                        concat!("invalid ", stringify!($name), " discriminant: {}"),
                        other
                    ))),
                }
            }
        }
    };
}

int_enum! {
    /// How the client should synchronise document content to the server.
    pub enum TextDocumentSyncKind {
        /// Documents are not synced at all.
        None = 0,
        /// The full document text is sent on every change.
        Full = 1,
        /// Only incremental ranges are sent on each change.
        Incremental = 2,
    }
}

int_enum! {
    /// Severity used by `window/showMessage` and `window/logMessage`.
    pub enum MessageType {
        /// An error message.
        Error = 1,
        /// A warning message.
        Warning = 2,
        /// An informational message.
        Info = 3,
        /// A log message.
        Log = 4,
        /// A debug message (LSP 3.18).
        Debug = 5,
    }
}

int_enum! {
    /// Severity of a [`crate::lsp::Diagnostic`].
    pub enum DiagnosticSeverity {
        /// Reports an error.
        Error = 1,
        /// Reports a warning.
        Warning = 2,
        /// Reports an informational hint.
        Information = 3,
        /// Reports a low-priority hint.
        Hint = 4,
    }
}

int_enum! {
    /// What triggered a completion request.
    pub enum CompletionTriggerKind {
        /// Completion was invoked manually (e.g. Ctrl+Space) or via the API.
        Invoked = 1,
        /// Completion was triggered by a trigger character.
        TriggerCharacter = 2,
        /// Completion was re-triggered because the current list is incomplete.
        TriggerForIncompleteCompletions = 3,
    }
}

int_enum! {
    /// The semantic kind of a [`crate::lsp::CompletionItem`], used by clients to
    /// pick an icon. Values match the LSP `CompletionItemKind` table.
    pub enum CompletionItemKind {
        /// A free-text completion.
        Text = 1,
        /// A method.
        Method = 2,
        /// A function.
        Function = 3,
        /// A constructor.
        Constructor = 4,
        /// A field.
        Field = 5,
        /// A variable.
        Variable = 6,
        /// A class.
        Class = 7,
        /// An interface.
        Interface = 8,
        /// A module.
        Module = 9,
        /// A property.
        Property = 10,
        /// A unit.
        Unit = 11,
        /// A literal value.
        Value = 12,
        /// An enum.
        Enum = 13,
        /// A keyword.
        Keyword = 14,
        /// A code snippet.
        Snippet = 15,
        /// A color.
        Color = 16,
        /// A file.
        File = 17,
        /// A reference.
        Reference = 18,
        /// A folder.
        Folder = 19,
        /// An enum member.
        EnumMember = 20,
        /// A constant.
        Constant = 21,
        /// A struct.
        Struct = 22,
        /// An event.
        Event = 23,
        /// An operator.
        Operator = 24,
        /// A type parameter.
        TypeParameter = 25,
    }
}

/// The format of a [`crate::lsp::MarkupContent`] value. Unlike the enums above
/// this is encoded as a JSON string, so it uses a plain serde derive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkupKind {
    /// Plain text.
    PlainText,
    /// Markdown.
    Markdown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn integer_enums_encode_as_numbers() {
        assert_eq!(
            serde_json::to_value(TextDocumentSyncKind::Full).unwrap(),
            json!(1)
        );
        assert_eq!(
            serde_json::to_value(MessageType::Warning).unwrap(),
            json!(2)
        );
        assert_eq!(
            serde_json::to_value(DiagnosticSeverity::Hint).unwrap(),
            json!(4)
        );
        assert_eq!(
            serde_json::to_value(CompletionItemKind::TypeParameter).unwrap(),
            json!(25)
        );
    }

    #[test]
    fn integer_enums_decode_from_numbers() {
        let kind: DiagnosticSeverity = serde_json::from_value(json!(1)).unwrap();
        assert_eq!(kind, DiagnosticSeverity::Error);
        let trigger: CompletionTriggerKind = serde_json::from_value(json!(2)).unwrap();
        assert_eq!(trigger, CompletionTriggerKind::TriggerCharacter);
    }

    #[test]
    fn invalid_discriminant_is_rejected() {
        assert!(serde_json::from_value::<DiagnosticSeverity>(json!(99)).is_err());
    }

    #[test]
    fn markup_kind_encodes_as_lowercase_string() {
        assert_eq!(
            serde_json::to_value(MarkupKind::PlainText).unwrap(),
            json!("plaintext")
        );
        assert_eq!(
            serde_json::to_value(MarkupKind::Markdown).unwrap(),
            json!("markdown")
        );
    }
}
