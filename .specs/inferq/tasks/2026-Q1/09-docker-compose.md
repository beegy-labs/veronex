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
      # OLLAMA_URL 없음 — GPU 서버는 API로 등록 (POST /v1/servers)
      # 선택: 시작 시 자동 등록할 서버 목록 (쉼표 구분)
      - INFERQ_BOOTSTRAP_SERVERS=${INFERQ_BOOTSTRAP_SERVERS:-}
      - OBSERVABILITY_BACKEND=${OBSERVABILITY_BACKEND:-stdout}
      - OTEL_EXPORTER_OTLP_ENDPOINT=${OTEL_EXPORTER_OTLP_ENDPOINT:-}
      - CLICKHOUSE_HOST=${CLICKHOUSE_HOST:-}
    depends_on: [postgres, valkey]

  inferq-worker:
    build: .
    command: arq src.infrastructure.outbound.queue.worker.WorkerSettings
    environment:
      - DATABASE_URL=postgresql+asyncpg://inferq:inferq@postgres:5432/inferq
      - VALKEY_URL=redis://valkey:6379
      # GPU 서버 정보는 PostgreSQL에서 읽음 — 환경변수 불필요
    depends_on: [postgres, valkey]

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
# 1. 시작
docker compose up

# 2. GPU 서버 등록 (배포 환경 무관 — URL만 있으면 됨)
curl -X POST http://localhost:8000/v1/servers \
  -H "Content-Type: application/json" \
  -d '{"id": "gpu-01", "url": "http://host.docker.internal:11434", "total_vram_mb": 98304}'

# 또는 시작 시 자동 등록 (bootstrap)
INFERQ_BOOTSTRAP_SERVERS=http://host.docker.internal:11434 docker compose up

# k8s Ollama 연결
INFERQ_BOOTSTRAP_SERVERS=http://ollama-service.gpu-ns.svc.cluster.local:11434 \
docker compose up inferq inferq-worker

# With monitoring
docker compose --profile monitoring up

# With analytics
docker compose --profile analytics up
```

## Done

- [ ] `docker compose up` starts inferq fully functional
- [ ] `--profile monitoring` adds Prometheus + Grafana + Jaeger
- [ ] `--profile analytics` adds ClickHouse
- [ ] `.env.example` documents all env vars
- [ ] README documents all startup options
