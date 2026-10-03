//! veronex-llm-agent — per-node HTTP API that hosts `llama-server` for
//! Veronex managed providers (Phase 3).
//!
//! Runs on every compute node:
//! - Mac M-chip baremetal (homebrew tap, launchd plist)
//! - AMD Strix Halo k8s pods (DaemonSet + helm chart)
//!
//! The Veronex API server treats this agent as a remote process manager
//! over HTTP. Endpoints:
//!
//! | Method | Path                     | Purpose                                  |
//! |--------|--------------------------|------------------------------------------|
//! | GET    | `/probe`                 | OS / arch / GPU / VRAM / RAM detection    |
//! | POST   | `/spawn`                 | start a llama-server process              |
//! | DELETE | `/process/{pid}`         | SIGTERM the process                       |
//! | GET    | `/health/{port}`         | proxy `GET :{port}/health` from llama     |
//! | POST   | `/blobs/{sha256}`        | fetch blob from Garage to local PV        |
//! | GET    | `/blobs/{sha256}`        | check blob presence                       |
//! | GET    | `/healthz`               | liveness for k8s + probe                  |
//!
//! The agent owns a Local PV cache (`/var/lib/veronex-agent/blobs`) and
//! never relies on the API server's blob store directly — that decouples
//! agent restarts from blob fetches and lets the agent honor `idle TTL`
//! eviction independently.
//!
//! Phase 3 ships the API surface + probe + spawn + healthz. Streaming
//! blob fetches and graceful stop land in the same crate when the
//! Veronex side wires `agent_client.rs`.

pub mod api;
pub mod blob_pv;
pub mod probe;
pub mod process;
pub mod spawn;
