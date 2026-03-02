FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

COPY crates/veronex/Cargo.toml crates/veronex/Cargo.lock* ./crates/veronex/
COPY crates/veronex/src ./crates/veronex/src
COPY crates/veronex/migrations ./crates/veronex/migrations
COPY crates/veronex/.sqlx ./crates/veronex/.sqlx

RUN SQLX_OFFLINE=true cargo build --release --manifest-path crates/veronex/Cargo.toml

FROM alpine:3.21

RUN apk add --no-cache ca-certificates

WORKDIR /app
COPY --from=builder /app/crates/veronex/target/release/veronex ./

EXPOSE 3000
CMD ["./veronex"]
