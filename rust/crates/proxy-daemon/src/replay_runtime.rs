use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use proxy_core::{
    CapturedTransaction, EngineEvent, EngineStatus, HeaderField, TransactionState,
    body::{BodyCapture, content_type},
    http::{MAX_RESPONSE_HEAD_BYTES, find_response_head_end, parse_response_head},
    rules::{RewriteRule, apply_response_rules},
};
use rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::RwLock,
};
use tokio_rustls::TlsConnector;
use tracing::info;

use crate::{
    replay::{PreparedReplayRequest, prepare},
    session::SessionStore,
};

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

    let replayed_id = match execute_replay(prepared, session, status, rewrite_rules).await {
        Ok(id) => id,
        Err(error) => {
            return EngineEvent::Error {
                code: "replay_failed".into(),
                message: error.to_string(),
            };
        }
    };

    match session.get(replayed_id).await {
        Some(transaction) => EngineEvent::ReplayResult {
            source_transaction_id,
            transaction,
        },
        None => EngineEvent::Error {
            code: "replay_result_missing".into(),
            message: "replay completed but its captured transaction is unavailable".into(),
        },
    }
}

async fn execute_replay(
    prepared: PreparedReplayRequest,
    session: &SessionStore,
    status: &Arc<RwLock<EngineStatus>>,
    rewrite_rules: &Arc<RwLock<Vec<RewriteRule>>>,
) -> anyhow::Result<u64> {
    let id = begin_replay_transaction(session, status, &prepared).await;

    let mut request_capture = BodyCapture::default();
    request_capture.ingest(&prepared.body);
    session
        .set_request_preview(
            id,
            request_capture.finish(content_type(&prepared.parsed.headers)),
        )
        .await;

    let result = async {
        let upstream = TcpStream::connect((prepared.host.as_str(), prepared.port))
            .await
            .with_context(|| {
                format!(
                    "failed to connect replay upstream {}:{}",
                    prepared.host, prepared.port
                )
            })?;

        match prepared.scheme.as_str() {
            "http" => replay_over_stream(upstream, &prepared, id, session, rewrite_rules).await,
            "https" => {
                let mut roots = RootCertStore::empty();
                roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                let client_config = ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth();
                let connector = TlsConnector::from(Arc::new(client_config));
                let server_name = ServerName::try_from(prepared.host.clone())
                    .with_context(|| format!("invalid replay TLS server name {}", prepared.host))?;
                let upstream_tls = connector
                    .connect(server_name, upstream)
                    .await
                    .with_context(|| {
                        format!("replay TLS handshake failed for {}", prepared.host)
                    })?;
                replay_over_stream(upstream_tls, &prepared, id, session, rewrite_rules).await
            }
            _ => anyhow::bail!("unsupported replay scheme after preparation"),
        }
    }
    .await;

    if let Err(error) = result {
        session.fail(id).await;
        return Err(error);
    }

    Ok(id)
}

async fn replay_over_stream<S>(
    mut upstream: S,
    prepared: &PreparedReplayRequest,
    id: u64,
    session: &SessionStore,
    rewrite_rules: &Arc<RwLock<Vec<RewriteRule>>>,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    upstream.write_all(&prepared.serialize()).await?;
    upstream.flush().await?;

    let response_buffer = read_response_head(&mut upstream).await?;
    let response_head_end = find_response_head_end(&response_buffer)?
        .context("replay response head unexpectedly incomplete after read")?;
    let mut response = parse_response_head(&response_buffer[..response_head_end])?;

    let applied_response_rules = {
        let rules = rewrite_rules.read().await;
        apply_response_rules(&prepared.parsed, &mut response, &rules)
    };

    let mut response_capture = BodyCapture::default();
    response_capture.ingest(&response_buffer[response_head_end..]);
    read_to_capture(&mut upstream, &mut response_capture).await?;

    session
        .complete(
            id,
            response.status_code,
            header_fields(&response.headers),
            response_capture.finish(content_type(&response.headers)),
        )
        .await;

    info!(
        transaction_id = id,
        source_transaction_id = prepared.source_transaction_id,
        scheme = %prepared.scheme,
        host = %prepared.host,
        status = response.status_code,
        response_rewrite_rules_applied = applied_response_rules.len(),
        "replayed captured HTTP request"
    );
    Ok(())
}

async fn begin_replay_transaction(
    session: &SessionStore,
    status: &Arc<RwLock<EngineStatus>>,
    prepared: &PreparedReplayRequest,
) -> u64 {
    let id = session.next_id();
    session
        .insert(CapturedTransaction {
            id,
            started_at_unix_ms: unix_time_ms(),
            scheme: prepared.scheme.clone(),
            host: prepared.host.clone(),
            method: prepared.parsed.method.clone(),
            target: prepared.parsed.destination.origin_form_target.clone(),
            request_headers: header_fields(&prepared.parsed.headers),
            request_body_bytes: prepared.body.len() as u64,
            request_body_preview: None,
            status_code: None,
            response_headers: Vec::new(),
            response_body_bytes: 0,
            response_body_preview: None,
            state: TransactionState::Pending,
        })
        .await;
    let mut engine_status = status.write().await;
    engine_status.captured_transactions = engine_status.captured_transactions.saturating_add(1);
    id
}

fn header_fields(headers: &[(String, String)]) -> Vec<HeaderField> {
    headers
        .iter()
        .map(|(name, value)| HeaderField {
            name: name.clone(),
            value: value.clone(),
        })
        .collect()
}

async fn read_response_head<S>(stream: &mut S) -> anyhow::Result<Vec<u8>>
where
    S: AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];

    loop {
        if find_response_head_end(&bytes)?.is_some() {
            return Ok(bytes);
        }
        if bytes.len() >= MAX_RESPONSE_HEAD_BYTES {
            anyhow::bail!("HTTP response head exceeded {MAX_RESPONSE_HEAD_BYTES} bytes");
        }

        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            anyhow::bail!("connection closed before replay response head completed");
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

async fn read_to_capture<R>(reader: &mut R, capture: &mut BodyCapture) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
{
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(());
        }
        capture.ingest(&buffer[..read]);
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
