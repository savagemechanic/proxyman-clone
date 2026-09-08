use std::sync::Arc;

use proxy_core::{ComposedRequest, EngineEvent, EngineStatus, rules::RewriteRule};
use tokio::sync::RwLock;

use crate::{composer, request_runtime, session::SessionStore};

pub async fn execute_request(
    request: &ComposedRequest,
    session: &SessionStore,
    status: &Arc<RwLock<EngineStatus>>,
    rewrite_rules: &Arc<RwLock<Vec<RewriteRule>>>,
) -> EngineEvent {
    let prepared = match composer::prepare(request) {
        Ok(prepared) => prepared,
        Err(error) => {
            return EngineEvent::Error {
                code: "invalid_composed_request".into(),
                message: error.to_string(),
            };
        }
    };

    let transaction_id = match request_runtime::execute(
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
                code: "composed_request_failed".into(),
                message: error.to_string(),
            };
        }
    };

    match session.get(transaction_id).await {
        Some(transaction) => EngineEvent::ExecutionResult { transaction },
        None => EngineEvent::Error {
            code: "execution_result_missing".into(),
            message: "request completed but its captured transaction is unavailable".into(),
        },
    }
}
