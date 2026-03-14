FROM rust:1-alpine AS chef
RUN apk add --no-cache musl-dev protoc mold clang
RUN cargo install cargo-chef --locked
WORKDIR /app

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
    cargo chef cook --release -p veronex-analytics --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY workspace-hack/ ./workspace-hack/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" \
    cargo build --release -p veronex-analytics && \
    cp /app/target/release/veronex-analytics /app/veronex-analytics

FROM alpine:3.21
RUN apk add --no-cache ca-certificates
WORKDIR /app
COPY --from=builder /app/veronex-analytics ./
EXPOSE 3003
CMD ["./veronex-analytics"]
