FROM rust:1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY workspace-hack/ ./workspace-hack/
RUN cargo build --release -p veronex-builder-runner

FROM node:24-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates openssh-client \
    && rm -rf /var/lib/apt/lists/*
RUN npm install -g @openai/codex@0.157.1 @anthropic-ai/claude-code@2.1.283 @google/gemini-cli@0.61.0
COPY --from=builder /app/target/release/veronex-builder-runner /usr/local/bin/veronex-builder-runner
USER 1000:1000
ENTRYPOINT ["veronex-builder-runner"]
