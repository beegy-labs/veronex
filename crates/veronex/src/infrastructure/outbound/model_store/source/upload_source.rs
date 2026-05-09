//! Direct multipart upload source.
//!
//! Used when the admin uploads a custom-quantized GGUF directly through
//! `POST /v1/admin/models/:id/upload`. The HTTP handler builds the
//! [`UploadSource`] from the multipart field's stream and hands it to the
//! install orchestrator. The orchestrator pipes the stream through the
//! Local PV streaming-write path (which hashes incidentally), so this
//! adapter is little more than a stream-passthrough wrapper.
//!
//! sha256 cannot be pre-resolved (the body hasn't arrived yet) — we always
//! return `Ok(None)` from `resolve_sha256`. The orchestrator falls back to
//! streaming compute and only hits the CAS dedup path AFTER the upload
//! completes, which means a duplicate upload still pays the wire cost. That
//! tradeoff is acceptable: uploads are rare admin actions, not request-rate.

use std::sync::Mutex;

use anyhow::Result;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::Stream;

use super::{ByteStream, ModelSource, Sha256Hex};

/// Direct upload source.
///
/// The contained stream is moved out by `stream_blob`; calling that method
/// twice returns an error. This matches the underlying multipart body which
/// is single-use.
pub struct UploadSource {
    stream: Mutex<Option<ByteStream>>,
    uploaded_at: DateTime<Utc>,
    uploaded_by: String,
    total_bytes_hint: Option<u64>,
}

impl UploadSource {
    /// `stream` is the body of the multipart field. `uploaded_by` is an
    /// audit-trail string (admin username or token alias) — purely for
    /// logging in `source_spec`.
    pub fn new<S>(stream: S, uploaded_by: impl Into<String>) -> Self
    where
        S: Stream<Item = Result<Bytes>> + Send + 'static,
    {
        Self {
            stream: Mutex::new(Some(Box::pin(stream))),
            uploaded_at: Utc::now(),
            uploaded_by: uploaded_by.into(),
            total_bytes_hint: None,
        }
    }

    /// Optional `Content-Length` from the upload header. Phase 2 uses it
    /// only for SSE progress bars; the install pipeline doesn't depend on it.
    pub fn with_total_bytes(mut self, total: Option<u64>) -> Self {
        self.total_bytes_hint = total;
        self
    }
}

#[async_trait::async_trait]
impl ModelSource for UploadSource {
    async fn resolve_sha256(&self) -> Result<Option<Sha256Hex>> {
        Ok(None)
    }

    async fn stream_blob(&self) -> Result<ByteStream> {
        let mut slot = self.stream.lock().expect("poisoned");
        slot.take()
            .ok_or_else(|| anyhow::anyhow!("UploadSource::stream_blob called twice"))
    }

    fn total_bytes(&self) -> Option<u64> {
        self.total_bytes_hint
    }

    fn source_spec(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "upload",
            "uploaded_at": self.uploaded_at.to_rfc3339(),
            "uploaded_by": self.uploaded_by,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use futures::StreamExt as _;

    fn ok_chunks(chunks: Vec<&'static [u8]>) -> impl Stream<Item = Result<Bytes>> {
        stream::iter(chunks.into_iter().map(|b| Ok(Bytes::from(b))))
    }

    #[tokio::test]
    async fn resolve_sha256_always_none() {
        let src = UploadSource::new(ok_chunks(vec![b"x"]), "admin");
        assert_eq!(src.resolve_sha256().await.unwrap(), None);
    }

    #[tokio::test]
    async fn stream_blob_yields_chunks_in_order() {
        let src = UploadSource::new(ok_chunks(vec![b"hel", b"lo"]), "admin");
        let mut s = src.stream_blob().await.unwrap();
        let mut acc = Vec::new();
        while let Some(c) = s.next().await {
            acc.extend_from_slice(&c.unwrap());
        }
        assert_eq!(&acc, b"hello");
    }

    #[tokio::test]
    async fn stream_blob_errors_on_second_call() {
        let src = UploadSource::new(ok_chunks(vec![b"x"]), "admin");
        let _ = src.stream_blob().await.unwrap();
        let r = src.stream_blob().await;
        assert!(r.is_err());
    }

    #[test]
    fn source_spec_includes_uploaded_by_and_timestamp() {
        let src = UploadSource::new(ok_chunks(vec![b"x"]), "admin@example");
        let s = src.source_spec();
        assert_eq!(s["type"], "upload");
        assert_eq!(s["uploaded_by"], "admin@example");
        assert!(s["uploaded_at"].is_string());
    }

    #[test]
    fn total_bytes_propagates_when_set() {
        let src = UploadSource::new(ok_chunks(vec![b"x"]), "admin")
            .with_total_bytes(Some(12345));
        assert_eq!(src.total_bytes(), Some(12345));
    }
}
