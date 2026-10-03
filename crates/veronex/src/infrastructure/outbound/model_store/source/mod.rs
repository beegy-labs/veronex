//! Source adapters — origin of a GGUF blob being registered.
//!
//! Phase 2 supports four source kinds; this slice ships the HuggingFace
//! adapter, the most common path. Upload / URL / S3-pointer follow in later
//! slices.
//!
//! All adapters speak the [`ModelSource`] trait so the install orchestrator
//! treats them uniformly:
//!
//! 1. Optionally pre-resolve sha256 (HF advertises LFS OID, S3 has ETag).
//!    A [`Some`] result lets the orchestrator skip the download entirely
//!    when CAS already has the blob.
//! 2. If the orchestrator needs the bytes, it calls [`stream_blob`] and
//!    pipes them into Local PV with a streaming sha256 hasher.
//!
//! [`stream_blob`]: ModelSource::stream_blob

pub mod hf_source;
pub mod s3_pointer_source;
pub mod upload_source;
pub mod url_source;

pub use hf_source::HfSource;
pub use s3_pointer_source::S3PointerSource;
pub use upload_source::UploadSource;
pub use url_source::UrlSource;

use anyhow::Result;
use bytes::Bytes;
use futures::stream::BoxStream;

/// Identity of a GGUF blob in the CAS — the file's sha256, hex-encoded.
pub type Sha256Hex = String;

/// Stream of bytes from a source. `'static` is required because the
/// orchestrator owns the stream after the adapter returns.
pub type ByteStream = BoxStream<'static, Result<Bytes>>;

/// Trait for a "where does this GGUF live?" adapter.
///
/// Implementations are responsible for HTTP/multipart concerns and a single
/// canonical [`source_spec`] JSON value that the registry persists alongside
/// the model row (so a future PATCH can re-execute the same fetch).
///
/// Implementations should NOT hash the body themselves — the install
/// orchestrator hashes once on the way to Local PV. Adapters only OPTIONALLY
/// pre-resolve sha256 when their backend exposes it cheaply.
///
/// [`source_spec`]: ModelSource::source_spec
#[async_trait::async_trait]
pub trait ModelSource: Send + Sync {
    /// Try to learn the sha256 without downloading the body. `Ok(Some(_))`
    /// lets the orchestrator skip the download if the CAS already has the
    /// blob; `Ok(None)` means "I don't know — please download and compute".
    ///
    /// HF: returns `lfs.oid` (which is sha256). Upload: always `None`. URL:
    /// `Some` only if the server returns a `sha256:...` ETag. S3 pointer:
    /// `Some` if `x-amz-meta-sha256` is set on the object.
    async fn resolve_sha256(&self) -> Result<Option<Sha256Hex>>;

    /// Begin streaming the body. Called only when `resolve_sha256` either
    /// returned `None` or returned a hash that the CAS does not yet have.
    async fn stream_blob(&self) -> Result<ByteStream>;

    /// Total expected bytes if known (HEAD `Content-Length`, HF tree size).
    /// Used for progress estimation; `None` is acceptable.
    fn total_bytes(&self) -> Option<u64>;

    /// Canonical JSON identity of this source. Persisted in
    /// `veronex_models.source_spec` so a future PATCH can re-execute the
    /// same fetch deterministically. Shape examples:
    ///
    /// - HF:         `{"type":"hf","repo":"...","filename":"...","revision":"..."}`
    /// - upload:     `{"type":"upload","uploaded_at":"...","uploaded_by":"..."}`
    /// - url:        `{"type":"url","url":"...","auth_header_secret":"..."}`
    /// - s3_pointer: `{"type":"s3_pointer","s3_key":"..."}`
    fn source_spec(&self) -> serde_json::Value;
}
