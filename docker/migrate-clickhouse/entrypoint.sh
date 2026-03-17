#!/bin/sh
set -e

# Required env vars
: "${CLICKHOUSE_HOST:?CLICKHOUSE_HOST is required}"
: "${CLICKHOUSE_USER:?CLICKHOUSE_USER is required}"
: "${CLICKHOUSE_PASSWORD:?CLICKHOUSE_PASSWORD is required}"
: "${CLICKHOUSE_DB:?CLICKHOUSE_DB is required}"

# Kafka defaults (plaintext for local docker-compose)
export KAFKA_BROKER="${KAFKA_BROKER:-redpanda:9092}"
export KAFKA_SECURITY_PROTOCOL="${KAFKA_SECURITY_PROTOCOL:-PLAINTEXT}"
export KAFKA_SASL_MECHANISM="${KAFKA_SASL_MECHANISM:-}"
export KAFKA_USERNAME="${KAFKA_USERNAME:-}"
export KAFKA_PASSWORD="${KAFKA_PASSWORD:-}"
export RETENTION_METRICS_DAYS="${RETENTION_METRICS_DAYS:-14}"
export RETENTION_LOGS_DAYS="${RETENTION_LOGS_DAYS:-7}"
export RETENTION_INFERENCE_DAYS="${RETENTION_INFERENCE_DAYS:-90}"
export RETENTION_AUDIT_DAYS="${RETENTION_AUDIT_DAYS:-90}"
export RETENTION_TRACES_DAYS="${RETENTION_TRACES_DAYS:-7}"

# Substitute __PLACEHOLDER__ vars using envsubst
TMPDIR=$(mktemp -d)
for f in /migrations/*.sql; do
  sed \
    -e "s/__KAFKA_BROKER__/${KAFKA_BROKER}/g" \
    -e "s/__KAFKA_SECURITY_PROTOCOL__/${KAFKA_SECURITY_PROTOCOL}/g" \
    -e "s/__KAFKA_SASL_MECHANISM__/${KAFKA_SASL_MECHANISM}/g" \
    -e "s/__KAFKA_USERNAME__/${KAFKA_USERNAME}/g" \
    -e "s/__KAFKA_PASSWORD__/${KAFKA_PASSWORD}/g" \
    -e "s/__CLICKHOUSE_DB__/${CLICKHOUSE_DB}/g" \
    -e "s/__RETENTION_METRICS_DAYS__/${RETENTION_METRICS_DAYS}/g" \
    -e "s/__RETENTION_LOGS_DAYS__/${RETENTION_LOGS_DAYS}/g" \
    -e "s/__RETENTION_INFERENCE_DAYS__/${RETENTION_INFERENCE_DAYS}/g" \
    -e "s/__RETENTION_AUDIT_DAYS__/${RETENTION_AUDIT_DAYS}/g" \
    -e "s/__RETENTION_TRACES_DAYS__/${RETENTION_TRACES_DAYS}/g" \
    "$f" > "$TMPDIR/$(basename "$f")"
done

DB_URL="clickhouse://${CLICKHOUSE_HOST}:9000?username=${CLICKHOUSE_USER}&password=${CLICKHOUSE_PASSWORD}&database=${CLICKHOUSE_DB}&x-multi-statement=true"

exec migrate -path "$TMPDIR" -database "$DB_URL" "${@:-up}"
