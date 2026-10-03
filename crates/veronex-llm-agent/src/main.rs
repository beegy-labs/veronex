//! veronex-llm-agent binary.
//!
//! Reads its config from env vars:
//!
//! - `VERONEX_AGENT_BIND`         — `0.0.0.0:9100` (default)
//! - `VERONEX_AGENT_BLOB_DIR`     — `/var/lib/veronex-agent` (default)
//! - `VERONEX_AGENT_LLAMA_BIN`    — path to `llama-server`. When unset,
//!   `/spawn` returns 503; useful for pre-deployment smoke tests.
//! - `RUST_LOG`                   — tracing filter (default: `info`)

use std::net::SocketAddr;

use anyhow::{Context, Result};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use veronex_llm_agent::api::{router, AgentState};
use veronex_llm_agent::blob_pv::BlobPv;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    init_tracing();

    let bind = std::env::var("VERONEX_AGENT_BIND")
        .unwrap_or_else(|_| "0.0.0.0:9100".to_string());
    let addr: SocketAddr = bind
        .parse()
        .with_context(|| format!("invalid VERONEX_AGENT_BIND={bind}"))?;

    let blob_dir = std::env::var("VERONEX_AGENT_BLOB_DIR")
        .unwrap_or_else(|_| "/var/lib/veronex-agent".to_string());
    let pv = BlobPv::new(&blob_dir);
    pv.ensure_root()
        .await
        .with_context(|| format!("ensure_root({blob_dir})"))?;

    let mut state = AgentState::new(pv);
    if let Ok(p) = std::env::var("VERONEX_AGENT_LLAMA_BIN") {
        state.binary_path = Some(std::sync::Arc::new(std::path::PathBuf::from(p)));
    } else {
        tracing::warn!(
            "VERONEX_AGENT_LLAMA_BIN unset — /spawn will return 503 until configured",
        );
    }

    let app = router(state);
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("veronex-llm-agent listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}
