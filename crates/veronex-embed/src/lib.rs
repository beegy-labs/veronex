//! veronex-embed — embedding service.
//!
//! Wraps fastembed (ONNX Runtime) to serve text embeddings via HTTP.
//! Default model: multilingual-e5-large (1024-dim, 100+ languages).

use std::sync::Arc;

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use serde::{Deserialize, Serialize};

/// Supported embedding models. v1: multilingual-e5-large only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelId {
    MultilingualE5Large,
}

impl ModelId {
    pub fn fastembed_model(&self) -> EmbeddingModel {
        match self {
            Self::MultilingualE5Large => EmbeddingModel::MultilingualE5Large,
        }
    }

    pub fn dims(&self) -> usize {
        match self {
            Self::MultilingualE5Large => 1024,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::MultilingualE5Large => "multilingual-e5-large",
        }
    }

    pub fn default_model() -> Self {
        Self::MultilingualE5Large
    }
}

/// Shared state for the embed service.
pub struct EmbedState {
    pub model: Arc<TextEmbedding>,
    pub model_id: ModelId,
}

impl EmbedState {
    pub fn new(model_id: ModelId) -> anyhow::Result<Self> {
        let model = TextEmbedding::try_new(
            InitOptions::new(model_id.fastembed_model()).with_show_download_progress(true),
        )?;
        Ok(Self {
            model: Arc::new(model),
            model_id,
        })
    }

    pub fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let results = self
            .model
            .embed(vec![text], None)
            .map_err(|e| format!("embed failed: {e}"))?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| "no embedding returned".to_string())
    }

    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.model
            .embed(texts.to_vec(), None)
            .map_err(|e| format!("embed_batch failed: {e}"))
    }
}

// ── Request / Response types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct EmbedRequest {
    pub text: String,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EmbedBatchRequest {
    pub texts: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EmbedResponse {
    pub vector: Vec<f32>,
    pub dims: usize,
}

#[derive(Debug, Serialize)]
pub struct EmbedBatchResponse {
    pub vectors: Vec<Vec<f32>>,
    pub dims: usize,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub name: String,
    pub dims: usize,
    pub loaded: bool,
}

#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub models: Vec<ModelInfo>,
    pub default: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_id_dims() {
        assert_eq!(ModelId::MultilingualE5Large.dims(), 1024);
    }

    #[test]
    fn model_id_name() {
        assert_eq!(ModelId::MultilingualE5Large.name(), "multilingual-e5-large");
    }

    #[test]
    fn model_id_default() {
        assert_eq!(ModelId::default_model(), ModelId::MultilingualE5Large);
    }
}
