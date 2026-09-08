mod certificates;

use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use certificates::CertificateAuthority;
use proxy_core::{
    handle_command,
    http::{find_request_head_end, parse_request_head, rewrite_to_origin_form, MAX_REQUEST_HEAD_BYTES},
    ClientCommand, EngineStatus, ProxyState,
};
use rustls::{pki_types::ServerName, ClientConfig, RootCertStore, ServerConfig};
use tokio::{
    io::{copy_bidirectional, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
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

    let runtime = RuntimeConfig {
        tls_interception_enabled,
        certificate_authority,
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
                tokio::spawn(async move {
                    if let Err(error) = serve_control_client(stream, status).await {
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
        let event = handle_command(command, &snapshot);
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
    let buffered = match read_request_head(&mut downstream).await {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = downstream
                .write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
                .await;
            return Err(error);
        }
    };

    let head_end = find_request_head_end(&buffered)?
        .context("request head unexpectedly incomplete after read")?;
    let parsed = parse_request_head(&buffered[..head_end])?;
    let upstream_addr = format!("{}:{}", parsed.destination.host, parsed.destination.port);

    let mut upstream = match TcpStream::connect(&upstream_addr).await {
        Ok(stream) => stream,
        Err(error) => {
            let _ = downstream
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
                .await;
            return Err(error).with_context(|| format!("failed to connect upstream {upstream_addr}"));
        }
    };

    {
        let mut snapshot = status.write().await;
        snapshot.captured_transactions = snapshot.captured_transactions.saturating_add(1);
    }

    info!(
        method = %parsed.method,
        target = %parsed.target,
        upstream = %upstream_addr,
        connect = parsed.destination.is_connect,
        "captured transaction"
    );

    if parsed.destination.is_connect {
        downstream
            .write_all(b"HTTP/1.1 200 Connection Established\r\nProxy-Agent: proxyman-clone\r\n\r\n")
            .await?;

        if runtime.tls_interception_enabled {
            let ca = runtime
                .certificate_authority
                .as_ref()
                .context("TLS interception enabled without certificate authority")?;
            return serve_intercepted_tls(downstream, upstream, &parsed.destination.host, ca).await;
        }
    } else {
        let rewritten_head = rewrite_to_origin_form(&buffered[..head_end], &parsed);
        upstream.write_all(&rewritten_head).await?;
        if buffered.len() > head_end {
            upstream.write_all(&buffered[head_end..]).await?;
        }
    }

    let _ = copy_bidirectional(&mut downstream, &mut upstream).await?;
    Ok(())
}

async fn serve_intercepted_tls(
    downstream: TcpStream,
    upstream: TcpStream,
    host: &str,
    ca: &CertificateAuthority,
) -> anyhow::Result<()> {
    let identity = ca.identity_for_host(host).await?;
    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(identity.cert_chain.clone(), identity.private_key.clone_key())
        .context("failed to build downstream TLS configuration")?;
    let acceptor = TlsAcceptor::from(Arc::new(server_config));
    let mut downstream_tls = acceptor
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
    let mut upstream_tls = connector
        .connect(server_name, upstream)
        .await
        .with_context(|| format!("upstream TLS handshake failed for {host}"))?;

    info!(%host, "TLS MITM session established");
    let _ = copy_bidirectional(&mut downstream_tls, &mut upstream_tls).await?;
    Ok(())
}

async fn read_request_head(stream: &mut TcpStream) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];

    loop {
        if find_request_head_end(&bytes)?.is_some() {
            return Ok(bytes);
        }
        if bytes.len() >= MAX_REQUEST_HEAD_BYTES {
            anyhow::bail!("request head exceeded {} bytes", MAX_REQUEST_HEAD_BYTES);
        }

        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            anyhow::bail!("connection closed before request head completed");
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
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
