//! `window/*` notification parameters for surfacing messages to the user.

use super::enums::MessageType;
use serde::{Deserialize, Serialize};

/// Parameters of `window/showMessage`: a message shown prominently to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShowMessageParams {
    /// The message's severity.
    #[serde(rename = "type")]
    pub typ: MessageType,
    /// The message text.
    pub message: String,
}

/// Parameters of `window/logMessage`: a message routed to the client's log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogMessageParams {
    /// The message's severity.
    #[serde(rename = "type")]
    pub typ: MessageType,
    /// The message text.
    pub message: String,
}
