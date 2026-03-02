use std::{collections::BTreeMap, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use rskafka::{
    client::{
        partition::{Compression, UnknownTopicHandling},
        ClientBuilder,
    },
    record::Record,
};

use crate::application::ports::outbound::audit_port::{AuditEvent, AuditPort};

/// Redpanda-backed implementation of [`AuditPort`].
///
/// Serialises each [`AuditEvent`] to flat JSON and produces it onto the
/// `audit` topic.  ClickHouse consumes this topic via a Kafka Engine →
/// Materialized View chain and writes rows into `audit_events`.
///
/// All errors are logged as warnings and swallowed — audit failures must
/// not affect request processing.
pub struct RedpandaAuditAdapter {
    partition_client: Arc<rskafka::client::partition::PartitionClient>,
}

impl RedpandaAuditAdapter {
    pub async fn new(brokers: Vec<String>) -> Result<Self> {
        let client = ClientBuilder::new(brokers).build().await?;
        let partition_client = client
            .partition_client("audit", 0, UnknownTopicHandling::Retry)
            .await?;
        Ok(Self {
            partition_client: Arc::new(partition_client),
        })
    }
}

#[async_trait]
impl AuditPort for RedpandaAuditAdapter {
    async fn record(&self, event: AuditEvent) {
        let event_time_ms = event.event_time.timestamp_millis();

        let payload = serde_json::json!({
            "event_time_ms":  event_time_ms,
            "account_id":     event.account_id.to_string(),
            "account_name":   event.account_name,
            "action":         event.action,
            "resource_type":  event.resource_type,
            "resource_id":    event.resource_id,
            "resource_name":  event.resource_name,
            "ip_address":     event.ip_address.unwrap_or_default(),
            "details":        event.details.unwrap_or_default(),
        });

        let json_bytes = match serde_json::to_vec(&payload) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("audit json serialization failed (non-fatal): {e}");
                return;
            }
        };

        let record = Record {
            key: None,
            value: Some(json_bytes.into()),
            headers: BTreeMap::new(),
            timestamp: chrono::Utc::now(),
        };

        if let Err(e) = self
            .partition_client
            .produce(vec![record], Compression::NoCompression)
            .await
        {
            tracing::warn!("redpanda audit produce failed (non-fatal): {e}");
        }
    }
}
