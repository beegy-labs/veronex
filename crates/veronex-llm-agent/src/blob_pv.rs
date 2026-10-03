//! Local PV cache for GGUF blobs on the agent side.
//!
//! Keyed by sha256 — same content-addressed layout as the API server's
//! `LocalPv`, but owned by the agent so a node restart doesn't lose
//! the cache and the API server doesn't need a shared filesystem.
//!
//! `POST /blobs/{sha256}` triggers a streaming fetch from Garage S3
//! (URL provided in the request body for now; future versions resolve
//! via the API server). `GET /blobs/{sha256}` returns presence + size.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use tokio::fs;

/// Sub-directory under the configured base where verified blobs live.
pub const BLOBS_SUBDIR: &str = "blobs";

#[derive(Debug, Clone)]
pub struct BlobPv {
    base: PathBuf,
}

impl BlobPv {
    pub fn new<P: Into<PathBuf>>(base: P) -> Self {
        Self { base: base.into() }
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn blobs_dir(&self) -> PathBuf {
        self.base.join(BLOBS_SUBDIR)
    }

    pub fn path_for(&self, sha256: &str) -> PathBuf {
        self.blobs_dir().join(format!("{sha256}.gguf"))
    }

    /// Idempotent — safe to call on every startup. Used by the agent
    /// boot path before any HTTP traffic so first-blob fetches don't
    /// race the directory creation.
    pub async fn ensure_root(&self) -> Result<()> {
        let dir = self.blobs_dir();
        fs::create_dir_all(&dir)
            .await
            .with_context(|| format!("create_dir_all {}", dir.display()))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BlobStatus {
    pub sha256: String,
    pub present: bool,
    pub size: Option<u64>,
}

impl BlobPv {
    pub async fn status(&self, sha256: &str) -> BlobStatus {
        let path = self.path_for(sha256);
        match fs::metadata(&path).await {
            Ok(m) if m.is_file() => BlobStatus {
                sha256: sha256.to_string(),
                present: true,
                size: Some(m.len()),
            },
            _ => BlobStatus {
                sha256: sha256.to_string(),
                present: false,
                size: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as std_fs;

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "veronex-llm-agent-test-{}",
            uuid::Uuid::now_v7()
        ));
        std_fs::create_dir_all(&p).unwrap();
        p
    }

    #[tokio::test]
    async fn ensure_root_creates_blobs_subdir() {
        let dir = tempdir();
        let pv = BlobPv::new(&dir);
        pv.ensure_root().await.unwrap();
        assert!(dir.join(BLOBS_SUBDIR).exists());
    }

    #[tokio::test]
    async fn status_reports_present_with_size() {
        let dir = tempdir();
        let pv = BlobPv::new(&dir);
        pv.ensure_root().await.unwrap();
        let sha = "abc123";
        std_fs::write(pv.path_for(sha), b"hello").unwrap();
        let s = pv.status(sha).await;
        assert!(s.present);
        assert_eq!(s.size, Some(5));
    }

    #[tokio::test]
    async fn status_reports_absent() {
        let dir = tempdir();
        let pv = BlobPv::new(&dir);
        pv.ensure_root().await.unwrap();
        let s = pv.status("missing").await;
        assert!(!s.present);
        assert!(s.size.is_none());
    }
}
