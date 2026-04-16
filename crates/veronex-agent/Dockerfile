FROM rust:1-alpine3.23 AS chef
RUN --mount=type=cache,target=/var/cache/apk \
    apk add musl-dev mold clang
RUN cargo install cargo-chef --locked
# sccache (pre-built) — rustc wrapper backed by MinIO WebDAV cache
ARG SCCACHE_VERSION=0.8.2
RUN wget -qO- https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl.tar.gz \
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
    cargo chef cook --release -p veronex-agent --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY workspace-hack/ ./workspace-hack/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    --mount=type=secret,id=sccache_env \
    . /run/secrets/sccache_env && \
    RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" \
    cargo build --release -p veronex-agent && \
    cp /app/target/release/veronex-agent /app/veronex-agent && \
    sccache --show-stats

FROM alpine:3.23
RUN --mount=type=cache,target=/var/cache/apk \
    apk add ca-certificates
WORKDIR /app
COPY --from=builder /app/veronex-agent ./
CMD ["./veronex-agent"]
