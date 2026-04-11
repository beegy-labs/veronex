FROM rust:1-alpine AS chef
RUN apk add --no-cache musl-dev mold clang
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
    RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" \
    cargo chef cook --release -p veronex-mcp --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY workspace-hack/ ./workspace-hack/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" \
    cargo build --release -p veronex-mcp --bin veronex-mcp && \
    cp /app/target/release/veronex-mcp /app/veronex-mcp

FROM alpine:3.21
RUN apk add --no-cache ca-certificates wget
WORKDIR /app
COPY --from=builder /app/veronex-mcp ./
EXPOSE 3100
CMD ["./veronex-mcp"]
