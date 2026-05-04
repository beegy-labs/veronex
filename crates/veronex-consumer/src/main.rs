//! veronex-consumer: Kafka consumer for OTel pipeline.
//!
//! Pipeline: veronex-analytics → OTel Collector → Redpanda → [this] → ClickHouse
//!
//! At-least-once guarantee:
//!   1. Consume batch from Redpanda
//!   2. Parse OTLP JSON + fan-out rows to per-table buffers
//!   3. INSERT into ClickHouse (all tables)
//!   4. Commit offsets ONLY after ALL inserts succeed
//!
//! Idempotency: ClickHouse block-level deduplication (`insert_deduplicate=1`, default)
//! deduplicates identical blocks on retry. Safe to re-deliver messages after restart.

use std::collections::HashMap;
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

const TOPICS: &[&str] = &["otel.audit.logs", "otel.audit.metrics", "otel.audit.traces"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_match_otel_exporter_config() {
        assert_eq!(TOPICS.len(), 3);
        assert!(TOPICS.contains(&"otel.audit.logs"));
        assert!(TOPICS.contains(&"otel.audit.metrics"));
        assert!(TOPICS.contains(&"otel.audit.traces"));
    }
}
const MAX_BATCH: usize = 500;
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);

fn main() -> Result<()> {
    // available_parallelism() returns the host CPU count, ignoring the cgroup
    // CPU limit — on a 16-core node this allocates 16 worker threads + 128
    // blocking threads regardless of the pod's 500m CPU request. Stack +
    // per-thread state alone push us past a 256Mi memory cap. Cap workers at
    // 2 (matches 500m CPU) and trim blocking-thread pool for I/O-bound work.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(8)
        .thread_name("veronex-consumer-worker")
        .enable_all()
        .build()?;
    rt.block_on(async_main())
}

async fn async_main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::from_env()?;

    let ch = ClickhouseClient::new(
        cfg.clickhouse_url.clone(),
        cfg.clickhouse_db.clone(),
        cfg.clickhouse_user.clone(),
        cfg.clickhouse_password.clone(),
    );

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &cfg.kafka_broker)
        .set("group.id", &cfg.kafka_group_id)
        .set("security.protocol", &cfg.kafka_security_protocol)
        .set("sasl.mechanisms", &cfg.kafka_sasl_mechanism)
        .set("sasl.username", &cfg.kafka_username)
        .set("sasl.password", &cfg.kafka_password)
        .set("enable.auto.commit", "false") // manual commit after INSERT
        .set("auto.offset.reset", "latest")
        .set("enable.partition.eof", "false")
        .set("session.timeout.ms", "10000")
        .set("max.poll.interval.ms", "300000")
        // Memory-bound the librdkafka prefetch queue. Defaults
        // (queued.max.messages.kbytes=1GiB per partition, queued.min.messages
        // =100k) overflow any reasonable pod cap when subscribed to multiple
        // partitioned topics. With 3 topics x N partitions and these caps:
        //   - per-partition queue: <= 4 MiB
        //   - per-fetch response: <= 4 MiB
        // Worst-case prefetch memory: ~ partitions x 4 MiB. For 18 partitions
        // (3 topics x 6) ≈ 72 MiB — well inside a 256Mi pod limit alongside
        // Tokio runtime, ClickHouse client, and the MAX_BATCH=500 buffers.
        .set("queued.max.messages.kbytes", "4096")    // 4 MiB per partition
        .set("queued.min.messages", "1000")           // prefetch target (was 100k)
        .set("fetch.message.max.bytes", "524288")     // 512 KiB max single message
        .set("fetch.max.bytes", "4194304")            // 4 MiB max per fetch response
        .set("receive.message.max.bytes", "8388608")  // 8 MiB max protocol payload
        .create()
        .context("Failed to create Kafka consumer")?;

    consumer
        .subscribe(TOPICS)
        .context("Failed to subscribe to topics")?;

    tracing::info!("veronex-consumer started, topics={:?}", TOPICS);

    run_consumer_loop(consumer, ch).await
}

async fn run_consumer_loop(consumer: StreamConsumer, ch: ClickhouseClient) -> Result<()> {
    // Register shutdown future once — handles SIGINT (ctrl_c) + SIGTERM (K8s).
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

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
                                "otel.audit.logs" => match handlers::logs::parse(payload) {
                                    Ok(rows) => log_buf.extend(rows),
                                    Err(e)   => tracing::warn!("Failed to parse otel.audit.logs: {e}"),
                                },
                                "otel.audit.metrics" => match handlers::metrics::parse(payload) {
                                    Ok(rows) => metric_buf.extend(rows),
                                    Err(e)   => tracing::warn!("Failed to parse otel.audit.metrics: {e}"),
                                },
                                "otel.audit.traces" => match handlers::traces::parse(payload) {
                                    Ok(rows) => trace_buf.extend(rows),
                                    Err(e)   => tracing::warn!("Failed to parse otel.audit.traces: {e}"),
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

            _ = &mut shutdown => {
                tracing::info!("Received shutdown signal, shutting down");
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
    macro_rules! try_insert {
        ($table:expr, $rows:expr) => {{
            if !$rows.is_empty() {
                let n = $rows.len();
                match ch.insert($table, &$rows).await {
                    Ok(_) => {}
                    Err(clickhouse::InsertError::BadData(msg)) => {
                        // HTTP 4xx: data is malformed — retrying won't help.
                        // Discard rows and continue so the pipeline doesn't stall.
                        tracing::error!("INSERT into {} bad data (discarding {n} rows): {}", $table, msg);
                        $rows.clear();
                    }
                    Err(clickhouse::InsertError::Transient(e)) => {
                        tracing::error!("INSERT into {} failed (will retry): {e}", $table);
                        return; // abort flush — offsets not committed
                    }
                }
            }
        }};
    }

    try_insert!("otel_logs",         log_buf.otel_logs);
    try_insert!("inference_logs",    log_buf.inference_logs);
    try_insert!("audit_events",      log_buf.audit_events);
    try_insert!("mcp_tool_calls",    log_buf.mcp_tool_calls);
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

/// Resolves on SIGINT (Ctrl+C) or SIGTERM (K8s pod termination).
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
