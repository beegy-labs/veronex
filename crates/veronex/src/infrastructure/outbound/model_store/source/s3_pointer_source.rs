//! Garage-internal S3 pointer source.
//!
//! For the case where an external tool (CI artifact build, llama.cpp
//! `quantize` job, manual `aws s3 cp`) has already pushed a GGUF into the
//! Veronex bucket. The operator just registers the existing object — no
//! copy, no re-upload.
//!
//! sha256 pre-resolution looks for an `x-amz-meta-sha256` user metadata
//! field on the object (set by the tool that uploaded it). If absent, we
//! fall through to streaming: the install pipeline GETs the object, hashes
//! it, and updates the registry. The CAS still ends up correctly keyed —
//! the caller just spent an extra round-trip on a file we already had.

use anyhow::{Context as _, Result};
use aws_sdk_s3::Client;
use bytes::Bytes;

use super::{ByteStream, ModelSource, Sha256Hex};

pub struct S3PointerSource {
    client: Client,
    bucket: String,
    s3_key: String,
}

impl S3PointerSource {
    pub fn new(client: Client, bucket: impl Into<String>, s3_key: impl Into<String>) -> Self {
        Self {
            client,
            bucket: bucket.into(),
            s3_key: s3_key.into(),
        }
    }
}

#[async_trait::async_trait]
impl ModelSource for S3PointerSource {
    async fn resolve_sha256(&self) -> Result<Option<Sha256Hex>> {
        // HEAD lifts the user metadata map. Tools that uploaded the GGUF
        // with `--metadata sha256=...` populate `x-amz-meta-sha256`; the
        // SDK exposes it under the `metadata` field of the response.
        let resp = match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&self.s3_key)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };
        let sha = resp
            .metadata()
            .and_then(|m| m.get("sha256"))
            .map(|s| s.to_lowercase())
            .filter(|hex| hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()));
        Ok(sha)
    }

    async fn stream_blob(&self) -> Result<ByteStream> {
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&self.s3_key)
            .send()
            .await
            .with_context(|| format!("GET s3://{}/{}", self.bucket, self.s3_key))?;
        // Re-emit the SDK's async byte stream as a Bytes-based stream that
        // matches the ModelSource trait. `next()` on `aws_sdk_s3::primitives::
        // ByteStream` yields the next chunk of bytes from the body.
        let mut body = resp.body;
        let stream = async_stream::try_stream! {
            loop {
                match body.next().await {
                    Some(Ok(bytes)) => yield Bytes::from(bytes.to_vec()),
                    Some(Err(e)) => Err(anyhow::anyhow!(e))?,
                    None => break,
                }
            }
        };
        Ok(Box::pin(stream))
    }

    fn total_bytes(&self) -> Option<u64> {
        None
    }

    fn source_spec(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "s3_pointer",
            "s3_key": self.s3_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Phase 2 ships the trait + spec wiring; integration tests against a
    // real S3 (localstack / Garage) are deferred to the install_orchestrator
    // tests which exercise the full pipeline.

    #[test]
    fn source_spec_carries_canonical_fields() {
        // Build a Client without performing any network IO. We use a
        // hard-coded fake region; the test only inspects source_spec.
        let cfg = aws_sdk_s3::Config::builder()
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .credentials_provider(aws_sdk_s3::config::Credentials::new(
                "ak", "sk", None, None, "test",
            ))
            .build();
        let client = Client::from_conf(cfg);
        let src = S3PointerSource::new(client, "veronex-models", "external/x.gguf");
        let s = src.source_spec();
        assert_eq!(s["type"], "s3_pointer");
        assert_eq!(s["s3_key"], "external/x.gguf");
    }
}
