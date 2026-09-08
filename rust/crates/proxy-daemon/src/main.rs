mod certificates;
mod session;

use std::{
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use certificates::CertificateAuthority;
use proxy_core::{
    BodyPreview, CapturedTransaction, ClientCommand, EngineStatus, HeaderField, ProxyState,
    TransactionState,
    body::{BodyCapture, content_type},
    handle_command,
    http::{
        MAX_REQUEST_HEAD_BYTES, MAX_RESPONSE_HEAD_BYTES, content_length, find_request_head_end,
        find_response_head_end, is_chunked, parse_request_head, parse_response_head,
        upstream_request_head,
    },
};
use rustls::{ClientConfig, RootCertStore, ServerConfig, pki_types::ServerName};
use session::SessionStore;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::RwLock,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tracing::{info, warn};

const DEFAULT_CONTROL_ADDR: &str = "127.0.0.1:9099";
const DEFAULT_PROXY_ADDR: &str = "127.0.0.1:8080";

#[derive(Clone)]
struct RuntimeConfig {
    tls_interception_enabled: bool,
    certificate_authority: Option<CertificateAuthority>,
    session: SessionStore,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "proxy_daemon=info".into()),
        )
        .init();

    let control_addr = std::env::var("PROXYMAN_CLONE_CONTROL_ADDR")
        .unwrap_or_else(|_| DEFAULT_CONTROL_ADDR.to_owned());
    let proxy_addr = std::env::var("PROXYMAN_CLONE_PROXY_ADDR")
        .unwrap_or_else(|_| DEFAULT_PROXY_ADDR.to_owned());
    let tls_interception_enabled = env_flag("PROXYMAN_CLONE_TLS_INTERCEPT");

    let certificate_authority = if tls_interception_enabled {
        let directory = certificate_storage_dir();
        let ca = CertificateAuthority::load_or_create(&directory).await?;
        info!(
            certificate = %ca.certificate_path().display(),
            "TLS interception enabled; install and trust the local CA only on authorized development clients"
        );
        Some(ca)
    } else {
        info!("TLS interception disabled; CONNECT requests will be tunneled transparently");
        None
    };

    let session = SessionStore::default();
    let runtime = RuntimeConfig {
        tls_interception_enabled,
        certificate_authority,
        session: session.clone(),
    };

    let control_listener = TcpListener::bind(&control_addr)
        .await
        .with_context(|| format!("failed to bind engine control socket at {control_addr}"))?;
    let proxy_listener = TcpListener::bind(&proxy_addr)
        .await
        .with_context(|| format!("failed to bind proxy listener at {proxy_addr}"))?;

    let status = Arc::new(RwLock::new(EngineStatus {
        proxy_state: ProxyState::Running,
        listen_address: Some(proxy_addr.clone()),
        tls_interception_enabled,
        ..EngineStatus::default()
    }));

    info!(address = %control_addr, "engine control socket listening");
    info!(address = %proxy_addr, "HTTP proxy listening");

    loop {
        tokio::select! {
            accepted = control_listener.accept() => {
                let (stream, peer) = accepted?;
                let status = Arc::clone(&status);
                let session = session.clone();
                tokio::spawn(async move {
                    if let Err(error) = serve_control_client(stream, status, session).await {
                        warn!(%peer, %error, "control client disconnected with error");
                    }
                });
            }
            accepted = proxy_listener.accept() => {
                let (stream, peer) = accepted?;
                let status = Arc::clone(&status);
                let runtime = runtime.clone();
                tokio::spawn(async move {
                    if let Err(error) = serve_proxy_client(stream, status, runtime).await {
                        warn!(%peer, %error, "proxy client disconnected with error");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
                status.write().await.proxy_state = ProxyState::Stopping;
                break;
            }
        }
    }

    status.write().await.proxy_state = ProxyState::Stopped;
    Ok(())
}

async fn serve_control_client(
    stream: TcpStream,
    status: Arc<RwLock<EngineStatus>>,
    session: SessionStore,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let command = match serde_json::from_str::<ClientCommand>(&line) {
            Ok(command) => command,
            Err(error) => {
                let event = proxy_core::EngineEvent::Error {
                    code: "invalid_command".into(),
                    message: error.to_string(),
                };
                writer
                    .write_all(format!("{}\n", serde_json::to_string(&event)?).as_bytes())
                    .await?;
                continue;
            }
        };

        let snapshot = status.read().await.clone();
        let transactions = session.list().await;
        let event = handle_command(command, &snapshot, &transactions);
        writer
            .write_all(format!("{}\n", serde_json::to_string(&event)?).as_bytes())
            .await?;
    }

    Ok(())
}

async fn serve_proxy_client(
    mut downstream: TcpStream,
    status: Arc<RwLock<EngineStatus>>,
    runtime: RuntimeConfig,
) -> anyhow::Result<()> {
    let buffered = match read_http_head(&mut downstream, HeadKind::Request).await {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = downstream
                .write_all(
                    b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                )
                .await;
            return Err(error);
        }
    };

    let head_end = find_request_head_end(&buffered)?
        .context("request head unexpectedly incomplete after read")?;
    let parsed = parse_request_head(&buffered[..head_end])?;
    let upstream_addr = format!("{}:{}", parsed.destination.host, parsed.destination.port);

    let upstream = match TcpStream::connect(&upstream_addr).await {
        Ok(stream) => stream,
        Err(error) => {
            let _ = downstream
                .write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                )
                .await;
            return Err(error)
                .with_context(|| format!("failed to connect upstream {upstream_addr}"));
        }
    };

    if parsed.destination.is_connect {
        downstream
            .write_all(
                b"HTTP/1.1 200 Connection Established\r\nProxy-Agent: proxyman-clone\r\n\r\n",
            )
            .await?;

        if runtime.tls_interception_enabled {
            let ca = runtime
                .certificate_authority
                .as_ref()
                .context("TLS interception enabled without certificate authority")?;
            return serve_intercepted_tls(
                downstream,
                upstream,
                &parsed.destination.host,
                ca,
                status,
                runtime.session,
            )
            .await;
        }

        let id = begin_transaction(
            &runtime.session,
            &status,
            "tunnel",
            &parsed.destination.host,
            &parsed,
        )
        .await;
        runtime
            .session
            .complete(id, 200, Vec::new(), empty_preview())
            .await;
        let mut upstream = upstream;
        let _ = tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await?;
        return Ok(());
    }

    proxy_single_exchange(
        downstream,
        upstream,
        "http",
        Some(buffered),
        status,
        runtime.session,
    )
    .await
}

