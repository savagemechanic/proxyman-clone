use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use proxy_core::{
    CapturedTransaction, EngineStatus, HeaderField, TransactionState,
    body::{BodyCapture, content_type},
    http::{
        MAX_RESPONSE_HEAD_BYTES, ParsedRequestHead, find_response_head_end, parse_response_head,
        upstream_request_head,
    },
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

use crate::session::SessionStore;

pub async fn execute(
    scheme: &str,
    host: &str,
    port: u16,
    parsed: &ParsedRequestHead,
    body: &[u8],
    session: &SessionStore,
    status: &Arc<RwLock<EngineStatus>>,
    rewrite_rules: &Arc<RwLock<Vec<RewriteRule>>>,
) -> anyhow::Result<u64> {
    let id = begin_transaction(scheme, host, parsed, body, session, status).await;

    let result = async {
        let upstream = TcpStream::connect((host, port))
            .await
            .with_context(|| format!("failed to connect upstream {host}:{port}"))?;

        match scheme {
            "http" => execute_over_stream(upstream, parsed, body, id, session, rewrite_rules).await,
            "https" => {
                let mut roots = RootCertStore::empty();
                roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                let client_config = ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth();
                let connector = TlsConnector::from(Arc::new(client_config));
                let server_name = ServerName::try_from(host.to_owned())
                    .with_context(|| format!("invalid TLS server name {host}"))?;
                let upstream_tls = connector
                    .connect(server_name, upstream)
                    .await
                    .with_context(|| format!("TLS handshake failed for {host}"))?;
                execute_over_stream(upstream_tls, parsed, body, id, session, rewrite_rules).await
            }
            _ => anyhow::bail!("unsupported request scheme after validation"),
        }
    }
    .await;

    if let Err(error) = result {
        session.fail(id).await;
        return Err(error);
    }

    Ok(id)
}

async fn execute_over_stream<S>(
    mut upstream: S,
    parsed: &ParsedRequestHead,
    body: &[u8],
    id: u64,
    session: &SessionStore,
    rewrite_rules: &Arc<RwLock<Vec<RewriteRule>>>,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    upstream.write_all(&upstream_request_head(parsed)).await?;
    upstream.write_all(body).await?;
    upstream.flush().await?;

    let response_buffer = read_response_head(&mut upstream).await?;
    let response_head_end = find_response_head_end(&response_buffer)?
        .context("response head unexpectedly incomplete after read")?;
    let mut response = parse_response_head(&response_buffer[..response_head_end])?;

    let applied_response_rules = {
        let rules = rewrite_rules.read().await;
        apply_response_rules(parsed, &mut response, &rules)
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
        scheme = %if parsed.destination.port == 443 { "https" } else { "http" },
        host = %parsed.destination.host,
        status = response.status_code,
        response_rewrite_rules_applied = applied_response_rules.len(),
        "executed explicit HTTP request"
    );
    Ok(())
}

async fn begin_transaction(
    scheme: &str,
    host: &str,
    parsed: &ParsedRequestHead,
    body: &[u8],
    session: &SessionStore,
    status: &Arc<RwLock<EngineStatus>>,
) -> u64 {
    let id = session.next_id();
    session
        .insert(CapturedTransaction {
            id,
            started_at_unix_ms: unix_time_ms(),
            scheme: scheme.to_owned(),
            host: host.to_owned(),
            method: parsed.method.clone(),
            target: parsed.destination.origin_form_target.clone(),
            request_headers: header_fields(&parsed.headers),
            request_body_bytes: body.len() as u64,
            request_body_preview: None,
            status_code: None,
            response_headers: Vec::new(),
            response_body_bytes: 0,
            response_body_preview: None,
            state: TransactionState::Pending,
        })
        .await;

    let mut request_capture = BodyCapture::default();
    request_capture.ingest(body);
    session
        .set_request_preview(id, request_capture.finish(content_type(&parsed.headers)))
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
            anyhow::bail!("connection closed before response head completed");
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
