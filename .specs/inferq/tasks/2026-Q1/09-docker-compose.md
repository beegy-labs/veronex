# Task 09: docker-compose

> Open-source deployment. Works out of box. Optional profiles for monitoring/analytics.

## Steps

### Phase 1 — Base (required services)

- [ ] `docker-compose.yml`:

```yaml
services:
  inferq:
    build: .
    ports: ["8000:8000"]
    environment:
      - DATABASE_URL=postgresql+asyncpg://inferq:inferq@postgres:5432/inferq
      - VALKEY_URL=redis://valkey:6379
      - OLLAMA_URL=http://host.docker.internal:11434  # Ollama on host or k8s
      - OBSERVABILITY_BACKEND=${OBSERVABILITY_BACKEND:-stdout}
      - OTEL_EXPORTER_OTLP_ENDPOINT=${OTEL_EXPORTER_OTLP_ENDPOINT:-}
      - CLICKHOUSE_HOST=${CLICKHOUSE_HOST:-}
    depends_on: [postgres, valkey]

  inferq-worker:
    build: .
    command: arq src.infrastructure.outbound.queue.worker.WorkerSettings
    environment:
      - VALKEY_URL=redis://valkey:6379
      - OLLAMA_URL=http://host.docker.internal:11434
    depends_on: [valkey]

  postgres:
    image: postgres:17-alpine
    environment:
      POSTGRES_DB: inferq
      POSTGRES_USER: inferq
      POSTGRES_PASSWORD: inferq
    volumes: [postgres_data:/var/lib/postgresql/data]

  valkey:
    image: valkey/valkey:8-alpine
    volumes: [valkey_data:/data]

  otel-collector:
    image: otel/opentelemetry-collector-contrib:latest
    volumes: [./config/otel-collector.yaml:/etc/otel/config.yaml]
    command: ["--config=/etc/otel/config.yaml"]
    ports: ["4317:4317", "4318:4318"]

volumes:
  postgres_data:
  valkey_data:
```

### Phase 2 — Monitoring Profile

- [ ] Add to `docker-compose.yml`:

```yaml
  prometheus:
    image: prom/prometheus:latest
    profiles: [monitoring]
    volumes: [./config/prometheus.yml:/etc/prometheus/prometheus.yml]

  grafana:
    image: grafana/grafana:latest
    profiles: [monitoring]
    ports: ["3000:3000"]

  jaeger:
    image: jaegertracing/all-in-one:latest
    profiles: [monitoring]
    ports: ["16686:16686"]
```

### Phase 3 — Analytics Profile

- [ ] Add to `docker-compose.yml`:

```yaml
  clickhouse:
    image: clickhouse/clickhouse-server:latest
    profiles: [analytics]
    ports: ["8123:8123", "9000:9000"]
    volumes: [clickhouse_data:/var/lib/clickhouse]
```

### Phase 4 — OTel Collector Config

- [ ] `config/otel-collector.yaml`: route to stdout (default), jaeger (monitoring profile)

### Phase 5 — README Usage

- [ ] Document:

```bash
# Minimal
docker compose up

# With monitoring
docker compose --profile monitoring up

# With analytics
docker compose --profile analytics up

# k8s environment (point to existing infra)
OBSERVABILITY_BACKEND=otel \
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317 \
docker compose up inferq inferq-worker
```

## Done

- [ ] `docker compose up` starts inferq fully functional
- [ ] `--profile monitoring` adds Prometheus + Grafana + Jaeger
- [ ] `--profile analytics` adds ClickHouse
- [ ] `.env.example` documents all env vars
- [ ] README documents all startup options
