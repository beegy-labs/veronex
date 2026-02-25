# Infrastructure — Deployment & Observability Pipeline

> SSOT | **Last Updated**: 2026-02-25

## Services

| 서비스 | 이미지 | 호스트 포트 | 역할 |
|--------|--------|------------|------|
| postgres | postgres:17-alpine | 5433 | 메인 DB (jobs, api_keys, backends, gpu_servers) |
| valkey | valkey/valkey:8-alpine | 6380 | 큐(BLPOP), rate limiting, busy_backends 캐시 |
| clickhouse | clickhouse-server:latest | 8123, 9000 | 분석 로그, OTel metrics/traces |
| redpanda | redpandadata/redpanda:v24.2.7 | 9092 | Kafka-호환 스트리밍 버퍼 |
| inferq | 로컬 빌드 | 3001→3000 | Rust API 서버 |
| inferq-web | 로컬 빌드 | 3002 | Next.js admin dashboard |
| otel-collector | docker/otel/Dockerfile | 4317, 4318, 13133 | 메트릭·트레이스 수집·라우팅 |

## 포트 정리

| 서비스 | 컨테이너 | docker-compose (host) | K8s ClusterIP |
|--------|---------|----------------------|---------------|
| postgres | 5432 | **5433** | 5432 |
| valkey | 6379 | **6380** | 6379 |
| clickhouse HTTP | 8123 | 8123 | 8123 |
| clickhouse native | 9000 | 9000 | 9000 |
| redpanda Kafka | 9092 | 9092 | 9092 |
| inferq API | 3000 | **3001** | 3000 |
| inferq-web | 3002 | 3002 | 3002 |
| OTel gRPC | 4317 | 4317 | 4317 |
| OTel HTTP | 4318 | 4318 | 4318 |
| OTel healthcheck | 13133 | 13133 | 13133 |

> 5432, 6379, 3000은 vergate/Gitea가 사용 중 → host 포트 충돌 방지

---

## Redpanda

```yaml
image: docker.redpanda.com/redpandadata/redpanda:v24.2.7
command:
  - redpanda start
  - --smp=1 --memory=512M --overprovisioned
  - --kafka-addr=PLAINTEXT://0.0.0.0:9092
  - --advertise-kafka-addr=PLAINTEXT://redpanda:9092
```

- JVM/ZooKeeper 없음, 단일 컨테이너
- `auto_create_topics_enabled: true` — 토픽 자동 생성
- Kafka 마이그레이션: `kafka.brokers` 주소만 변경, 코드 수정 없음

---

## OTel Collector

### Dockerfile (`docker/otel/Dockerfile`)

```dockerfile
# 공식 이미지는 distroless → wget 없음 → healthcheck 불가
# debian:12-slim 기반으로 래핑
FROM otel/opentelemetry-collector-contrib:latest AS otel
FROM debian:12-slim
RUN apt-get install -y wget ca-certificates
COPY --from=otel /otelcol-contrib /otelcol-contrib
```

### config.yaml 구조

```yaml
extensions:
  health_check:
    endpoint: 0.0.0.0:13133

receivers:
  prometheus:
    config:
      scrape_configs:
        - job_name: inferq-node-exporters
          scrape_interval: 30s
          http_sd_configs:
            - url: http://inferq:3000/v1/metrics/targets   # 동적 targets
              refresh_interval: 30s
  otlp:
    protocols:
      grpc: { endpoint: 0.0.0.0:4317 }
      http: { endpoint: 0.0.0.0:4318 }

processors:
  memory_limiter: { check_interval: 5s, limit_mib: 256 }
  batch: {}

exporters:
  clickhouse:   # 1차 쓰기 경로 (auto-schema: otel_metrics_*, otel_traces)
  kafka/metrics:
    brokers: [redpanda:9092]
    topic: otel-metrics
    encoding: otlp_json
  kafka/traces:
    brokers: [redpanda:9092]
    topic: otel-traces
    encoding: otlp_json

service:
  pipelines:
    metrics: receivers:[prometheus]  → processors:[memory_limiter,batch] → exporters:[clickhouse,kafka/metrics]
    traces:  receivers:[otlp]        → processors:[memory_limiter,batch] → exporters:[clickhouse,kafka/traces]
```

