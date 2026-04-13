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

# Rust stable (pre-baked so CI jobs skip rustup download)
USER runner
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH="/home/runner/.cargo/bin:${PATH}"

# cargo-nextest: faster parallel test runner (pre-baked to avoid per-job install)
RUN cargo install cargo-nextest --locked

# Verify toolchain
RUN rustc --version && cargo --version && cargo nextest --version
