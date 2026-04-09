/// veronex-consumer: Kafka consumer for OTel pipeline.
///
/// Pipeline: veronex-analytics → OTel Collector → Redpanda → [this] → ClickHouse
///
/// At-least-once guarantee:
///   1. Consume batch from Redpanda
///   2. Parse OTLP JSON + fan-out rows to per-table buffers
///   3. INSERT into ClickHouse (all tables)
///   4. Commit offsets ONLY after ALL inserts succeed
///
/// Idempotency: ClickHouse block-level deduplication (`insert_deduplicate=1`, default)
/// deduplicates identical blocks on retry. Safe to re-deliver messages after restart.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::StreamExt;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::Message;
use rdkafka::{ClientConfig, Offset, TopicPartitionList};
use tokio::signal;

mod clickhouse;
mod config;
mod handlers;
mod otlp;

use clickhouse::ClickhouseClient;
use config::Config;

const TOPICS: &[&str] = &["otel-logs", "otel-metrics", "otel-traces"];
const MAX_BATCH: usize = 500;
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::from_env()?;

    let ch = Arc::new(ClickhouseClient::new(
        cfg.clickhouse_url.clone(),
        cfg.clickhouse_db.clone(),
        cfg.clickhouse_user.clone(),
        cfg.clickhouse_password.clone(),
    ));

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &cfg.kafka_broker)
        .set("group.id", &cfg.kafka_group_id)
        .set("enable.auto.commit", "false") // manual commit after INSERT
        .set("auto.offset.reset", "earliest")
        .set("enable.partition.eof", "false")
        .set("session.timeout.ms", "10000")
        .set("max.poll.interval.ms", "300000")
        .create()
        .context("Failed to create Kafka consumer")?;

    consumer
        .subscribe(TOPICS)
        .context("Failed to subscribe to topics")?;

    tracing::info!("veronex-consumer started, topics={:?}", TOPICS);

    run_consumer_loop(consumer, ch).await
}

async fn run_consumer_loop(consumer: StreamConsumer, ch: Arc<ClickhouseClient>) -> Result<()> {
    let mut stream = consumer.stream();

    let mut log_buf = handlers::logs::LogRows::default();
    let mut metric_buf: Vec<serde_json::Value> = Vec::new();
    let mut trace_buf: Vec<serde_json::Value> = Vec::new();

    // Latest committed offset per (topic, partition) — only updated on successful flush.
    let mut pending: HashMap<(String, i32), i64> = HashMap::new();

    let mut flush_timer = tokio::time::interval(FLUSH_INTERVAL);
    flush_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;

            msg = stream.next() => {
                match msg {
                    None => {
                        tracing::warn!("Kafka stream ended unexpectedly");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::error!("Kafka error: {e}");
                        continue;
                    }
                    Some(Ok(m)) => {
                        let topic     = m.topic().to_owned();
                        let partition = m.partition();
                        let offset    = m.offset();

                        if let Some(payload) = m.payload() {
                            match topic.as_str() {
                                "otel-logs" => match handlers::logs::parse(payload) {
                                    Ok(rows) => log_buf.extend(rows),
                                    Err(e)   => tracing::warn!("Failed to parse otel-logs: {e}"),
                                },
                                "otel-metrics" => match handlers::metrics::parse(payload) {
                                    Ok(rows) => metric_buf.extend(rows),
                                    Err(e)   => tracing::warn!("Failed to parse otel-metrics: {e}"),
                                },
                                "otel-traces" => match handlers::traces::parse(payload) {
                                    Ok(rows) => trace_buf.extend(rows),
                                    Err(e)   => tracing::warn!("Failed to parse otel-traces: {e}"),
                                },
                                other => tracing::debug!("Unknown topic: {other}"),
                            }
                        }

                        // Track latest offset — will commit after successful flush.
                        pending.insert((topic, partition), offset + 1);

                        // Flush when batch size threshold is reached.
                        let total = log_buf.len() + metric_buf.len() + trace_buf.len();
                        if total >= MAX_BATCH {
                            flush(&ch, &consumer, &mut log_buf, &mut metric_buf, &mut trace_buf, &mut pending).await;
                        }
                    }
                }
            }

            _ = flush_timer.tick() => {
                let total = log_buf.len() + metric_buf.len() + trace_buf.len();
                if total > 0 {
                    flush(&ch, &consumer, &mut log_buf, &mut metric_buf, &mut trace_buf, &mut pending).await;
                }
            }

            _ = signal::ctrl_c() => {
                tracing::info!("Received SIGINT, shutting down");
                break;
            }
        }
    }

    // Final flush before exit.
    let total = log_buf.len() + metric_buf.len() + trace_buf.len();
    if total > 0 {
        flush(&ch, &consumer, &mut log_buf, &mut metric_buf, &mut trace_buf, &mut pending).await;
    }

    Ok(())
}

/// Flush all buffers to ClickHouse and commit offsets if all inserts succeed.
///
/// On INSERT failure: logs error, keeps buffers intact, does NOT commit offsets.
/// Next flush will retry with accumulated rows (old + new). ClickHouse block-level
/// dedup (`insert_deduplicate=1`) makes retried inserts idempotent.
async fn flush(
    ch: &ClickhouseClient,
    consumer: &StreamConsumer,
    log_buf: &mut handlers::logs::LogRows,
    metric_buf: &mut Vec<serde_json::Value>,
    trace_buf: &mut Vec<serde_json::Value>,
    pending: &mut HashMap<(String, i32), i64>,
) {
    let inserts: &[(&str, &dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>>)] = &[];
    let _ = inserts; // unused, inserts done inline below

    macro_rules! try_insert {
        ($table:expr, $rows:expr) => {
            if !$rows.is_empty() {
                if let Err(e) = ch.insert($table, $rows).await {
                    tracing::error!("INSERT into {} failed (will retry): {e}", $table);
                    return; // abort flush — offsets not committed
                }
            }
        };
    }

    try_insert!("otel_logs",         &log_buf.otel_logs);
    try_insert!("inference_logs",    &log_buf.inference_logs);
    try_insert!("audit_events",      &log_buf.audit_events);
    try_insert!("mcp_tool_calls",    &log_buf.mcp_tool_calls);
    try_insert!("otel_metrics_gauge", metric_buf);
    try_insert!("otel_traces_raw",   trace_buf);

    // All inserts succeeded — commit offsets.
    if !pending.is_empty() {
        let mut tpl = TopicPartitionList::new();
        for ((topic, partition), offset) in pending.drain() {
            if let Err(e) = tpl.add_partition_offset(&topic, partition, Offset::Offset(offset)) {
                tracing::error!("Failed to set partition offset in TPL: {e}");
            }
        }
        if let Err(e) = consumer.commit(&tpl, CommitMode::Async) {
            tracing::error!("Failed to commit offsets: {e}");
        }
    }

    log_buf.clear();
    metric_buf.clear();
    trace_buf.clear();
}