async fn serve_intercepted_tls(
    downstream: TcpStream,
    upstream: TcpStream,
    host: &str,
    ca: &CertificateAuthority,
    status: Arc<RwLock<EngineStatus>>,
    session: SessionStore,
) -> anyhow::Result<()> {
    let identity = ca.identity_for_host(host).await?;
    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            identity.cert_chain.clone(),
            identity.private_key.clone_key(),
        )
        .context("failed to build downstream TLS configuration")?;
    let acceptor = TlsAcceptor::from(Arc::new(server_config));
    let downstream_tls = acceptor
        .accept(downstream)
        .await
        .with_context(|| format!("downstream TLS handshake failed for {host}"))?;

    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let client_config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(client_config));
    let server_name = ServerName::try_from(host.to_owned())
        .with_context(|| format!("invalid upstream TLS server name {host}"))?;
    let upstream_tls = connector
        .connect(server_name, upstream)
        .await
        .with_context(|| format!("upstream TLS handshake failed for {host}"))?;

    info!(%host, "TLS MITM session established");
    proxy_single_exchange(downstream_tls, upstream_tls, "https", None, status, session).await
}

async fn proxy_single_exchange<D, U>(
    mut downstream: D,
    mut upstream: U,
    scheme: &str,
    initial_request: Option<Vec<u8>>,
    status: Arc<RwLock<EngineStatus>>,
    session: SessionStore,
) -> anyhow::Result<()>
where
    D: AsyncRead + AsyncWrite + Unpin,
    U: AsyncRead + AsyncWrite + Unpin,
{
    let buffered = match initial_request {
        Some(bytes) => bytes,
        None => read_http_head(&mut downstream, HeadKind::Request).await?,
    };
    let head_end = find_request_head_end(&buffered)?
        .context("request head unexpectedly incomplete after read")?;
    let parsed = parse_request_head(&buffered[..head_end])?;

    let id = begin_transaction(&session, &status, scheme, &parsed.destination.host, &parsed).await;
    let request_body_length = content_length(&parsed.headers)?.unwrap_or(0);

    if is_chunked(&parsed.headers) {
        session.fail(id).await;
        downstream
            .write_all(
                b"HTTP/1.1 501 Not Implemented\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            )
            .await?;
        anyhow::bail!("chunked request bodies are not supported by the capture path yet");
    }

    upstream.write_all(&upstream_request_head(&parsed)).await?;
    let already_buffered = &buffered[head_end..];
    let initial_body_bytes = already_buffered.len().min(request_body_length as usize);
    let mut request_capture = BodyCapture::default();
    if initial_body_bytes > 0 {
        let body = &already_buffered[..initial_body_bytes];
        request_capture.ingest(body);
        upstream.write_all(body).await?;
    }
    copy_exact_remaining_with_capture(
        &mut downstream,
        &mut upstream,
        request_body_length.saturating_sub(initial_body_bytes as u64),
        &mut request_capture,
    )
    .await?;
    upstream.flush().await?;
    session
        .set_request_preview(id, request_capture.finish(content_type(&parsed.headers)))
        .await;

    let response_buffer = match read_http_head(&mut upstream, HeadKind::Response).await {
        Ok(bytes) => bytes,
        Err(error) => {
            session.fail(id).await;
            return Err(error);
        }
    };
    let response_head_end = find_response_head_end(&response_buffer)?
        .context("response head unexpectedly incomplete after read")?;
    let response = parse_response_head(&response_buffer[..response_head_end])?;

    downstream.write_all(&response_buffer).await?;
    let mut response_capture = BodyCapture::default();
    response_capture.ingest(&response_buffer[response_head_end..]);
    copy_with_capture(&mut upstream, &mut downstream, &mut response_capture).await?;
    downstream.flush().await?;

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
        scheme,
        method = %parsed.method,
        host = %parsed.destination.host,
        status = response.status_code,
        "captured HTTP exchange"
    );
    Ok(())
}

