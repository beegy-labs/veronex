#!/usr/bin/env bash
# Applies ClickHouse schema with configurable data retention TTLs.
# Runs from /docker-entrypoint-initdb.d/ on first volume creation.
#
# Env vars (set via docker-compose environment:):
#   CLICKHOUSE_RETENTION_ANALYTICS_DAYS  inference + audit logs  (default: 90)
#   CLICKHOUSE_RETENTION_METRICS_DAYS    metrics, traces, hw     (default: 30)
#   CLICKHOUSE_RETENTION_AUDIT_DAYS      audit_events table      (default: 365)
#
# Defaults are defined once in docker-compose.yml; do not duplicate here.

ANALYTICS_DAYS="${CLICKHOUSE_RETENTION_ANALYTICS_DAYS:?missing CLICKHOUSE_RETENTION_ANALYTICS_DAYS}"
METRICS_DAYS="${CLICKHOUSE_RETENTION_METRICS_DAYS:?missing CLICKHOUSE_RETENTION_METRICS_DAYS}"
AUDIT_DAYS="${CLICKHOUSE_RETENTION_AUDIT_DAYS:?missing CLICKHOUSE_RETENTION_AUDIT_DAYS}"
KAFKA_BROKER="${KAFKA_BROKER:-redpanda:9092}"
KAFKA_USER="${KAFKA_USERNAME:-}"
KAFKA_PASS="${KAFKA_PASSWORD:-}"

sed \
  -e "s/__RETENTION_ANALYTICS_DAYS__/${ANALYTICS_DAYS}/g" \
  -e "s/__RETENTION_METRICS_DAYS__/${METRICS_DAYS}/g"     \
  -e "s/__RETENTION_AUDIT_DAYS__/${AUDIT_DAYS}/g"         \
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
