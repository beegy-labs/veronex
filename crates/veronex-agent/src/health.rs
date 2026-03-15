/// Minimal health HTTP server for K8s probes.
use std::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

use crate::HealthState;

const OK: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
const UNAVAILABLE: &[u8] = b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 11\r\n\r\nnot ready\r\n";
const NOT_FOUND: &[u8] = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";

pub async fn serve(port: u16, state: Arc<HealthState>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!(port, "health server listening");

    loop {
        let (mut stream, _) = listener.accept().await?;
        let state = state.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 256];
            let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
            let req = String::from_utf8_lossy(&buf);

            let response = if req.contains("GET /startup") {
                if state.started.load(Ordering::Relaxed) { OK } else { UNAVAILABLE }
            } else if req.contains("GET /ready") {
                if state.ready.load(Ordering::Relaxed) { OK } else { UNAVAILABLE }
            } else if req.contains("GET /health") {
                if state.alive.load(Ordering::Relaxed) { OK } else { UNAVAILABLE }
            } else {
                NOT_FOUND
            };

            let _ = stream.write_all(response).await;
            let _ = stream.shutdown().await;
        });
    }
}