async fn begin_transaction(
    session: &SessionStore,
    status: &Arc<RwLock<EngineStatus>>,
    scheme: &str,
    host: &str,
    parsed: &proxy_core::http::ParsedRequestHead,
) -> u64 {
    let id = session.next_id();
    let request_body_bytes = content_length(&parsed.headers).ok().flatten().unwrap_or(0);
    session
        .insert(CapturedTransaction {
            id,
            started_at_unix_ms: unix_time_ms(),
            scheme: scheme.to_owned(),
            host: host.to_owned(),
            method: parsed.method.clone(),
            target: parsed.destination.origin_form_target.clone(),
            request_headers: header_fields(&parsed.headers),
            request_body_bytes,
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

async fn copy_exact_remaining_with_capture<R, W>(
    reader: &mut R,
    writer: &mut W,
    mut remaining: u64,
    capture: &mut BodyCapture,
) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = [0_u8; 16 * 1024];
    while remaining > 0 {
        let wanted = remaining.min(buffer.len() as u64) as usize;
        let read = reader.read(&mut buffer[..wanted]).await?;
        if read == 0 {
            anyhow::bail!("connection closed before declared request body completed");
        }
        capture.ingest(&buffer[..read]);
        writer.write_all(&buffer[..read]).await?;
        remaining -= read as u64;
    }
    Ok(())
}

async fn copy_with_capture<R, W>(
    reader: &mut R,
    writer: &mut W,
    capture: &mut BodyCapture,
) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        capture.ingest(&buffer[..read]);
        writer.write_all(&buffer[..read]).await?;
    }
    Ok(())
}

fn empty_preview() -> BodyPreview {
    BodyPreview {
        content_type: None,
        text: None,
        captured_bytes: 0,
        total_bytes: 0,
        truncated: false,
    }
}

#[derive(Clone, Copy)]
enum HeadKind {
    Request,
    Response,
}

async fn read_http_head<S>(stream: &mut S, kind: HeadKind) -> anyhow::Result<Vec<u8>>
where
    S: AsyncRead + Unpin,
{
    let limit = match kind {
        HeadKind::Request => MAX_REQUEST_HEAD_BYTES,
        HeadKind::Response => MAX_RESPONSE_HEAD_BYTES,
    };
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];

    loop {
        let complete = match kind {
            HeadKind::Request => find_request_head_end(&bytes)?,
            HeadKind::Response => find_response_head_end(&bytes)?,
        };
        if complete.is_some() {
            return Ok(bytes);
        }
        if bytes.len() >= limit {
            anyhow::bail!("HTTP head exceeded {limit} bytes");
        }

        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            anyhow::bail!("connection closed before HTTP head completed");
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn certificate_storage_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var("PROXYMAN_CLONE_CERT_DIR") {
        return PathBuf::from(override_dir);
    }

    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("ProxymanClone")
            .join("certificates");
    }

    PathBuf::from(".proxyman-clone/certificates")
}
