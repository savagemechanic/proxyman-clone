use std::sync::Arc;

use anyhow::Context;
use proxy_core::{handle_command, ClientCommand, EngineStatus};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::RwLock,
};
use tracing::{info, warn};

const DEFAULT_CONTROL_ADDR: &str = "127.0.0.1:9099";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "proxy_daemon=info".into()),
        )
        .init();

    let bind_addr = std::env::var("PROXYMAN_CLONE_CONTROL_ADDR")
        .unwrap_or_else(|_| DEFAULT_CONTROL_ADDR.to_owned());
    let listener = TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("failed to bind engine control socket at {bind_addr}"))?;
    let status = Arc::new(RwLock::new(EngineStatus::default()));

    info!(address = %bind_addr, "engine control socket listening");

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let status = Arc::clone(&status);
                tokio::spawn(async move {
                    if let Err(error) = serve_client(stream, status).await {
                        warn!(%peer, %error, "control client disconnected with error");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
                break;
            }
        }
    }

    Ok(())
}

async fn serve_client(stream: TcpStream, status: Arc<RwLock<EngineStatus>>) -> anyhow::Result<()> {
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
