# Observability — Production Config (2026)

> **Last Researched**: 2026-04-07 | **Source**: OTel docs + ClickHouse docs + web search
> **Companion**: `research/infrastructure/observability.md` — core patterns

---

## OTel Collector — Processor Ordering (Critical)

`memory_limiter` **must** be first in every pipeline. `batch` second. Everything else after.

```yaml
processors:
  memory_limiter:
    limit_mib: 1800
    spike_limit_mib: 500
    check_interval: 1s
  batch:
    send_batch_size: 1024
    send_batch_max_size: 2048
    timeout: 5s
```

---

## OTel Receiver — Max Message Size

```yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317       # must be explicit 0.0.0.0 in containers (v0.110.0+ changed default to localhost)
        max_recv_msg_size_mib: 16
```

---

## OTel Exporter — Persistent Queue

```yaml
exporters:
  kafka:
    retry_on_failure:
      initial_interval: 5s
      max_interval: 30s
      max_elapsed_time: 300s
    sending_queue:
      num_consumers: 10
      queue_size: 5000
      storage: file_storage/queue   # survives collector restarts
extensions:
  file_storage/queue:
    directory: /var/lib/otel/queue
```

Queue size formula: `expected_throughput_per_sec × acceptable_outage_minutes × 60`

---

## Alert Metrics

| Metric | Signals |
|--------|---------|
| `otelcol_receiver_refused_spans` | Auth or rate limit failures |
| `otelcol_exporter_queue_size` | Backend (Redpanda) bottleneck |

---

## ClickHouse Kafka Engine — Tuning

| Parameter | Default | Recommended | Why |
|-----------|---------|-------------|-----|
| `kafka_max_block_size` | 65,536 | 524,288 | Default causes excessive flush cycles |
| `kafka_num_consumers` | 1 | Match partition count | Parallelizes consumption |
| `kafka_thread_per_consumer` | 0 | 1 | Parallel flush vs. sequential |
| `input_format_skip_unknown_fields` | 0 | 1 | OTel schema drift resilience |

```sql
ENGINE = Kafka SETTINGS
    kafka_broker_list = 'redpanda:9092',
    kafka_topic_list = 'otel-logs',
    kafka_group_name = 'clickhouse-otel-logs',
    kafka_format = 'JSONEachRow',
    kafka_max_block_size = 524288,
    kafka_thread_per_consumer = 1,
    input_format_skip_unknown_fields = 1;
```

**Multiple MV warning:** Chaining multiple MVs to a single Kafka engine is less reliable — inserts across chained MVs are not atomic. Use separate Kafka engine tables for fanout.

**TTL clause (required for retention):**
```sql
TTL Timestamp + INTERVAL 30 DAY DELETE
```

---

## Sources

- [OTel production setup 2026](https://oneuptime.com/blog/post/2026-01-25-opentelemetry-collector-production-setup/view)
- [OTel security config best practices](https://opentelemetry.io/docs/security/config-best-practices/)
- [ClickHouse Kafka ingestion 2026](https://oneuptime.com/blog/post/2026-01-21-clickhouse-kafka-ingestion/view)
- [Chaining materialized views](https://clickhouse.com/blog/chaining-materialized-views)
