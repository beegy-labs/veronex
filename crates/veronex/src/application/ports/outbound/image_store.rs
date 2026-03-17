use async_trait::async_trait;
use uuid::Uuid;

/// Object storage port for inference job images (WebP thumbnails).
///
/// Images are stored as `images/{job_id}/{index}.webp` (full) and
/// `images/{job_id}/{index}_thumb.webp` (128px thumbnail).
#[async_trait]
pub trait ImageStore: Send + Sync {
    /// Upload a full-size WebP and its thumbnail for a job image.
    /// Returns the S3 keys: `(full_key, thumb_key)`.
    async fn put(
        &self,
        job_id: Uuid,
        index: usize,
        webp: &[u8],
        thumb: &[u8],
    ) -> anyhow::Result<(String, String)>;

    /// Convert a base64-encoded image (JPEG/PNG) to WebP and upload full + thumbnail.
    ///
    /// Encapsulates the conversion+upload pipeline so that the application layer
    /// does not need to import infrastructure conversion utilities.
    /// Returns the S3 keys: `(full_key, thumb_key)`.
    async fn put_base64(
        &self,
        job_id: Uuid,
        index: usize,
        b64: &str,
    ) -> anyhow::Result<(String, String)>;

    /// Generate a presigned or direct URL for the given object key.
    fn url(&self, key: &str) -> String;
}
