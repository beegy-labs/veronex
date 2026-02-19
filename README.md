# inferq

Queue-based LLM inference server with real-time SSE streaming.

## Overview

`inferq` is an open-source, shared LLM inference serving system designed for single-GPU environments.
It queues incoming requests and processes them sequentially, streaming results in real-time via SSE (Server-Sent Events).

## Features

- Queue-based request management (single GPU safe)
- Real-time token streaming via SSE
- Easy integration with any project via HTTP

## Branch Strategy

| Branch | Purpose |
|--------|---------|
| `main` | Stable production releases |
| `release` | Release candidates / staging |
| `develop` | Active development |

## License

MIT
