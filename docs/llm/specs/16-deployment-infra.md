# Spec 16 — Deployment Infrastructure

> SSOT | **Status**: 구현 완료 | **Last Updated**: 2026-02-25

## Goal

inferq 전체 스택(Rust 서비스, Web UI, 데이터 레이어, 메트릭 파이프라인)을
docker-compose(개발/단일 서버)와 Helm(K8s 프로덕션) 두 환경에서 일관되게 배포한다.

---

## 서비스 구성

| 서비스 | 이미지 | 포트(host) | 역할 |
|--------|--------|-----------|------|
| postgres | postgres:17-alpine | 5433 | 메인 DB (jobs, api_keys, backends) |
| valkey | valkey/valkey:8-alpine | 6380 | 큐(BLPOP), rate limiting, busy_backends |
| clickhouse | clickhouse/clickhouse-server:latest | 8123, 9000 | 분석 로그, OTel metrics/traces |
| **redpanda** | redpandadata/redpanda:v24.2.7 | 9092 | Kafka-호환 스트리밍 버퍼 |
| inferq | inferq (로컬 빌드) | 3001→3000 | Rust API 서버 |
| inferq-web | inferq-web (로컬 빌드) | 3002 | Next.js 대시보드 |
| **otel-collector** | docker/otel/Dockerfile | 4317, 4318, 13133 | 메트릭/트레이스 수집·라우팅 |

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
- `auto_create_topics_enabled` 기본값 `true` — 토픽 자동 생성
- **Kafka 마이그레이션**: `kafka/metrics.brokers` 주소만 변경, 코드 수정 없음
- 목표: GPU 서버 50+ 대 규모에서 OTel → ClickHouse 백프레셔 방지

---

## OTel Collector 파이프라인

### docker/otel/Dockerfile

```dockerfile
# 공식 이미지는 distroless — wget/sh 없음.
# healthcheck를 위해 debian:12-slim 기반으로 래핑.
FROM otel/opentelemetry-collector-contrib:latest AS otel
FROM debian:12-slim
RUN apt-get update && apt-get install -y --no-install-recommends wget ca-certificates ...
COPY --from=otel /otelcol-contrib /otelcol-contrib
```

### docker/otel/config.yaml 구조

```
extensions:
  health_check: 0.0.0.0:13133      ← docker healthcheck용

receivers:
  prometheus:
    http_sd_configs: inferq:3000/v1/metrics/targets   ← 동적 node-exporter targets
  otlp: grpc:4317, http:4318       ← inferq 애플리케이션 트레이스

processors:
  memory_limiter, batch

exporters:
  clickhouse  → otel_metrics_*, otel_traces 자동 스키마 (1차 쓰기 경로)
  kafka/metrics → redpanda:9092 topic=otel-metrics (otlp_json)
  kafka/traces  → redpanda:9092 topic=otel-traces  (otlp_json)

pipelines:
  metrics: [prometheus] → [memory_limiter,batch] → [clickhouse, kafka/metrics]
  traces:  [otlp]       → [memory_limiter,batch] → [clickhouse, kafka/traces]
```

### ClickHouse Kafka Engine (Redpanda 소비)

```sql
-- Kafka Engine → MergeTree 패턴
kafka_otel_metrics  ENGINE=Kafka (format=JSONAsString)
    → MV → otel_metrics_raw ENGINE=MergeTree (30일 TTL)

kafka_otel_traces   ENGINE=Kafka (format=JSONAsString)
    → MV → otel_traces_raw  ENGINE=MergeTree (30일 TTL)
```

- 현재: ClickHouse exporter가 1차 쓰기 경로 (otel_metrics_* 자동 스키마)
- 미래: ClickHouse exporter 제거 후 Kafka Engine만 사용 → otel_metrics_raw에서 JSONExtract

---

## docker-compose 배포

### 서비스 의존성 그래프

```
postgres ──┐
valkey   ──┼─→ inferq ──┐
clickhouse─┘             ├─→ inferq-web
                         │
redpanda ──┐             │
clickhouse─┼─→ otel-collector
inferq ────┘
```

### GPU 서버 측 (docker-compose.ollama.yml)

각 Ollama GPU 서버에서 별도로 실행:
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

등록 방법: inferq admin → GPU Servers → Register GPU Server → OTel 자동 반영

### inferq 서비스 마운트

