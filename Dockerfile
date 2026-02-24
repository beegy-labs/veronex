FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

COPY crates/inferq/Cargo.toml crates/inferq/Cargo.lock* ./crates/inferq/
COPY crates/inferq/src ./crates/inferq/src
COPY crates/inferq/migrations ./crates/inferq/migrations

RUN cargo build --release --manifest-path crates/inferq/Cargo.toml

FROM alpine:3.21

RUN apk add --no-cache ca-certificates

WORKDIR /app
COPY --from=builder /app/crates/inferq/target/release/inferq ./

EXPOSE 3000
CMD ["./inferq"]
