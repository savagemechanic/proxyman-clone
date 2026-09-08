pub mod body;
pub mod http;

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientCommand {
    Ping,
    GetStatus,
    ListTransactions { limit: Option<usize> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    Pong {
        protocol_version: u16,
    },
    Status {
        status: EngineStatus,
    },
    Transactions {
        transactions: Vec<CapturedTransaction>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineStatus {
    pub protocol_version: u16,
    pub proxy_state: ProxyState,
    pub listen_address: Option<String>,
    pub captured_transactions: u64,
    pub tls_interception_enabled: bool,
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            proxy_state: ProxyState::Stopped,
            listen_address: None,
            captured_transactions: 0,
            tls_interception_enabled: false,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionState {
    Pending,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderField {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyPreview {
    pub content_type: Option<String>,
    pub text: Option<String>,
    pub captured_bytes: u64,
    pub total_bytes: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedTransaction {
    pub id: u64,
    pub started_at_unix_ms: u64,
    pub scheme: String,
    pub host: String,
    pub method: String,
    pub target: String,
    pub request_headers: Vec<HeaderField>,
    pub request_body_bytes: u64,
    pub request_body_preview: Option<BodyPreview>,
    pub status_code: Option<u16>,
    pub response_headers: Vec<HeaderField>,
    pub response_body_bytes: u64,
    pub response_body_preview: Option<BodyPreview>,
    pub state: TransactionState,
}

pub fn handle_command(
    command: ClientCommand,
    status: &EngineStatus,
    transactions: &[CapturedTransaction],
) -> EngineEvent {
    match command {
        ClientCommand::Ping => EngineEvent::Pong {
            protocol_version: PROTOCOL_VERSION,
        },
        ClientCommand::GetStatus => EngineEvent::Status {
            status: status.clone(),
        },
        ClientCommand::ListTransactions { limit } => {
            let limit = limit.unwrap_or(250).min(1000);
            EngineEvent::Transactions {
                transactions: transactions.iter().take(limit).cloned().collect(),
            }
        }
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
        let event = handle_command(ClientCommand::GetStatus, &EngineStatus::default(), &[]);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"status""#));
        assert!(json.contains(r#""protocol_version":1"#));
        assert!(json.contains(r#""tls_interception_enabled":false"#));
    }

    #[test]
    fn list_transactions_applies_limit() {
        let transactions = (1..=3)
            .map(|id| CapturedTransaction {
                id,
                started_at_unix_ms: id,
                scheme: "https".into(),
                host: "example.com".into(),
                method: "GET".into(),
                target: "/".into(),
                request_headers: Vec::new(),
                request_body_bytes: 0,
                request_body_preview: None,
                status_code: Some(200),
                response_headers: Vec::new(),
                response_body_bytes: 0,
                response_body_preview: None,
                state: TransactionState::Complete,
            })
            .collect::<Vec<_>>();

        let event = handle_command(
            ClientCommand::ListTransactions { limit: Some(2) },
            &EngineStatus::default(),
            &transactions,
        );
        let EngineEvent::Transactions { transactions } = event else {
            panic!("expected transaction list");
        };
        assert_eq!(transactions.len(), 2);
        assert_eq!(transactions[0].id, 1);
    }

    #[test]
    fn default_status_starts_stopped() {
        let status = EngineStatus::default();
        assert_eq!(status.protocol_version, PROTOCOL_VERSION);
        assert_eq!(status.proxy_state, ProxyState::Stopped);
        assert_eq!(status.captured_transactions, 0);
        assert!(!status.tls_interception_enabled);
    }
}
