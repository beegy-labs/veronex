// ── Lint configuration ────────────────────────────────────────────────────────
// warn on unwrap() in production code — use ? or expect() with a message instead
#![warn(clippy::unwrap_used)]
// warn on expect() without a message
#![warn(clippy::expect_used)]
// allow in test code (tests use unwrap extensively)

pub mod application;
pub mod domain;
pub mod infrastructure;
