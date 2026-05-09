//! Phase 3 — managed `llama-server` process lifecycle.
//!
//! This module ships the data-structure foundation:
//!
//! - [`port_pool::PortPool`] hands out / reclaims TCP ports from a configurable
//!   range so the manager can spawn N processes without colliding.
//! - [`activity_tracker::ActivityTracker`] records per-provider in-flight
//!   counts and request timestamps so the idle reaper can decide who to stop.
//!
//! The orchestrator (`manager.rs`), idle reaper (`idle_manager.rs`), warmup
//! probe, and agent client (`agent_client.rs`) land in a follow-up commit
//! together with the `veronex-agent` sub-crate. Running the actual binary
//! requires either local spawn (Mac baremetal) or HTTP delegation to the
//! agent (k8s pod), and the remote path can't compile until the agent crate
//! exists.

pub mod activity_tracker;
pub mod port_pool;

pub use activity_tracker::{ActivityTracker, RequestGuard};
pub use port_pool::{PortLease, PortPool};
