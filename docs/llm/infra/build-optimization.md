# Build Performance Optimization

> SSOT | **Last Updated**: 2026-03-10

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
| sccache | Compilation cache backed by MinIO S3 | Active in CI; Dockerfiles use arch-aware `TARGETARCH` (amd64→x86_64, arm64→aarch64). Secret `sccache_env` is optional (CI provides it, local builds skip). |
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

## Future (Nightly Only)

| Feature | Expected Gain | Requirement |
|---------|--------------|-------------|
| Cranelift codegen backend | ~20% codegen speedup | `rustup +nightly component add rustc-codegen-cranelift` |
| Parallel frontend (`-Zthreads=8`) | 20-50% build speedup | Nightly toolchain |
