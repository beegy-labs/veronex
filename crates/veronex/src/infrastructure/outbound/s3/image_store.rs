use async_trait::async_trait;
use aws_sdk_s3::{error::SdkError, primitives::ByteStream, Client};
use uuid::Uuid;

use crate::application::ports::outbound::image_store::ImageStore;

/// S3-compatible adapter for storing inference job images as WebP.
///
/// Uses `images/{job_id}/{index}.webp` and `images/{job_id}/{index}_thumb.webp`.
/// Compatible with MinIO (local dev) and AWS S3 (production).
pub struct S3ImageStore {
    client: Client,
    bucket: String,
    /// Public endpoint base URL for constructing direct URLs.
    /// e.g. `"http://localhost:9000/veronex-images"`
    endpoint_url: String,
}

impl S3ImageStore {
    pub fn new(client: Client, bucket: impl Into<String>, endpoint_url: impl Into<String>) -> Self {
        let bucket = bucket.into();
        let endpoint_url = endpoint_url.into();
        Self { client, bucket, endpoint_url }
    }

    fn full_key(job_id: Uuid, index: usize) -> String {
        format!("images/{job_id}/{index}.webp")
    }

    fn thumb_key(job_id: Uuid, index: usize) -> String {
        format!("images/{job_id}/{index}_thumb.webp")
    }

    async fn put_object(&self, key: &str, data: &[u8]) -> anyhow::Result<()> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(bytes::Bytes::copy_from_slice(data)))
            .content_type("image/webp")
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("S3 put_object failed for {key}: {e}"))?;
        Ok(())
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
                tracing::info!(bucket = %self.bucket, "S3 image bucket created");
                Ok(())
            }
            Err(SdkError::ServiceError(e))
                if matches!(e.err(), CreateBucketError::BucketAlreadyOwnedByYou(_)) =>
            {
                Ok(())
            }
            Err(SdkError::ServiceError(e))
                if e.err().meta().code() == Some("BucketAlreadyExists") =>
            {
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!("failed to create S3 image bucket: {e}")),
        }
    }
}

#[async_trait]
impl ImageStore for S3ImageStore {
    async fn put(
        &self,
        job_id: Uuid,
        index: usize,
        webp: &[u8],
        thumb: &[u8],
    ) -> anyhow::Result<(String, String)> {
        let fk = Self::full_key(job_id, index);
        let tk = Self::thumb_key(job_id, index);
        self.put_object(&fk, webp).await?;
        self.put_object(&tk, thumb).await?;
        Ok((fk, tk))
    }

    async fn put_base64(
        &self,
        job_id: Uuid,
        index: usize,
        b64: &str,
    ) -> anyhow::Result<(String, String)> {
        let b64 = b64.to_string();
        let (full, thumb) = tokio::task::spawn_blocking(move || {
            super::webp_convert::base64_to_webp_pair(&b64)
        })
        .await??;
        self.put(job_id, index, &full, &thumb).await
    }

    fn url(&self, key: &str) -> String {
        format!("{}/{key}", self.endpoint_url.trim_end_matches('/'))
    }
}
