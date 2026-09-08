use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientCommand {
    Ping,
    GetStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    Pong { protocol_version: u16 },
    Status { status: EngineStatus },
    Error { code: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineStatus {
    pub protocol_version: u16,
    pub proxy_state: ProxyState,
    pub listen_address: Option<String>,
    pub captured_transactions: u64,
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            proxy_state: ProxyState::Stopped,
            listen_address: None,
            captured_transactions: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

pub fn handle_command(command: ClientCommand, status: &EngineStatus) -> EngineEvent {
    match command {
        ClientCommand::Ping => EngineEvent::Pong {
            protocol_version: PROTOCOL_VERSION,
        },
        ClientCommand::GetStatus => EngineEvent::Status {
            status: status.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_round_trip_is_stable_json() {
        let json = serde_json::to_string(&ClientCommand::GetStatus).unwrap();
        assert_eq!(json, r#"{"type":"get_status"}"#);
        let decoded: ClientCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, ClientCommand::GetStatus);
    }

    #[test]
    fn status_event_uses_versioned_envelope() {
        let event = handle_command(ClientCommand::GetStatus, &EngineStatus::default());
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"status""#));
        assert!(json.contains(r#""protocol_version":1"#));
    }

    #[test]
    fn default_status_starts_stopped() {
        let status = EngineStatus::default();
        assert_eq!(status.protocol_version, PROTOCOL_VERSION);
        assert_eq!(status.proxy_state, ProxyState::Stopped);
        assert_eq!(status.captured_transactions, 0);
    }
}
