use std::sync::Arc;

use anyhow::Context;
use proxy_core::{
    handle_command,
    http::{find_request_head_end, parse_request_head, rewrite_to_origin_form, MAX_REQUEST_HEAD_BYTES},
    ClientCommand, EngineStatus, ProxyState,
};
use tokio::{
    io::{copy_bidirectional, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::RwLock,
};
use tracing::{info, warn};

const DEFAULT_CONTROL_ADDR: &str = "127.0.0.1:9099";
const DEFAULT_PROXY_ADDR: &str = "127.0.0.1:8080";

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

    let control_listener = TcpListener::bind(&control_addr)
        .await
        .with_context(|| format!("failed to bind engine control socket at {control_addr}"))?;
    let proxy_listener = TcpListener::bind(&proxy_addr)
        .await
        .with_context(|| format!("failed to bind proxy listener at {proxy_addr}"))?;

    let status = Arc::new(RwLock::new(EngineStatus {
        proxy_state: ProxyState::Running,
        listen_address: Some(proxy_addr.clone()),
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
                tokio::spawn(async move {
                    if let Err(error) = serve_proxy_client(stream, status).await {
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
