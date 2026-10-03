//! Phase 2 — Modelfile registry + CAS blob store (AI BaaS).
//!
//! ```text
//! Source (HF / Upload / URL / S3 pointer)
//!     │
//!     │  resolve sha256 → CAS dedup check
//!     ▼
//! gguf_blobs   (sha256 PK)  ←──── Garage S3 (gguf-blobs/{sha256}.gguf)
//!     ▲                           Local PV (/var/veronex/blobs/{sha256}.gguf, LRU)
//!     │ FK
//! veronex_models  (model_id PK = "{family}:{quantization}")
//!     │
//!     ▼
//! Phase 3 ProcessManager spawns llama-server with the resolved blob path.
//! ```
//!
//! This file owns module wiring only. Components:
//!
//! - [`blob_store`] — Garage S3 CAS client (HEAD / GET / PUT / DELETE +
//!   ensure_bucket).
//!
//! Future slices add: source adapters (`source/`), Local PV LRU (`local_pv`),
//! registry repos (in `infrastructure/outbound/persistence/`), install
//! orchestrator FSM (`install_orchestrator`).

pub mod blob_store;
pub mod install_orchestrator;
pub mod local_pv;
pub mod source;
pub mod startup_recovery;

pub use blob_store::BlobStore;
pub use install_orchestrator::{InstallEvent, InstallOrchestrator};
pub use local_pv::{LocalPv, WriteOutcome};
pub use source::{HfSource, ModelSource};
