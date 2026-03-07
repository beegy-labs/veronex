FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

# Copy workspace root manifests
COPY Cargo.toml Cargo.lock ./

# Copy all workspace member Cargo.toml files
COPY crates/veronex/Cargo.toml ./crates/veronex/
COPY crates/veronex-agent/Cargo.toml ./crates/veronex-agent/
COPY crates/veronex-analytics/Cargo.toml ./crates/veronex-analytics/

# Dummy source for dependency caching
RUN mkdir -p crates/veronex/src crates/veronex-agent/src crates/veronex-analytics/src && \
    echo "fn main() {}" > crates/veronex/src/main.rs && \
    echo "fn main() {}" > crates/veronex-agent/src/main.rs && \
    echo "fn main() {}" > crates/veronex-analytics/src/main.rs
RUN SQLX_OFFLINE=true cargo build --release -p veronex 2>/dev/null || true

# Real source
COPY crates/veronex/src ./crates/veronex/src
COPY crates/veronex/migrations ./crates/veronex/migrations
COPY crates/veronex/.sqlx ./crates/veronex/.sqlx
RUN touch crates/veronex/src/main.rs && \
    SQLX_OFFLINE=true cargo build --release -p veronex

FROM alpine:3.21

RUN apk add --no-cache ca-certificates wget

WORKDIR /app
COPY --from=builder /app/target/release/veronex ./

EXPOSE 3000
CMD ["./veronex"]
