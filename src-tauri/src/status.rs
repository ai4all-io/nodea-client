use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error {
        message: String,
    },
    DeviceLimitReached {
        message: String,
    },
    /// The server refused this account: it was deleted, or an admin
    /// deactivated it. Terminal, like DeviceLimitReached - the same token
    /// would be refused again - and distinct from Error, which the UI reads
    /// as "can't reach the coordinator". `message` is the server's own
    /// reason ("Account deleted", "Account is inactive").
    Refused {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ModelStatus {
    NoModel,
    Downloading { model_name: String, progress: u8 },
    Loading { model_name: String },
    Ready { model_name: String },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub connection: ConnectionStatus,
    pub model: ModelStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_status_reaches_the_ui_with_its_type_and_reason() {
        // useNodePhase.ts switches on `type` and shows `message`.
        let json = serde_json::to_value(ConnectionStatus::Refused {
            message: "Account is inactive".to_string(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type": "Refused", "message": "Account is inactive"})
        );
    }
}
