use std::pin::Pin;

use anyhow::Result;
use futures::Stream;

use crate::domain::value_objects::{JobId, StreamToken};

/// Outbound port for pushing/reading SSE token streams.
pub trait StreamPort: Send + Sync {
    fn stream(&self, job_id: &JobId) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>>;
}
