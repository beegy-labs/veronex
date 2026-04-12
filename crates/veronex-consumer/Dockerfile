FROM rust:1-alpine3.23 AS chef
# cmake + make + perl: required for rdkafka cmake-build + ssl-vendored (OpenSSL from source)
RUN --mount=type=cache,target=/var/cache/apk \
    apk add musl-dev mold clang g++ cmake make perl python3 curl-dev zlib-dev zlib-static
RUN cargo install cargo-chef --locked
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
    RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" \
    cargo chef cook --release -p veronex-consumer --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY workspace-hack/ ./workspace-hack/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" \
    cargo build --release -p veronex-consumer && \
    cp /app/target/release/veronex-consumer /app/veronex-consumer

FROM alpine:3.23
RUN --mount=type=cache,target=/var/cache/apk \
    apk add ca-certificates
WORKDIR /app
COPY --from=builder /app/veronex-consumer ./
CMD ["./veronex-consumer"]
