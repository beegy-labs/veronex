-- Migration 000004: Remove ClickHouse Kafka Engine tables
--
-- Replaces the internal Kafka Engine consumer pattern with an external
-- Redpanda Connect consumer.  The MergeTree target tables and all derived
-- Materialized Views are unaffected — only the three Kafka Engine ingestion
-- tables and their immediate MVs are dropped.
--
-- New topology:
--   OTel Collector → Redpanda → Redpanda Connect → ClickHouse HTTP INSERT
--
-- Rationale:
--   • n services share one ClickHouse cluster; Kafka Engine background threads
--     are cluster-global, so one misbehaving table degrades all services.
--   • at-least-once semantics only; rebalancing cascades on 24.x+.
--   • External consumer (Redpanda Connect) isolates ingest load and provides
--     proper failure boundaries per service.

DROP VIEW  IF EXISTS kafka_otel_logs_mv;
DROP TABLE IF EXISTS kafka_otel_logs;

DROP VIEW  IF EXISTS kafka_otel_metrics_mv;
DROP TABLE IF EXISTS kafka_otel_metrics;

DROP VIEW  IF EXISTS kafka_otel_traces_mv;
DROP TABLE IF EXISTS kafka_otel_traces;
