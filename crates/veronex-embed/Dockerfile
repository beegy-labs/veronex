FROM rust:1-slim AS chef
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev g++ mold clang
RUN cargo install cargo-chef --locked
# sccache (pre-built) — rustc wrapper backed by MinIO WebDAV cache
ARG SCCACHE_VERSION=0.8.2
RUN curl -fsSL https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl.tar.gz \
  | tar -xz -C /tmp \
  && mv /tmp/sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl/sccache /usr/local/bin/ \
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
    . /run/secrets/sccache_env && \
    RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" \
    cargo chef cook --release -p veronex-embed --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY workspace-hack/ ./workspace-hack/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    --mount=type=secret,id=sccache_env \
    . /run/secrets/sccache_env && \
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

# Pre-bake the embedding model into the image at build time so the container
# starts immediately without downloading from HuggingFace at runtime.
ENV FASTEMBED_CACHE_PATH=/app/models
RUN PRELOAD_ONLY=1 ./veronex-embed

EXPOSE 3200
CMD ["./veronex-embed"]
