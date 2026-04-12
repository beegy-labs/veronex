# Build Performance Optimization

> SSOT | **Last Updated**: 2026-04-12

## Local Development

### Cargo Config (`.cargo/config.toml`)

| Setting | Value | Effect |
|---------|-------|--------|
| Linker | `mold` via `clang` | ~5x faster linking |
| split-debuginfo | `unpacked` | Faster incremental builds |
| Registry protocol | `sparse` | Faster index updates |

### Cargo Profiles (`Cargo.toml`)

| Profile | debuginfo | Dependencies | LTO | Use case |
|---------|-----------|-------------|-----|----------|
| `dev` | line-tables-only | opt-level=2 | off | Local dev (fast compile, fast deps) |
| `release` | off | opt-level=3 | **full** | Production (max perf) |
| `ci` | off | opt-level=2 | **thin** | CI pipeline (balanced) |

### Workspace Optimization

- **cargo-hakari** (`workspace-hack`): 42 unified deps across workspace crates
  - Prevents redundant rebuilds when switching between crates
  - Run `cargo hakari generate` after dependency changes
- **tokio**: Selected features only (not `full`) — reduces compile units

### Test Runner

- **cargo-nextest**: Parallel test execution, better output
  - Config: `.config/nextest.toml`
  - Run: `cargo nextest run`

---

## Docker Builds

### cargo-chef Pattern (all 3 Rust Dockerfiles)

3-stage build: chef → planner → builder. Deps cached in layer, source-only changes skip dep rebuild.

```dockerfile
FROM rust:1-alpine AS chef
RUN apk add --no-cache musl-dev mold clang
RUN cargo install cargo-chef --locked

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY workspace-hack/ ./workspace-hack/
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" \
    cargo chef cook --release -p <CRATE> --recipe-path recipe.json
```

### Per-Dockerfile Details

| Dockerfile | Extras |
|------------|--------|
| `Dockerfile` | cargo-chef, mold, SQLX_OFFLINE, .sqlx/ |
| `crates/veronex-agent/Dockerfile` | cargo-chef, mold |
| `crates/veronex-analytics/Dockerfile` | cargo-chef, mold, protoc |
| `web/Dockerfile` | npm cache mount + `.next/cache` mount |

### Web Dockerfile Cache

```dockerfile
RUN --mount=type=cache,target=/root/.npm npm ci
COPY . .
RUN --mount=type=cache,target=/app/.next/cache npm run build
```

---

## Toolchain

| Tool | Purpose | Status |
|------|---------|--------|
| mold + clang | Fast linker | Installed |
| sccache | Compilation cache | Config ready (uncomment in .cargo/config.toml) |
| cargo-chef | Docker dep caching | All 3 Rust Dockerfiles |
| cargo-nextest | Parallel test runner | Installed |
| cargo-hakari | workspace-hack management | Installed |

---

## Measured Results

| Metric | Before | After | Speedup |
|--------|--------|-------|---------|
| `cargo check` (clean) | 10m 31s | 1m 41s | **6.3x** |
| `cargo nextest` | — | 205 tests / 0.5s | — |
| `vitest` | — | 42 tests / 2.4s | — |

---

## CI Runners

### Runner Architecture

| Runner | Manager | Purpose |
|--------|---------|---------|
| `arc-runner-set` | Terraform (bootstrap) | General-purpose — used only by `build-runner.yml` |
| `veronex-runner-set` | ArgoCD | Veronex-specific — all other workflows |

`arc-runner-set` is managed by Terraform to avoid chicken-and-egg with ArgoCD bootstrap.
`veronex-runner-set` is managed by ArgoCD (added after ArgoCD exists), scales to zero via KEDA.

### Custom Runner Image (`.github/runner/Dockerfile`)

Built on top of `ghcr.io/actions/actions-runner:latest`, pre-baked with:

| Tool | Purpose |
|------|---------|
| `build-essential`, `pkg-config` | C build toolchain |
| `libssl-dev`, `libcurl4-openssl-dev` | Rust TLS/HTTP deps |
| `clang`, `mold` | Fast linker (5x faster than ld) |
| `cmake` | Native dependency builds |
| Rust stable (rustup) | Cargo, rustc |

Image pushed to `gitea.girok.dev/beegy-labs/veronex-runner:latest` on every push to `develop`/`main`.
Max 5 images retained per branch (SHA-tagged older images pruned automatically).

### Why Custom Runner

ARC runner pods are ephemeral — every job starts a fresh pod.
Default `actions-runner:latest` has no Rust or build tools, requiring ~74s apt-get + ~10s rustup per job.
Custom image eliminates this entirely: `cargo test` starts immediately.

### Workflow Assignment

All `.github/workflows/*.yml` use `veronex-runner-set` **except**:
- `build-runner.yml` → `arc-runner-set` (builds the veronex-runner image — circular dependency risk)

---

## Future (Nightly Only)

| Feature | Expected Gain | Requirement |
|---------|--------------|-------------|
| Cranelift codegen backend | ~20% codegen speedup | `rustup +nightly component add rustc-codegen-cranelift` |
| Parallel frontend (`-Zthreads=8`) | 20-50% build speedup | Nightly toolchain |
