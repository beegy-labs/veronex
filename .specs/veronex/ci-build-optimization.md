# SDD: CI Build Optimization (sccache + incremental + geo cache + arch)

> Status: planned | Created: 2026-04-28 | Owner: TBD

## Problem

PR validation `Test API` job (`cargo nextest run --all`) takes 8–12 minutes per PR.
Docker build jobs (`ci-rust`, `ci-embed`) are partially optimized via sccache + cargo-chef but still cold-cache slow. The `veronex-mcp/build.rs` GeoNames step adds 40–60 s per run by re-downloading + re-parsing 167k cities. Three root causes amplify each other.

## Root Causes (verified, 2026 best-practice references)

| # | Root cause | Evidence |
|---|-----------|----------|
| R1 | `pr-validation.yaml` test-api uses `RUSTC_WRAPPER=""` + `CARGO_INCREMENTAL=1`. sccache disabled, incremental incompatible with ephemeral runners. | xxchan blog ("disabling incremental → -4 min"); DeepWiki sccache: "incremental compilation is incompatible with sccache" |
| R2 | `crates/veronex-mcp/data/cities1000.txt` is gitignored, downloaded fresh from `download.geonames.org` every build (~9.7 MB external) | `crates/veronex-mcp/build.rs:198-217` |
| R3 | `geo.bin` (10 MB, 167k cities + 917k index) `include_bytes!`-embedded into `veronex-mcp` binary. Inflates crate metadata → every dependent crate's incremental compile slows down | `crates/veronex-mcp/build.rs:223` writes to `OUT_DIR/geo.bin`; `lib.rs` uses `include_bytes!` |

Self-hosted (arc-runner-set) + in-cluster Garage S3 is the **sweet spot** for sccache, not rust-cache. Docker jobs already use sccache; PR validation is the asymmetric leak.

## Solution

Tiered, ROI-ordered. Each tier independently mergeable.

### Tier 1 — sccache + incremental fix (PR validation)

`pr-validation.yaml` `test-api` job:

| Change | Value |
|--------|-------|
| `RUSTC_WRAPPER` | `""` → `sccache` |
| `CARGO_INCREMENTAL` | `"1"` → `"0"` |
| sccache S3 env | inject from existing `SCCACHE_*` secrets (same pool as docker builds) |
| Install sccache binary | curl pre-built x86_64-unknown-linux-musl, /usr/local/bin/ |
| `Swatinem/rust-cache@v2` | add `cache-targets: false` (sccache owns target/) |

Expected: 8–12 min → 1–3 min (warm cache).

### Tier 2 — GeoNames data caching

| Layer | Mechanism |
|-------|-----------|
| pr-validation `test-api` | `actions/cache@v4` on `crates/veronex-mcp/data` with key `geonames-cities1000-v1` |
| `docker/Dockerfile.rust` chef stage | `RUN --mount=type=cache,target=/app/crates/veronex-mcp/data,sharing=locked` |
| `crates/veronex-mcp/Dockerfile` (embed) | same mount |

Expected: -40~60 s/run, eliminates external `download.geonames.org` egress.

### Tier 3 — geo.bin runtime load (architectural)

Replace `include_bytes!("OUT_DIR/geo.bin")` with runtime load:

| Phase | Change |
|-------|--------|
| Build | `build.rs` writes `geo.bin` to `OUT_DIR` only (no embed). Crate metadata stays slim. |
| Image | Garage bucket `veronex-geo-data` contains `geo.bin` (uploaded once by mirror workflow). |
| K8s | `mcp` Deployment `initContainer` `mc cp s3://veronex-geo-data/geo.bin /app/geo.bin` (RWX volume). |
| Runtime | `Geo::load_from_file("/app/geo.bin")` at process start; `OnceLock<GeoIndex>`. |
| Local dev | Fallback: if file missing, load from `OUT_DIR/geo.bin` (build.rs output). |

Expected: -1~3 min (every dependent crate compiles faster, especially incremental).

### Tier 4 — Cranelift backend (optional, nightly)

