# Task 01: Project Structure

> Based on: hexagonal architecture policy (`docs/llm/policies/architecture.md`)

## Steps

### Phase 1 — Scaffold

- [ ] Create `pyproject.toml` (Python 3.13, uv or pip)
- [ ] Create directory tree:

```
src/
├── domain/
│   ├── entities/
│   ├── value_objects/
│   └── exceptions/
├── application/
│   ├── ports/
│   │   ├── inbound/
│   │   └── outbound/
│   └── use_cases/
├── infrastructure/
│   ├── inbound/
│   │   ├── http/
│   │   └── sse/
│   └── outbound/
│       ├── queue/
│       ├── gpu/
│       ├── persistence/
│       └── observability/
└── main.py
```

- [ ] Create `Dockerfile` (python:3.13-slim)
- [ ] Create `.env.example`

## Dependencies (pyproject.toml)

```toml
[project]
dependencies = [
  "fastapi>=0.115",
  "uvicorn[standard]>=0.34",
  "arq>=0.26",
  "sse-starlette>=2.0",
  "sqlalchemy[asyncio]>=2.0",
  "asyncpg>=0.30",
  "alembic>=1.14",
  "alembic-postgresql-enum>=1.3",
  "httpx>=0.28",
  "pydantic-settings>=2.7",
  "opentelemetry-sdk>=1.30",
  "opentelemetry-instrumentation-fastapi>=0.51",
  "opentelemetry-exporter-otlp-proto-grpc>=1.30",
  "clickhouse-connect>=0.8",
]
```

## Verify

```bash
uv sync && python -c "import fastapi, arq, sqlalchemy"
```

## Done

- [ ] Directory tree matches architecture policy
- [ ] pyproject.toml installs cleanly
- [ ] Dockerfile builds
