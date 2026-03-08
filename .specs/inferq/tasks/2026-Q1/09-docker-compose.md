# Task 09: docker-compose

> Open-source deployment. Works out of box. OTel Collector included in base for testing.
> Port layout: inferq-web=3000, grafana=3001, jaeger=16686

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
      # 백엔드는 API로 등록 (POST /v1/backends) — 환경변수는 시작 시 자동 등록용
      - INFERQ_BOOTSTRAP_BACKENDS=${INFERQ_BOOTSTRAP_BACKENDS:-}
      - OBSERVABILITY_BACKEND=${OBSERVABILITY_BACKEND:-otel}
      - OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317
      - CLICKHOUSE_HOST=${CLICKHOUSE_HOST:-}
    depends_on:
      postgres: {condition: service_healthy}
      valkey:   {condition: service_started}
      otel-collector: {condition: service_started}

  inferq-worker:
    build: .
    command: arq src.infrastructure.outbound.queue.worker.WorkerSettings
    environment:
      - DATABASE_URL=postgresql+asyncpg://inferq:inferq@postgres:5432/inferq
      - VALKEY_URL=redis://valkey:6379
      - OBSERVABILITY_BACKEND=${OBSERVABILITY_BACKEND:-otel}
      - OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317
    depends_on:
      postgres: {condition: service_healthy}
      valkey:   {condition: service_started}

  inferq-web:
    build: ./web
    ports: ["3000:3000"]                 # web dashboard
    environment:
      - INFERQ_API_URL=http://inferq:8000
      - INFERQ_ADMIN_KEY=${INFERQ_ADMIN_KEY}
    depends_on: [inferq]

  postgres:
    image: postgres:17-alpine
    environment:
      POSTGRES_DB: inferq
      POSTGRES_USER: inferq
      POSTGRES_PASSWORD: inferq
    volumes: [postgres_data:/var/lib/postgresql/data]
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U inferq"]
      interval: 5s
      timeout: 5s
      retries: 5

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
    ports: ["3001:3000"]             # 3000은 inferq-web 사용

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

- [ ] `config/otel-collector.yaml`:

```yaml
receivers:
  otlp:
    protocols:
      grpc: {endpoint: "0.0.0.0:4317"}
      http: {endpoint: "0.0.0.0:4318"}

processors:
  batch:
    timeout: 5s

exporters:
  debug:                           # 항상 활성 — 로컬 테스트용 stdout 출력
    verbosity: normal
  otlp/jaeger:                     # monitoring profile에서 jaeger로 전달
    endpoint: jaeger:4317
    tls: {insecure: true}

service:
  pipelines:
    traces:
      receivers: [otlp]
      processors: [batch]
      exporters: [debug, otlp/jaeger]
    metrics:
      receivers: [otlp]
      processors: [batch]
      exporters: [debug]
    logs:
      receivers: [otlp]
      processors: [batch]
      exporters: [debug]
```

> Jaeger가 없을 때 `otlp/jaeger` exporter가 실패하면 에러 발생.
> monitoring profile 없이 쓸 때는 exporters에서 `otlp/jaeger` 제거 또는 조건 분기 필요.
> → 해결: `otel-collector.yaml` (base) + `otel-collector.monitoring.yaml` (monitoring profile 오버라이드) 2개 파일로 관리.

### Phase 5 — README Usage

- [ ] Document:

```bash
# 1. 기본 시작 (OTel 포함)
docker compose up

# 2. 백엔드 등록 (배포 환경 무관 — URL만 있으면 됨)

# Ollama (로컬 GPU)
curl -X POST http://localhost:8000/v1/backends \
  -H "Content-Type: application/json" \
  -d '{"id": "gpu-01", "name": "Local GPU", "backend_type": "ollama",
       "url": "http://host.docker.internal:11434", "total_vram_mb": 98304}'

# Gemini API
curl -X POST http://localhost:8000/v1/backends \
  -H "Content-Type: application/json" \
  -d '{"id": "gemini-main", "name": "Gemini", "backend_type": "gemini",
       "api_key": "AIza..."}'

# 시작 시 자동 등록 (bootstrap)
INFERQ_BOOTSTRAP_BACKENDS=ollama:gpu-01:http://host.docker.internal:11434 docker compose up

# 3. 웹 대시보드
open http://localhost:3000

# 4. With monitoring (Prometheus + Grafana:3001 + Jaeger)
docker compose --profile monitoring up

# 5. With analytics (ClickHouse)
docker compose --profile analytics up
```

## Done

- [ ] `docker compose up` → inferq + web dashboard + OTel Collector 기본 동작
- [ ] `OBSERVABILITY_BACKEND=otel` 기본값 — OTel Collector로 traces/metrics/logs 전송
- [ ] `inferq-web` (port 3000), `grafana` (port 3001) — 포트 충돌 없음
- [ ] `postgres` healthcheck → `inferq` 정상 기동 순서 보장
- [ ] `--profile monitoring` → Prometheus + Grafana(3001) + Jaeger
- [ ] `--profile analytics` → ClickHouse
- [ ] `otel-collector.yaml` (base: debug only) / `otel-collector.monitoring.yaml` (+ jaeger)
- [ ] `INFERQ_BOOTSTRAP_BACKENDS` 환경변수 지원
- [ ] `.env.example` 전체 환경변수 문서화
