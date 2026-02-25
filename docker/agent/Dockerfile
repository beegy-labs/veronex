FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY crates/inferq-agent/Cargo.toml crates/inferq-agent/Cargo.lock* ./crates/inferq-agent/
COPY crates/inferq-agent/src ./crates/inferq-agent/src
RUN cargo build --release --manifest-path crates/inferq-agent/Cargo.toml

FROM alpine:3.21
RUN apk add --no-cache ca-certificates
WORKDIR /app
COPY --from=builder /app/crates/inferq-agent/target/release/inferq-agent ./
EXPOSE 9091
CMD ["./inferq-agent"]
