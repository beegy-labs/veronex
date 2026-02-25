use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::entities::GpuServer;

/// Outbound port for managing GPU server records.
#[async_trait]
pub trait GpuServerRegistry: Send + Sync {
    async fn register(&self, server: GpuServer) -> Result<()>;
    async fn list_all(&self) -> Result<Vec<GpuServer>>;
    async fn get(&self, id: Uuid) -> Result<Option<GpuServer>>;
    async fn delete(&self, id: Uuid) -> Result<()>;
}
