use async_trait::async_trait;
use aws_sdk_s3::{error::SdkError, primitives::ByteStream, Client};
use uuid::Uuid;

use crate::application::ports::outbound::message_store::MessageStore;

/// S3-compatible adapter for storing LLM conversation contexts.
///
/// Objects are stored as zstd-compressed JSON at `messages/{job_id}.json.zst`.
/// Compression typically achieves 10–20× ratio on LLM message arrays, reducing
/// MinIO storage and network I/O on the hot-path. Compatible with
/// MinIO (local dev) and AWS S3 (production) — select via `S3_ENDPOINT`.
pub struct S3MessageStore {
    client: Client,
    bucket: String,
}

impl S3MessageStore {
    pub fn new(client: Client, bucket: impl Into<String>) -> Self {
        Self { client, bucket: bucket.into() }
    }

    fn key(job_id: Uuid) -> String {
        format!("messages/{job_id}.json.zst")
    }

    /// Ensure the bucket exists. Called once on startup.
    pub async fn ensure_bucket(&self) -> anyhow::Result<()> {
        use aws_sdk_s3::operation::create_bucket::CreateBucketError;
        match self.client
            .create_bucket()
            .bucket(&self.bucket)
            .send()
            .await
        {
            Ok(_) => {
                tracing::info!(bucket = %self.bucket, "S3 bucket created");
                Ok(())
            }
            Err(SdkError::ServiceError(e))
                if matches!(e.err(), CreateBucketError::BucketAlreadyOwnedByYou(_)) =>
            {
                tracing::debug!(bucket = %self.bucket, "S3 bucket already exists");
                Ok(())
            }
            // MinIO returns BucketAlreadyExists (not OwnedByYou) when bucket exists
            Err(SdkError::ServiceError(e))
                if e.err().meta().code() == Some("BucketAlreadyExists") =>
            {
                tracing::debug!(bucket = %self.bucket, "S3 bucket already exists (MinIO)");
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!("failed to create S3 bucket: {e}")),
        }
    }
}

#[async_trait]
impl MessageStore for S3MessageStore {
    async fn put(&self, job_id: Uuid, data: &serde_json::Value) -> anyhow::Result<()> {
        let json = serde_json::to_vec(data)?;
        // Level 3: good compression ratio (~15× on LLM messages) with sub-millisecond latency.
        let compressed = zstd::encode_all(json.as_slice(), 3)
            .map_err(|e| anyhow::anyhow!("zstd compress failed: {e}"))?;
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(Self::key(job_id))
            .body(ByteStream::from(bytes::Bytes::from(compressed)))
            .content_type("application/zstd")
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("S3 put_object failed: {e}"))?;
        Ok(())
    }

    async fn get(&self, job_id: Uuid) -> anyhow::Result<Option<serde_json::Value>> {
        use aws_sdk_s3::operation::get_object::GetObjectError;

        let result = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(Self::key(job_id))
            .send()
            .await;

        match result {
            Ok(output) => {
                let compressed = output
                    .body
                    .collect()
                    .await
                    .map_err(|e| anyhow::anyhow!("S3 body read failed: {e}"))?
                    .into_bytes();
                let json = zstd::decode_all(compressed.as_ref())
                    .map_err(|e| anyhow::anyhow!("zstd decompress failed: {e}"))?;
                let value = serde_json::from_slice(&json)
                    .map_err(|e| anyhow::anyhow!("S3 JSON parse failed: {e}"))?;
                Ok(Some(value))
            }
            Err(SdkError::ServiceError(e))
                if matches!(e.err(), GetObjectError::NoSuchKey(_)) =>
            {
                Ok(None)
            }
            Err(e) => Err(anyhow::anyhow!("S3 get_object failed: {e}")),
        }
    }
}
