# SDD: CI Build Optimization (residual tiers)

> Status: archived (Tier 1+2+3 shipped, Tier 4 dropped, Tier 5 wait-and-see) | Created: 2026-04-28 | Archived: 2026-04-29

## Status Snapshot

| Tier | Status | Landed |
| ---- | ------ | ------ |
| 1 — sccache + `CARGO_INCREMENTAL=0` in PR validation | ✓ Done | veronex#89 (`032a675`) |
| 2 — GeoNames data cache (workflow + Dockerfile) | ✓ Done | veronex#89 (`032a675`) |
| 3 — `include_bytes!` → runtime `geo.bin` load | ✓ Done | veronex#89 (`032a675`) |
| 4 — Cranelift backend PoC | Dropped | Low ROI on 2 CPU runner pods (architecture: `arc-runner-set` limits = 2 CPU / 4 GB) |
| 5 — Workspace consolidation | Pending | Audit deferred — not blocking |

This SDD is retained as the canonical record for Tier 1–3 (audit trail) and the active plan for Tier 5. Tier 4 is dropped per resource constraint.

## Residual Work

### Tier 5 — Workspace consolidation (planned, low priority)

| Aspect | Detail |
| ------ | ------ |
| Goal | Merge crates with <500 lines AND only one in-workspace consumer (per `corrode.dev`) — fewer compile units = less cargo metadata pass time on cold build |
| Scope | `crates/` audit. Candidates likely small leaf crates with single-consumer relationship |
| Trigger | Run when CI build time becomes the next-largest bottleneck after Tier 1–3 effects measured (currently ~15 min ci-rust, target ≤10 min) |
| Affected | Cargo.toml workspace members, possibly `workspace-hack` |
| Out of scope | Cross-crate refactoring beyond merging — only flat consolidation |

### Completion criteria (Tier 5)

| Check | Pass condition |
| ----- | -------------- |
| Audit | Inventory of crates × consumers count |
| Merge | `cargo build --workspace` succeeds after each merge |
| Measure | `time cargo build --release --workspace` improves ≥1 min from baseline |

## Verified Effects (Tier 1–3, post-merge)

| Metric | Before | After |
| ------ | ------ | ----- |
| PR validation `test-api` cold compile | 8–12 min | 1–3 min (warm sccache) |
| ci-rust workspace build | 28 min | 15 min (sccache hit) |
| External GeoNames egress per build | 9.7 MB | 0 (cache hit) |
| veronex-mcp binary size | ~24.7 MB | 14.2 MB |

## References (live)

- [Depot — Best practice Rust Dockerfile](https://depot.dev/blog/rust-dockerfile-best-practices)
- [Depot — sccache in GitHub Actions](https://depot.dev/blog/sccache-in-github-actions)
- [xxchan — Stupidly effective ways to optimize Rust compile time](https://xxchan.me/blog/2023-02-17-optimize-rust-comptime-en/)
- [DeepWiki sccache — Rust Compiler Integration](https://deepwiki.com/mozilla/sccache/4.1-rust-compiler-integration)
- [rust-lang/rust#65818 — `include_bytes!` on large blobs compiles slowly](https://github.com/rust-lang/rust/issues/65818) — Tier 3 root cause reference
- [corrode.dev — Tips For Faster Rust Compile Times](https://corrode.dev/blog/tips-for-faster-rust-compile-times/) — Tier 5 heuristic source
