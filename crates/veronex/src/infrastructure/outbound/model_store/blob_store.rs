//! Garage S3 CAS (Content-Addressable Storage) client for GGUF blobs.
//!
//! All blobs are keyed by their sha256 under `gguf-blobs/{sha256}.gguf` in a
//! single Garage bucket. The S3 API surface is the same as MinIO/AWS so the
//! existing `aws-sdk-s3` v1 client (already used by `S3ImageStore`) carries
//! over directly.
//!
//! Phase 2 ships HEAD / GET / PUT / DELETE plus a startup `ensure_bucket`.
//! Streaming put_from_path is the production path: the install pipeline
//! writes to Local PV first (sha256 verified there), then `put_from_path`
//! pushes the verified file to Garage with no rehash.

use std::path::Path;

use anyhow::{Context as _, Result};
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;

/// Object key prefix for CAS-keyed GGUF blobs.
///
/// All blobs land at `{KEY_PREFIX}{sha256}.gguf`. Keep this in one place so
/// callers (registry, admin tools) construct keys via [`BlobStore::key_for`]
/// rather than hard-coding the layout.
pub const KEY_PREFIX: &str = "gguf-blobs/";

/// Wrapper over an `aws-sdk-s3` client bound to the GGUF blob bucket.
///
/// Cheap to clone (`Client` is internally `Arc`); pass by value to handlers
/// or hold one inside the install orchestrator.
#[derive(Clone)]
pub struct BlobStore {
    client: Client,
    bucket: String,
}

impl BlobStore {
    pub fn new(client: Client, bucket: impl Into<String>) -> Self {
        Self { client, bucket: bucket.into() }
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Compose the canonical S3 key for a sha256.
    pub fn key_for(sha256: &str) -> String {
        format!("{KEY_PREFIX}{sha256}.gguf")
    }

    /// Idempotent bucket creation. Tolerates `BucketAlreadyOwnedByYou` and
    /// `BucketAlreadyExists` so multiple replicas can race the call on
    /// startup without errors. Mirrors the pattern used by `S3ImageStore`.
    pub async fn ensure_bucket(&self) -> Result<()> {
        use aws_sdk_s3::operation::create_bucket::CreateBucketError;
        match self.client.create_bucket().bucket(&self.bucket).send().await {
            Ok(_) => {
                tracing::info!(bucket = %self.bucket, "GGUF blob bucket created");
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
            Err(e) => Err(anyhow::anyhow!("failed to create GGUF blob bucket: {e}")),
        }
    }

    /// True iff a blob with this sha256 already exists in the bucket. Used by
    /// the install orchestrator to skip downloads when CAS already has the
    /// content. A single HEAD round-trip — no body fetched.
    pub async fn exists(&self, sha256: &str) -> Result<bool> {
        let key = Self::key_for(sha256);
        match self.client.head_object().bucket(&self.bucket).key(&key).send().await {
            Ok(_) => Ok(true),
            Err(SdkError::ServiceError(e)) if e.err().meta().code() == Some("NotFound") => {
                Ok(false)
            }
            Err(SdkError::ServiceError(e)) if e.err().meta().code() == Some("404") => Ok(false),
            // Some S3-compatible servers (Garage, MinIO older) surface 404 as
            // a non-classified `NoSuchKey`. Be permissive to keep this fast
            // path simple — any other error propagates.
            Err(SdkError::ServiceError(e)) if e.err().meta().code() == Some("NoSuchKey") => {
                Ok(false)
            }
            Err(e) => Err(anyhow::anyhow!("HEAD {key}: {e}")),
        }
    }

    /// Upload a verified GGUF file from a local path. Caller must have
    /// already streamed the source into the path **and** validated the
    /// sha256 — `BlobStore` does no rehashing.
    ///
    /// Implementation uses `ByteStream::from_path`, which the AWS SDK
    /// converts to multipart for files larger than a threshold automatically.
    /// Garage v2 advertises full S3 multipart so this works unchanged.
    pub async fn put_from_path(&self, sha256: &str, path: &Path) -> Result<()> {
        let key = Self::key_for(sha256);
        let body = ByteStream::from_path(path)
            .await
            .with_context(|| format!("open {} for upload", path.display()))?;
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(body)
            .content_type("application/octet-stream")
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("PUT {key}: {e}"))?;
        Ok(())
    }

    /// Stream the blob bytes from Garage. Caller pipes the stream to the
    /// node's Local PV (`.tmp.{uuid}` → atomic rename to `{sha256}.gguf`)
    /// and may verify sha256 incidentally.
    pub async fn get_stream(&self, sha256: &str) -> Result<ByteStream> {
        let key = Self::key_for(sha256);
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("GET {key}: {e}"))?;
        Ok(resp.body)
    }

    /// Delete a single CAS object. Used by admin manual GC for orphan blobs
    /// (`ref_count = 0`). Tolerates `NotFound` so repeated deletes are no-ops.
    pub async fn delete(&self, sha256: &str) -> Result<()> {
        let key = Self::key_for(sha256);
        match self.client.delete_object().bucket(&self.bucket).key(&key).send().await {
            Ok(_) => Ok(()),
            Err(SdkError::ServiceError(e))
                if matches!(
                    e.err().meta().code(),
                    Some("NotFound") | Some("NoSuchKey") | Some("404")
                ) =>
            {
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!("DELETE {key}: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_for_uses_prefix_and_extension() {
        let k = BlobStore::key_for("abc123");
        assert_eq!(k, "gguf-blobs/abc123.gguf");
    }

    #[test]
    fn key_prefix_constant_matches_layout() {
        assert!(KEY_PREFIX.ends_with('/'));
        assert!(BlobStore::key_for("x").starts_with(KEY_PREFIX));
    }
}
