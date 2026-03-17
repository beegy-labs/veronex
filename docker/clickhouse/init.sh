#!/usr/bin/env bash
set -e
# Applies ClickHouse schema with configurable data retention TTLs.
# Runs from /docker-entrypoint-initdb.d/ on first volume creation.
#
# Env vars (set via docker-compose environment:):
#   CLICKHOUSE_RETENTION_METRICS_DAYS    otel_metrics_gauge      (default: 14)
#   CLICKHOUSE_RETENTION_LOGS_DAYS       otel_logs               (default: 7)
#   CLICKHOUSE_RETENTION_INFERENCE_DAYS  inference_logs           (default: 90)
#   CLICKHOUSE_RETENTION_AUDIT_DAYS      audit_events            (default: 90)
#   CLICKHOUSE_RETENTION_TRACES_DAYS     otel_traces_raw         (default: 7)
#
# Defaults are defined once in docker-compose.yml; do not duplicate here.

METRICS_DAYS="${CLICKHOUSE_RETENTION_METRICS_DAYS:?missing CLICKHOUSE_RETENTION_METRICS_DAYS}"
LOGS_DAYS="${CLICKHOUSE_RETENTION_LOGS_DAYS:?missing CLICKHOUSE_RETENTION_LOGS_DAYS}"
INFERENCE_DAYS="${CLICKHOUSE_RETENTION_INFERENCE_DAYS:?missing CLICKHOUSE_RETENTION_INFERENCE_DAYS}"
AUDIT_DAYS="${CLICKHOUSE_RETENTION_AUDIT_DAYS:?missing CLICKHOUSE_RETENTION_AUDIT_DAYS}"
TRACES_DAYS="${CLICKHOUSE_RETENTION_TRACES_DAYS:?missing CLICKHOUSE_RETENTION_TRACES_DAYS}"
KAFKA_BROKER="${KAFKA_BROKER:-redpanda:9092}"
KAFKA_USER="${KAFKA_USERNAME:-}"
KAFKA_PASS="${KAFKA_PASSWORD:-}"

sed \
  -e "s/__RETENTION_METRICS_DAYS__/${METRICS_DAYS}/g"         \
  -e "s/__RETENTION_LOGS_DAYS__/${LOGS_DAYS}/g"               \
  -e "s/__RETENTION_INFERENCE_DAYS__/${INFERENCE_DAYS}/g"     \
  -e "s/__RETENTION_AUDIT_DAYS__/${AUDIT_DAYS}/g"             \
  -e "s/__RETENTION_TRACES_DAYS__/${TRACES_DAYS}/g"           \
  -e "s/__KAFKA_BROKER__/${KAFKA_BROKER}/g"               \
  -e "s/__KAFKA_USERNAME__/${KAFKA_USER}/g"               \
  -e "s/__KAFKA_PASSWORD__/${KAFKA_PASS}/g"               \
  -e "s/__CLICKHOUSE_DB__/${CLICKHOUSE_DB}/g"             \
  /docker-entrypoint-initdb.d/schema.sql.tmpl             \
| clickhouse-client                                        \
    --host 127.0.0.1                                       \
    --user "${CLICKHOUSE_USER}"                            \
    --password "${CLICKHOUSE_PASSWORD}"                    \
    --database "${CLICKHOUSE_DB}"                          \
    --multiquery
