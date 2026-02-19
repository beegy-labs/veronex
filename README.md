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

```
develop → main
```

| Branch | Purpose |
|--------|---------|
| `develop` | Active development |
| `main` | Stable production releases |

> **`release` branch** will be introduced when:
> - A separate staging environment is available
> - Multiple contributors require a QA freeze period
> - Release candidates need independent validation before merging to `main`

## License

MIT