---

## ClickHouse Kafka Engine (Redpanda → ClickHouse)

```sql
-- Kafka Engine → MergeTree 패턴
kafka_otel_metrics ENGINE=Kafka(topic='otel-metrics', format=JSONAsString)
    → MV → otel_metrics_raw ENGINE=MergeTree() TTL created_at + INTERVAL 30 DAY

kafka_otel_traces  ENGINE=Kafka(topic='otel-traces',  format=JSONAsString)
    → MV → otel_traces_raw  ENGINE=MergeTree() TTL created_at + INTERVAL 30 DAY
```

현재 ClickHouse exporter가 1차 쓰기 경로.
미래: Kafka Engine만으로 전환 → `otel_metrics_raw`에서 `JSONExtract` 사용.

---

## GPU 서버 측 (docker-compose.ollama.yml)

각 Ollama GPU 서버에서 별도 실행:

```yaml
services:
  ollama:
    image: ollama/ollama
    ports: ["11434:11434"]

  node-exporter:
    image: prom/node-exporter:latest
    command:
      - --collector.drm      # AMD GPU VRAM/utilization
      - --collector.hwmon    # 온도, 전력
      - --collector.meminfo  # 시스템 RAM
    volumes:
      - /proc:/host/proc:ro
      - /sys:/host/sys:ro
    ports: ["9100:9100"]
```

등록: inferq admin → GPU Servers → Register → OTel 자동 반영 (30s)

## inferq 서비스 마운트

```yaml
volumes:
  - /sys/class/drm:/sys/class/drm:ro   # AMD GPU sysfs (Linux only)
```

macOS/Docker Desktop: 경로 없으면 debug 로그 1회 후 무시 (fail-open)

---

## Helm Chart (`helm/inferq/`)

```
helm/inferq/
├── Chart.yaml
├── values.yaml
└── templates/
    ├── inferq/           Deployment + Service
    ├── inferq-web/       Deployment + Service
    ├── postgres/         Deployment + Service + PVC
    ├── valkey/           Deployment + Service + PVC
    ├── clickhouse/       Deployment + Service + PVC
    ├── redpanda/         Deployment + Service
    └── otel-collector/   Deployment + Service + ConfigMap
```

### 배포 시나리오

```bash
# 기본 (모든 서비스 포함)
helm install inferq ./helm/inferq/

# 기존 Kafka 사용
helm install inferq ./helm/inferq/ \
  --set redpanda.enabled=false \
  --set redpanda.externalBrokers=kafka-broker:9092

# 기존 OTel Collector 사용 (NOTES.txt에 HTTP SD 스니펫 출력)
helm install inferq ./helm/inferq/ --set otelCollector.enabled=false

# 기존 DB 인프라 사용
helm install inferq ./helm/inferq/ \
  --set postgres.enabled=false --set valkey.enabled=false --set clickhouse.enabled=false \
  --set inferq.env.databaseUrl="postgres://..." \
  --set inferq.env.valkeyUrl="redis://..." \
  --set inferq.env.clickhouseUrl="http://..."
```

### otelCollector.enabled=false 시 기존 Collector 설정 스니펫

```yaml
receivers:
  prometheus:
    config:
      scrape_configs:
        - job_name: inferq-node-exporters
          scrape_interval: 30s
          http_sd_configs:
            - url: http://<release>.<namespace>.svc.cluster.local:3000/v1/metrics/targets
              refresh_interval: 30s
```

---

## 환경 변수

```
DATABASE_URL     = postgres://inferq:inferq@localhost:5433/inferq
VALKEY_URL       = redis://localhost:6380
CLICKHOUSE_URL   = http://localhost:8123
CLICKHOUSE_USER  = inferq
CLICKHOUSE_PASSWORD = inferq
CLICKHOUSE_DB    = inferq
CLICKHOUSE_ENABLED = true
OLLAMA_URL       = http://localhost:11434
GEMINI_API_KEY   = <optional>
BOOTSTRAP_API_KEY = inferq-bootstrap-admin-key
PORT             = 3000 (컨테이너 내부)
OTEL_EXPORTER_OTLP_ENDPOINT = http://otel-collector:4317 (optional)
```
