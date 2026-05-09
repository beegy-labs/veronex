//! llama-server (llama.cpp HTTP server) outbound adapter.
//!
//! Phase 1: external mode only — the llama-server process is run by the operator
//! (or future Phase 3 ProcessManager). This module talks to it over HTTP.
//!
//! - `health.rs` — GET /health probe + SlotStatus parsing.
//! - `adapter.rs` — `LlamaServerAdapter` implementing `ModelLifecyclePort` +
//!   `InferenceProviderPort` against the OpenAI-compatible `/v1/chat/completions`.

pub mod adapter;
pub mod health;

pub use adapter::LlamaServerAdapter;
pub use health::{get_health, SlotStatus};
