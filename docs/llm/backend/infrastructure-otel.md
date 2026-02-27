# Infrastructure — OTel Pipeline, Redpanda & ClickHouse

> SSOT | **Last Updated**: 2026-02-27

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change scrape interval | `docker/otel/config.yaml` | `scrape_interval:` under prometheus receiver |
| Add new OTel exporter | `docker/otel/config.yaml` | Add to `exporters:` block + add to relevant `service.pipelines` |
| Change Kafka topic name | `docker/otel/config.yaml` kafka exporter `topic:` | Also update ClickHouse Kafka Engine CREATE TABLE |
| Add new ClickHouse metrics table | ClickHouse init SQL + Kafka Engine + MV | `kafka_* ENGINE=Kafka` → `MV` → `otel_*_raw ENGINE=MergeTree` pattern |
| Change HTTP SD endpoint auth | `infrastructure/inbound/http/gpu_server_handlers.rs` `metrics_targets()` + `router.rs` | Move route outside/inside auth middleware layer |
| Register new GPU server (ops) | `POST /v1/servers` → OTel auto-detects via HTTP SD | No code change — OTel polls targets every 30s after registration |

## Key Files

| File | Purpose |
|------|---------|
| `docker/otel/Dockerfile` | OTel Collector image (debian-wrapped distroless) |
| `docker/otel/config.yaml` | Receiver + exporter + pipeline config |
| `docker-compose.yml` | `otel-collector` + `redpanda` + `clickhouse` services |
| `crates/inferq/src/infrastructure/inbound/http/gpu_server_handlers.rs` | `GET /v1/metrics/targets` (HTTP SD) |

---

## OTel Collector Dockerfile

```dockerfile
# docker/otel/Dockerfile
# Official image is distroless → no wget → healthcheck impossible
FROM otel/opentelemetry-collector-contrib:latest AS otel
FROM debian:12-slim
RUN apt-get install -y wget ca-certificates
COPY --from=otel /otelcol-contrib /otelcol-contrib
```

---

## OTel Collector Config (docker/otel/config.yaml)

```yaml
extensions:
  health_check:
    endpoint: 0.0.0.0:13133

receivers:
  prometheus:
    config:
      scrape_configs:
        - job_name: veronex-node-exporters
          scrape_interval: 30s
          http_sd_configs:
            - url: http://veronex:3000/v1/metrics/targets
              refresh_interval: 30s
  otlp:
    protocols:
      grpc: { endpoint: 0.0.0.0:4317 }
      http: { endpoint: 0.0.0.0:4318 }

processors:
  memory_limiter: { check_interval: 5s, limit_mib: 256 }
  batch: {}

exporters:
  clickhouse:          # primary write path → otel_metrics_*, otel_traces
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
    metrics: receivers:[prometheus] → processors:[memory_limiter,batch] → exporters:[clickhouse,kafka/metrics]
    traces:  receivers:[otlp]       → processors:[memory_limiter,batch] → exporters:[clickhouse,kafka/traces]
```

---

## Prometheus HTTP SD

`GET /v1/metrics/targets` (gpu_server_handlers.rs) — no auth, OTel Collector only:

```json
[{
  "targets": ["192.168.1.10:9100"],
  "labels": { "server_id": "uuid", "server_name": "gpu-node-1", "host": "192.168.1.10" }
}]
```

- Only servers with `node_exporter_url` set
- `host` extracted from `node_exporter_url`
- Multiple backends on same server → one target (no duplicates)

---

## Redpanda

```yaml
image: docker.redpanda.com/redpandadata/redpanda:v24.2.7
command:
  - redpanda start --smp=1 --memory=512M --overprovisioned
  - --kafka-addr=PLAINTEXT://0.0.0.0:9092
  - --advertise-kafka-addr=PLAINTEXT://redpanda:9092
```

- No JVM/ZooKeeper — single container
- `auto_create_topics_enabled: true`
- Kafka migration: change broker address only, no code changes

---

## ClickHouse Kafka Engine (Redpanda → ClickHouse)

```sql
-- Kafka Engine → MergeTree pattern
kafka_otel_metrics ENGINE=Kafka(topic='otel-metrics', format=JSONAsString)
    → MV → otel_metrics_raw ENGINE=MergeTree() TTL created_at + INTERVAL 30 DAY

kafka_otel_traces ENGINE=Kafka(topic='otel-traces', format=JSONAsString)
    → MV → otel_traces_raw  ENGINE=MergeTree() TTL created_at + INTERVAL 30 DAY
```

ClickHouse exporter is **primary write path** (`otel_metrics_gauge` → history queries).
Kafka Engine = archival path (future: replace ClickHouse exporter).

---

## GPU Server Side (docker-compose.ollama.yml)

Run on each Ollama GPU server separately:

```yaml
services:
  ollama:
    image: ollama/ollama
    ports: ["11434:11434"]

  node-exporter:
    image: prom/node-exporter:latest
    command:
      - --collector.drm      # AMD GPU VRAM + utilization
      - --collector.hwmon    # temperature, power
      - --collector.meminfo  # system RAM
    volumes:
      - /proc:/host/proc:ro
      - /sys:/host/sys:ro
    ports: ["9100:9100"]
```

**Registration flow**:
1. `POST /v1/servers` → register GPU server with `node_exporter_url`
2. OTel Collector polls `GET /v1/metrics/targets` every 30s → auto-starts scraping

---

## Helm Deployment Scenarios

```bash
# Default (all services)
helm install veronex ./helm/inferq/

# External Kafka
helm install veronex ./helm/inferq/ \
  --set redpanda.enabled=false \
  --set redpanda.externalBrokers=kafka-broker:9092

# Disable OTel Collector (use existing)
helm install veronex ./helm/inferq/ --set otelCollector.enabled=false

# External infrastructure
helm install veronex ./helm/inferq/ \
  --set postgres.enabled=false --set valkey.enabled=false --set clickhouse.enabled=false \
  --set inferq.env.databaseUrl="postgres://..." \
  --set inferq.env.valkeyUrl="redis://..." \
  --set inferq.env.clickhouseUrl="http://..."
```

When using existing OTel Collector, add this scrape job:
```yaml
receivers:
  prometheus:
    config:
      scrape_configs:
        - job_name: veronex-node-exporters
          http_sd_configs:
            - url: http://<release>.<namespace>.svc.cluster.local:3000/v1/metrics/targets
```
