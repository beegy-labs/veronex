use anyhow::Result;
use async_trait::async_trait;

use crate::domain::entities::Model;
use crate::domain::value_objects::ModelName;

/// Outbound port for model state persistence.
#[async_trait]
pub trait ModelRepository: Send + Sync {
    async fn find_by_name(&self, name: &ModelName) -> Result<Option<Model>>;
    async fn save(&self, model: &Model) -> Result<()>;
}
