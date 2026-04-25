FROM rust:1-slim AS chef
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev g++ mold clang curl ca-certificates
RUN cargo install cargo-chef --locked
# sccache (pre-built) — rustc wrapper backed by MinIO WebDAV cache.
# Arch-aware: Docker's TARGETARCH (amd64/arm64) maps to sccache's naming
# (x86_64/aarch64) so the image builds on both ARM64 Macs and CI runners.
ARG SCCACHE_VERSION=0.8.2
ARG TARGETARCH
RUN case "${TARGETARCH}" in \
      amd64) SCCACHE_ARCH=x86_64 ;; \
      arm64) SCCACHE_ARCH=aarch64 ;; \
      *) echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac && \
    curl -fsSL "https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-${SCCACHE_ARCH}-unknown-linux-musl.tar.gz" \
      | tar -xz -C /tmp \
    && mv "/tmp/sccache-v${SCCACHE_VERSION}-${SCCACHE_ARCH}-unknown-linux-musl/sccache" /usr/local/bin/ \
    && chmod +x /usr/local/bin/sccache
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY workspace-hack/ ./workspace-hack/
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=secret,id=sccache_env \
    [ -f /run/secrets/sccache_env ] && . /run/secrets/sccache_env; \
    RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" \
    cargo chef cook --release -p veronex-embed --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY workspace-hack/ ./workspace-hack/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    --mount=type=secret,id=sccache_env \
    [ -f /run/secrets/sccache_env ] && . /run/secrets/sccache_env; \
    RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" \
    cargo build --release -p veronex-embed --bin veronex-embed && \
    cp /app/target/release/veronex-embed /app/veronex-embed && \
    sccache --show-stats

FROM debian:trixie-slim
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 wget
WORKDIR /app
COPY --from=builder /app/veronex-embed ./

# Model is downloaded at first request via fastembed's HuggingFace Hub client.
# Cache lives at FASTEMBED_CACHE_PATH; mount a PersistentVolume there in K8s so
# downloads persist across pod restarts (first pod warm-up ~30-60s from HF,
# subsequent starts instant). Baking the ~2GB model into the image was dropped
# because the resulting layer exceeded the Gitea registry push threshold.
ENV FASTEMBED_CACHE_PATH=/app/models
RUN mkdir -p /app/models

EXPOSE 3200
CMD ["./veronex-embed"]
