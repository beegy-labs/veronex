FROM rust:1-slim AS chef
RUN apt-get update && apt-get install -y pkg-config libssl-dev g++ && rm -rf /var/lib/apt/lists/*
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
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release -p veronex-embed --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY workspace-hack/ ./workspace-hack/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release -p veronex-embed --bin veronex-embed && \
    cp /app/target/release/veronex-embed /app/veronex-embed

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 wget && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/veronex-embed ./

# Pre-bake the embedding model into the image at build time so the container
# starts immediately without downloading from HuggingFace at runtime.
ENV FASTEMBED_CACHE_PATH=/app/models
RUN PRELOAD_ONLY=1 ./veronex-embed

EXPOSE 3200
CMD ["./veronex-embed"]