| Profile | Backend |
|---------|---------|
| `dev`, `test` | Cranelift (`-Zcodegen-backend=cranelift`) |
| `release`, `ci` | LLVM (unchanged) |

Nightly toolchain pinned in `rust-toolchain.toml` for dev/test profile only.
PoC first; merge if stable on workspace (some crates with inline asm may be incompatible).

Expected: -1~2 min on dev/test compile.

### Tier 5 — Workspace consolidation

Merge crates with <500 lines AND only one consumer (per `corrode.dev` heuristic).
Inventory pass; specific crates TBD after audit.

Expected: -1~2 min on cold compile.

## Out of Scope

- Distributed sccache (`sccache --dist`) — minimal real-world adoption, only Mozilla
- GitHub-hosted runner migration — we keep arc-runner-set
- `target/` mount in BuildKit (already covered by sccache)

## Tests

| Tier | Verification |
|------|-------------|
| 1 | `sccache --show-stats` step at end of test-api; assert cache hit count > 0 on second run with no Cargo.lock change |
| 2 | Second consecutive run reuses cache (no `Downloading https://download.geonames.org/...` warning) |
| 3 | `kubectl exec mcp -- ls -la /app/geo.bin` returns the file; `cargo build -p veronex-mcp` produces a binary <2 MB (vs ~12 MB now) |
| 4 | `cargo +nightly test --profile dev` succeeds on entire workspace |
| 5 | `cargo build --workspace` time ≤ baseline -1 min |

## Measurement Plan (before/after)

| Metric | Source |
|--------|--------|
| PR validation test-api elapsed | `gh api /repos/.../actions/runs/<id>/jobs` `(completed_at - started_at)` |
| Docker build elapsed | `gh run list --workflow=ci-rust.yml --json` |
| sccache hit rate | `sccache --show-stats` (CompileFinished cache hits / total) |

Baseline (current, 2026-04-28):
- PR validation test-api: 8–12 min (cold cache, post-Cargo.lock change)
- ci-rust workspace build: ~5 min (sccache warm)

Target (after Tier 1+2):
- PR validation test-api: 1–3 min
- ci-rust workspace build: 2–3 min
- External GeoNames egress: 0 (after first cache miss)

## Rollout Order

1. Tier 1 (sccache + incremental) — independent, smallest blast radius
2. Tier 2 (geo cache) — independent
3. Tier 3 (runtime geo.bin) — architectural; requires Garage bucket + initContainer + initial seed
4. Tier 4 (Cranelift PoC) — optional, separate branch
5. Tier 5 (workspace merge) — opportunistic

Each tier merges independently; no cross-dependency.

## References

- [Depot — Best practice Rust Dockerfile](https://depot.dev/blog/rust-dockerfile-best-practices)
- [Depot — sccache in GitHub Actions](https://depot.dev/blog/sccache-in-github-actions)
- [Earthly — Optimizing Rust Build Speed with sccache](https://earthly.dev/blog/rust-sccache/)
- [Earthly — Incremental Rust builds in CI](https://earthly.dev/blog/incremental-rust-builds/)
- [xxchan — Stupidly effective ways to optimize Rust compile time](https://xxchan.me/blog/2023-02-17-optimize-rust-comptime-en/)
- [DeepWiki sccache — Rust Compiler Integration](https://deepwiki.com/mozilla/sccache/4.1-rust-compiler-integration)
- [corrode.dev — Tips For Faster Rust Compile Times](https://corrode.dev/blog/tips-for-faster-rust-compile-times/)
- [The Rust Performance Book — Build Configuration](https://nnethercote.github.io/perf-book/build-configuration.html)
- [Cargo Book — Optimizing Build Performance](https://doc.rust-lang.org/stable/cargo/guide/build-performance.html)
- [Rust Project Goals 2025h2 — Production-ready Cranelift](https://rust-lang.github.io/rust-project-goals/2025h2/production-ready-cranelift.html)
- [libp2p#3823 — sccache instead of Swatinem/rust-cache](https://github.com/libp2p/rust-libp2p/issues/3823)
- [LukeMathWalker/cargo-chef](https://github.com/LukeMathWalker/cargo-chef)
