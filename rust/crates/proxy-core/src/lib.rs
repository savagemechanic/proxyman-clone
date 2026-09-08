pub mod body;
pub mod http;
pub mod rules;

use serde::{Deserialize, Serialize};

use crate::rules::RewriteRule;

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposedRequest {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub method: String,
    pub target: String,
    pub headers: Vec<HeaderField>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientCommand {
    Ping,
    GetStatus,
    ListTransactions { limit: Option<usize> },
    ListRewriteRules,
    ReplaceRewriteRules { rules: Vec<RewriteRule> },
    ReplayTransaction { id: u64 },
    ExecuteRequest { request: ComposedRequest },
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
    RewriteRules {
        rules: Vec<RewriteRule>,
    },
    ReplayResult {
        source_transaction_id: u64,
        transaction: CapturedTransaction,
    },
    ExecutionResult {
        transaction: CapturedTransaction,
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
    rewrite_rules: &[RewriteRule],
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
        ClientCommand::ListRewriteRules => EngineEvent::RewriteRules {
            rules: rewrite_rules.to_vec(),
        },
        ClientCommand::ReplaceRewriteRules { rules } => EngineEvent::RewriteRules { rules },
        ClientCommand::ReplayTransaction { .. } | ClientCommand::ExecuteRequest { .. } => {
            EngineEvent::Error {
                code: "runtime_command_required".into(),
                message: "command must be handled by the running proxy daemon".into(),
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
    fn replay_command_round_trip_is_stable_json() {
        let command = ClientCommand::ReplayTransaction { id: 42 };
        let json = serde_json::to_string(&command).unwrap();
        assert_eq!(json, r#"{"type":"replay_transaction","id":42}"#);
        let decoded: ClientCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, command);
    }

    #[test]
    fn execute_request_round_trip_is_stable_json() {
        let command = ClientCommand::ExecuteRequest {
            request: ComposedRequest {
                scheme: "https".into(),
                host: "example.com".into(),
                port: None,
                method: "POST".into(),
                target: "/v1/test".into(),
                headers: vec![HeaderField {
                    name: "Content-Type".into(),
                    value: "application/json".into(),
                }],
                body: Some("{\"ok\":true}".into()),
            },
        };
        let json = serde_json::to_string(&command).unwrap();
        assert!(json.contains(r#""type":"execute_request""#));
        assert!(json.contains(r#""scheme":"https""#));
        let decoded: ClientCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, command);
    }

    #[test]
    fn status_event_uses_versioned_envelope() {
        let event = handle_command(ClientCommand::GetStatus, &EngineStatus::default(), &[], &[]);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"status""#));
        assert!(json.contains(r#""protocol_version":1"#));
        assert!(json.contains(r#""tls_interception_enabled":false""#));
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
            &[],
        );
        let EngineEvent::Transactions { transactions } = event else {
            panic!("expected transaction list");
        };
        assert_eq!(transactions.len(), 2);
        assert_eq!(transactions[0].id, 1);
    }

    #[test]
    fn rewrite_rules_round_trip_over_protocol() {
        let rules = vec![RewriteRule {
            id: "debug-header".into(),
            enabled: true,
            host_contains: Some("example.com".into()),
            path_prefix: None,
            actions: vec![crate::rules::RewriteAction::SetHeader {
                name: "X-Debug".into(),
                value: "1".into(),
            }],
            response_actions: vec![crate::rules::ResponseRewriteAction::SetStatus { value: 503 }],
        }];
        let command = ClientCommand::ReplaceRewriteRules {
            rules: rules.clone(),
        };
        let json = serde_json::to_string(&command).unwrap();
        let decoded: ClientCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, command);
        assert!(json.contains("response_actions"));

        let event = handle_command(command, &EngineStatus::default(), &[], &rules);
        let EngineEvent::RewriteRules { rules: echoed } = event else {
            panic!("expected rewrite rule event");
        };
        assert_eq!(echoed, rules);
    }

    #[test]
    fn replay_result_round_trips() {
        let transaction = sample_transaction(9);
        let event = EngineEvent::ReplayResult {
            source_transaction_id: 4,
            transaction,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"replay_result""#));
        let decoded: EngineEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn execution_result_round_trips() {
        let event = EngineEvent::ExecutionResult {
            transaction: sample_transaction(10),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"execution_result""#));
        let decoded: EngineEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn legacy_rule_json_defaults_response_actions() {
        let json = r#"{"id":"legacy","enabled":true,"host_contains":null,"path_prefix":null,"actions":[]}"#;
        let rule: RewriteRule = serde_json::from_str(json).unwrap();
        assert!(rule.response_actions.is_empty());
    }

    #[test]
    fn default_status_starts_stopped() {
        let status = EngineStatus::default();
        assert_eq!(status.protocol_version, PROTOCOL_VERSION);
        assert_eq!(status.proxy_state, ProxyState::Stopped);
        assert_eq!(status.captured_transactions, 0);
        assert!(!status.tls_interception_enabled);
    }

    fn sample_transaction(id: u64) -> CapturedTransaction {
        CapturedTransaction {
            id,
            started_at_unix_ms: 1,
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
        }
    }
}
