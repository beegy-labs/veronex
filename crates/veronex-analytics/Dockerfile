FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev protoc

WORKDIR /app

# Copy workspace root manifests (Cargo.lock lives here)
COPY Cargo.toml Cargo.lock ./

# Copy all workspace member Cargo.toml files (needed to resolve workspace)
COPY crates/inferq/Cargo.toml ./crates/inferq/
COPY crates/inferq-agent/Cargo.toml ./crates/inferq-agent/
COPY crates/veronex-analytics/Cargo.toml ./crates/veronex-analytics/

# Dummy source for all workspace members — caches dependency compilation
RUN mkdir -p crates/inferq/src crates/inferq-agent/src crates/veronex-analytics/src && \
    echo "fn main() {}" > crates/inferq/src/main.rs && \
    echo "fn main() {}" > crates/inferq-agent/src/main.rs && \
    echo "fn main() {}" > crates/veronex-analytics/src/main.rs
RUN cargo build --release -p veronex-analytics 2>/dev/null || true

# Real source
COPY crates/veronex-analytics/src ./crates/veronex-analytics/src
RUN touch crates/veronex-analytics/src/main.rs && \
    cargo build --release -p veronex-analytics

FROM alpine:3.21

RUN apk add --no-cache ca-certificates

WORKDIR /app
COPY --from=builder /app/target/release/veronex-analytics ./

EXPOSE 3003
CMD ["./veronex-analytics"]