```yaml
volumes:
  - /sys/class/drm:/sys/class/drm:ro   # AMD GPU sysfs (Linux only)
```

macOS/Docker Desktop: 경로 없으면 디버그 로그 1회 출력 후 무시 (fail-open)

---

## Helm 차트 (`helm/inferq/`)

### 구조

```
helm/inferq/
├── Chart.yaml
├── values.yaml
└── templates/
    ├── _helpers.tpl
    ├── NOTES.txt
    ├── inferq/           Deployment + Service
    ├── inferq-web/       Deployment + Service
    ├── postgres/         Deployment + Service + PVC  (enabled: true)
    ├── valkey/           Deployment + Service + PVC  (enabled: true)
    ├── clickhouse/       Deployment + Service + PVC  (enabled: true)
    ├── redpanda/         Deployment + Service        (enabled: true)
    └── otel-collector/   Deployment + Service + ConfigMap (enabled: true)
```

### 주요 values

```yaml
redpanda:
  enabled: true           # false = 외부 Kafka 사용
  externalBrokers: ""     # enabled=false 시 "broker1:9092,broker2:9092"

otelCollector:
  enabled: true           # false = 기존 클러스터 OTel Collector 사용

postgres:
  enabled: true           # false = 외부 DB (inferq.env.databaseUrl 직접 지정)

inferq:
  env:
    bootstrapApiKey: inferq-bootstrap-admin-key
    ollamaUrl: http://host.docker.internal:11434
```

### 배포 시나리오

```bash
# 1. 기본 (모든 서비스 포함)
helm install inferq ./helm/inferq/

# 2. 기존 OTel Collector 있음 (otelCollector.enabled=false)
#    → NOTES.txt에 HTTP SD 설정 스니펫 자동 출력
helm install inferq ./helm/inferq/ \
  --set otelCollector.enabled=false

# 3. 기존 Kafka 클러스터 있음
helm install inferq ./helm/inferq/ \
  --set redpanda.enabled=false \
  --set redpanda.externalBrokers=kafka-broker:9092

# 4. 기존 DB 인프라 있음
helm install inferq ./helm/inferq/ \
  --set postgres.enabled=false \
  --set valkey.enabled=false \
  --set clickhouse.enabled=false \
  --set inferq.env.databaseUrl="postgres://..." \
  --set inferq.env.valkeyUrl="redis://..." \
  --set inferq.env.clickhouseUrl="http://..."
```

### otelCollector.enabled=false 시 기존 Collector 설정

NOTES.txt가 자동으로 아래 스니펫 출력:

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

- `/v1/metrics/targets`는 표준 Prometheus HTTP SD → Prometheus, Grafana Agent 등 모두 호환
- inferq admin에서 node-exporter URL 추가/삭제 → 30s 내 자동 반영

---

## 포트 정리

| 서비스 | 컨테이너 | 호스트(docker-compose) | K8s ClusterIP |
|--------|---------|----------------------|---------------|
| postgres | 5432 | 5433 | 5432 |
| valkey | 6379 | 6380 | 6379 |
| clickhouse HTTP | 8123 | 8123 | 8123 |
| clickhouse native | 9000 | 9000 | 9000 |
| redpanda Kafka | 9092 | 9092 | 9092 |
| inferq API | 3000 | 3001 | 3000 |
| inferq-web | 3002 | 3002 | 3002 |
| OTel gRPC | 4317 | 4317 | 4317 |
| OTel HTTP | 4318 | 4318 | 4318 |
| OTel healthcheck | 13133 | 13133 | 13133 |

---

## 구현 체크리스트

- [x] docker-compose.yml — 7개 서비스 전체 구성
- [x] docker-compose.ollama.yml — Ollama + node-exporter 번들 (GPU 서버용)
- [x] Redpanda v24.2.7 서비스 추가
- [x] OTel Collector custom Dockerfile (debian-slim 래핑, healthcheck 지원)
- [x] OTel config: health_check extension + kafka fan-out exporters
- [x] ClickHouse init.sql: Kafka Engine consumer tables + Materialized Views
- [x] Helm chart `helm/inferq/` — 22개 템플릿, `helm lint` 통과
- [x] Helm: 조건부 서비스 (postgres/valkey/clickhouse/redpanda/otelCollector)
- [x] Helm: `_helpers.tpl` 서비스 URL 자동 계산
- [x] Helm: NOTES.txt — otelCollector.enabled=false 시 HTTP SD 스니펫 출력
