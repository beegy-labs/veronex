FROM ghcr.io/actions/actions-runner:latest

USER root

# Build toolchain: clang, mold (fast linker), cmake, ssl, curl
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    libssl-dev \
    libcurl4-openssl-dev \
    clang \
    mold \
    cmake \
    && rm -rf /var/lib/apt/lists/*

# sccache — required because .env sets RUSTC_WRAPPER=sccache globally.
# Without the binary on PATH, `cargo nextest run` (and any cargo invocation
# in PR Validation) fails with "sccache rustc -vV never executed (os error 2)".
# Pre-bake into the runner image so jobs don't pay install latency.
ARG SCCACHE_VERSION=0.8.2
RUN curl -fsSL "https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
      | tar -xz -C /tmp && \
    mv "/tmp/sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl/sccache" /usr/local/bin/ && \
    chmod +x /usr/local/bin/sccache

# Rust stable (pre-baked so CI jobs skip rustup download)
USER runner
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH="/home/runner/.cargo/bin:${PATH}"

# cargo-nextest: faster parallel test runner (pre-baked to avoid per-job install)
RUN cargo install cargo-nextest --locked

# Verify toolchain
RUN rustc --version && cargo --version && cargo nextest --version
