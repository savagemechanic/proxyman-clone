use std::sync::Arc;

use proxy_core::{EngineEvent, EngineStatus, rules::RewriteRule};
use tokio::sync::RwLock;
use tracing::info;

use crate::{replay::prepare, request_runtime, session::SessionStore};

pub async fn replay_transaction(
    source_transaction_id: u64,
    session: &SessionStore,
    status: &Arc<RwLock<EngineStatus>>,
    rewrite_rules: &Arc<RwLock<Vec<RewriteRule>>>,
) -> EngineEvent {
    let Some(source) = session.get(source_transaction_id).await else {
        return EngineEvent::Error {
            code: "replay_transaction_not_found".into(),
            message: format!("transaction {source_transaction_id} is no longer available"),
        };
    };

    let prepared = match prepare(&source) {
        Ok(prepared) => prepared,
        Err(error) => {
            return EngineEvent::Error {
                code: "replay_not_safe".into(),
                message: error.to_string(),
            };
        }
    };

    let replayed_id = match request_runtime::execute(
        &prepared.scheme,
        &prepared.host,
        prepared.port,
        &prepared.parsed,
        &prepared.body,
        session,
        status,
        rewrite_rules,
    )
    .await
    {
        Ok(id) => id,
        Err(error) => {
            return EngineEvent::Error {
                code: "replay_failed".into(),
                message: error.to_string(),
            };
        }
    };

    match session.get(replayed_id).await {
        Some(transaction) => {
            info!(
                transaction_id = replayed_id,
                source_transaction_id,
                "replayed captured HTTP request"
            );
            EngineEvent::ReplayResult {
                source_transaction_id,
                transaction,
            }
        }
        None => EngineEvent::Error {
            code: "replay_result_missing".into(),
            message: "replay completed but its captured transaction is unavailable".into(),
        },
    }
}
