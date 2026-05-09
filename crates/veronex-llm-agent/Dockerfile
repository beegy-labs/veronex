# syntax=docker/dockerfile:1.7
#
# veronex-llm-agent — k8s DaemonSet image for AMD Strix Halo nodes.
#
# Bakes a pinned `llama-server` build (Vulkan backend) into the image so
# operators never pull external `llama.cpp` images and the agent + LLM
# runtime ship as a single Veronex artifact. The Mac variant lives in a
# homebrew formula (separate repo) that builds the same upstream tag
# with the Metal backend.
#
# Build args:
#   LLAMA_CPP_TAG     — upstream ggml-org/llama.cpp tag to build.
#                       Pinned per Veronex release; bump intentionally.
#   TARGETARCH        — Docker auto-injects amd64 / arm64. Vulkan path
#                       is amd64-only for now (Strix Halo).
#
# Runtime layout:
#   /opt/veronex-llm-agent/bin/veronex-llm-agent  — Rust binary
#   /opt/veronex-llm-agent/bin/llama-server       — built from source
#   /var/lib/veronex-llm-agent/                   — blob cache (PV mount)

ARG LLAMA_CPP_TAG=b5500

# ── Stage 1 — build llama.cpp with Vulkan ─────────────────────────────
FROM ubuntu:24.04 AS llama-builder
ARG LLAMA_CPP_TAG
ARG DEBIAN_FRONTEND=noninteractive

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
        git ca-certificates cmake ninja-build pkg-config \
        build-essential clang \
        libvulkan-dev vulkan-tools \
        glslc spirv-tools \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
RUN git clone --depth 1 --branch "${LLAMA_CPP_TAG}" \
        https://github.com/ggml-org/llama.cpp.git .

# Vulkan backend, server binary only. -DLLAMA_BUILD_TESTS=OFF and
# -DLLAMA_BUILD_EXAMPLES=OFF cut build time roughly in half. The server
# is technically an example; -DLLAMA_BUILD_SERVER=ON re-enables it.
RUN cmake -B build -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DGGML_VULKAN=ON \
        -DLLAMA_BUILD_TESTS=OFF \
        -DLLAMA_BUILD_EXAMPLES=OFF \
        -DLLAMA_BUILD_SERVER=ON \
    && cmake --build build --target llama-server -j

# Verify the binary exists where we expect; cmake places it under
# build/bin/ on this layout. Failure here is a hard build failure.
RUN test -x build/bin/llama-server && build/bin/llama-server --version

# ── Stage 2 — build the agent (Rust) via cargo-chef ────────────────────
FROM rust:1-bookworm AS rust-chef
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
        clang mold pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM rust-chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY workspace-hack/ ./workspace-hack/
RUN cargo chef prepare --recipe-path recipe.json

FROM rust-chef AS rust-builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" \
    cargo chef cook --release -p veronex-llm-agent --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY workspace-hack/ ./workspace-hack/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" \
    cargo build --release -p veronex-llm-agent && \
    cp /app/target/release/veronex-llm-agent /app/veronex-llm-agent

# ── Stage 3 — runtime image (Vulkan ICD + ours-built llama-server) ────
FROM ubuntu:24.04 AS runtime
ARG LLAMA_CPP_TAG
ARG DEBIAN_FRONTEND=noninteractive

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        libvulkan1 \
        mesa-vulkan-drivers \
        vulkan-tools \
    && rm -rf /var/lib/apt/lists/*

# Layout the Phase 3 SDD mandates so admin docs match what's on disk.
WORKDIR /opt/veronex-llm-agent/bin

# Copy the agent + the llama-server we built from the pinned tag. We
# also copy the shared ggml backend libs so the Vulkan backend dlopens
# resolve at runtime (cmake puts them next to the binary on Linux).
COPY --from=llama-builder /src/build/bin/llama-server  ./llama-server
COPY --from=llama-builder /src/build/bin/lib*.so       ./
COPY --from=rust-builder  /app/veronex-llm-agent       ./veronex-llm-agent

# Embed the upstream tag in a label + env var so operators can confirm
# which llama.cpp the running pod actually shipped with — no guessing.
LABEL ai.veronex.llama_cpp_tag="${LLAMA_CPP_TAG}"
ENV LLAMA_CPP_TAG="${LLAMA_CPP_TAG}" \
    LD_LIBRARY_PATH="/opt/veronex-llm-agent/bin" \
    VERONEX_AGENT_LLAMA_BIN="/opt/veronex-llm-agent/bin/llama-server" \
    VERONEX_AGENT_BLOB_DIR="/var/lib/veronex-llm-agent" \
    VERONEX_AGENT_BIND="0.0.0.0:9100" \
    RUST_LOG="info"

# /var/lib must be writable for blob cache. k8s mounts a hostPath or
# local-path PVC here; create the dir for non-PV smoke tests too.
RUN mkdir -p /var/lib/veronex-llm-agent

EXPOSE 9100
HEALTHCHECK --interval=15s --timeout=3s --start-period=10s --retries=3 \
    CMD wget -qO- http://localhost:9100/healthz >/dev/null 2>&1 || exit 1

CMD ["/opt/veronex-llm-agent/bin/veronex-llm-agent"]
