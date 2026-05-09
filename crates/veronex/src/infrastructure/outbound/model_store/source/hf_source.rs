//! HuggingFace Hub source adapter.
//!
//! Resolves a `(repo, filename, revision)` triple to a streaming download
//! URL plus, when possible, a pre-computed sha256 (HF LFS OID).
//!
//! HF stores GGUF models on Git LFS. The LFS pointer's `oid` field is the
//! sha256 of the actual file bytes — which is precisely the CAS key Veronex
//! uses. Returning that `oid` from [`resolve_sha256`] lets the install
//! orchestrator skip the download entirely if another model already
//! registered the same file.
//!
//! Files moved to HF Xet (the newer chunked CAS) expose a `xet.hash` field
//! instead. For Phase 2 we read `lfs.oid` only and fall back to streaming
//! compute when the field is absent — Xet support can ride later without
//! changing the orchestrator.
//!
//! [`resolve_sha256`]: super::ModelSource::resolve_sha256

use std::time::Duration;

use anyhow::{Context as _, Result};
use bytes::Bytes;
use futures::StreamExt as _;
use serde::Deserialize;

use super::{ByteStream, ModelSource, Sha256Hex};

/// Default API host. Operators in private regions may override via
/// `HF_HUB_ENDPOINT`.
pub const DEFAULT_HF_ENDPOINT: &str = "https://huggingface.co";

/// Per-call timeout for the metadata round-trip. The bytes stream itself uses
/// no overall timeout (large GGUFs run for minutes); the orchestrator owns
/// stall detection.
pub const METADATA_TIMEOUT: Duration = Duration::from_secs(30);

/// HuggingFace source.
///
/// Holds enough state to (a) hit the tree-metadata endpoint and read the LFS
/// OID, (b) build the streaming download URL. `revision` should be a commit
/// SHA when reproducibility matters; `"main"` is acceptable for "latest".
pub struct HfSource {
    repo: String,
    filename: String,
    revision: String,
    endpoint: String,
    /// Optional bearer token for private repos / higher rate limits.
    token: Option<String>,
    client: reqwest::Client,
}

impl HfSource {
    /// Builder. Defaults endpoint to [`DEFAULT_HF_ENDPOINT`] and revision to
    /// `"main"`. Pass an explicit revision (commit SHA) for reproducible
    /// installs.
    pub fn new(repo: impl Into<String>, filename: impl Into<String>) -> Self {
        Self {
            repo: repo.into(),
            filename: filename.into(),
            revision: "main".to_string(),
            endpoint: DEFAULT_HF_ENDPOINT.to_string(),
            token: None,
            client: reqwest::Client::new(),
        }
    }

    pub fn with_revision(mut self, revision: impl Into<String>) -> Self {
        self.revision = revision.into();
        self
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    pub fn with_token(mut self, token: Option<String>) -> Self {
        self.token = token;
        self
    }

    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    /// Build the streaming download URL.
    ///
    /// Format: `{endpoint}/{repo}/resolve/{revision}/{filename}`. The
    /// `resolve` route follows LFS pointers transparently — the response
    /// body is the actual GGUF bytes (or a 302 to S3-backed CDN).
    fn download_url(&self) -> String {
        format!(
            "{}/{}/resolve/{}/{}",
            self.endpoint.trim_end_matches('/'),
            self.repo,
            self.revision,
            self.filename,
        )
    }

    /// Build the tree metadata URL used to discover the LFS OID.
    ///
    /// Format: `{endpoint}/api/models/{repo}/tree/{revision}?recursive=true`.
    fn tree_url(&self) -> String {
        format!(
            "{}/api/models/{}/tree/{}?recursive=true",
            self.endpoint.trim_end_matches('/'),
            self.repo,
            self.revision,
        )
    }

    fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(t) => builder.bearer_auth(t),
            None => builder,
        }
    }
}

// ── HF tree response shape ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct TreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    #[serde(default)]
    lfs: Option<LfsInfo>,
}

#[derive(Deserialize)]
struct LfsInfo {
    /// SHA-256 of the file body (hex). Same value as the CAS sha256.
    oid: String,
    #[serde(default)]
    #[allow(dead_code)] // we read `oid`; size is available for future progress hooks
    size: Option<u64>,
}

#[async_trait::async_trait]
impl ModelSource for HfSource {
    async fn resolve_sha256(&self) -> Result<Option<Sha256Hex>> {
        let url = self.tree_url();
        let req = self.client.get(&url).timeout(METADATA_TIMEOUT);
        let req = self.auth(req);
        let resp = req
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("non-2xx from {url}"))?;
        let entries: Vec<TreeEntry> = resp
            .json()
            .await
            .with_context(|| "parse HF tree JSON")?;

        // Match by exact path. HF tree returns paths relative to repo root.
        let entry = entries
            .into_iter()
            .find(|e| e.kind == "file" && e.path == self.filename);

        // Non-LFS files (small) have no `lfs` field — sha256 is unknown until
        // download. That's fine; the orchestrator falls back to streaming.
        Ok(entry.and_then(|e| e.lfs).map(|l| l.oid))
    }

    async fn stream_blob(&self) -> Result<ByteStream> {
        let url = self.download_url();
        let req = self.client.get(&url);
        let req = self.auth(req);
        let resp = req
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("non-2xx from {url}"))?;
        let s = resp
            .bytes_stream()
            .map(|chunk| chunk.map(Bytes::from).map_err(anyhow::Error::from));
        Ok(Box::pin(s))
    }

    fn total_bytes(&self) -> Option<u64> {
        // Without a separate HEAD round-trip we don't know up-front. The
        // orchestrator can fold this in later from the tree metadata it
        // already fetched in `resolve_sha256` — kept simple here.
        None
    }

    fn source_spec(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "hf",
            "repo": self.repo,
            "filename": self.filename,
            "revision": self.revision,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn tree_response(filename: &str, oid: Option<&str>, size: u64) -> serde_json::Value {
        let mut entry = serde_json::json!({
            "type": "file",
            "path": filename,
            "size": size,
        });
        if let Some(oid) = oid {
            entry["lfs"] = serde_json::json!({ "oid": oid, "size": size });
        }
        serde_json::json!([entry])
    }

    #[tokio::test]
    async fn resolve_sha256_returns_lfs_oid_for_lfs_file() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/models/Qwen/Qwen3-Coder-7B-GGUF/tree/main"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(tree_response(
                    "qwen3-coder-7b-q4_k_m.gguf",
                    Some("abc123abc123"),
                    4_500_000_000,
                )),
            )
            .mount(&server)
            .await;

        let src = HfSource::new("Qwen/Qwen3-Coder-7B-GGUF", "qwen3-coder-7b-q4_k_m.gguf")
            .with_endpoint(server.uri());
        let oid = src.resolve_sha256().await.unwrap();
        assert_eq!(oid, Some("abc123abc123".to_string()));
    }

    #[tokio::test]
    async fn resolve_sha256_returns_none_for_non_lfs_file() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/models/owner/repo/tree/main"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(tree_response("README.md", None, 4096)),
            )
            .mount(&server)
            .await;

        let src = HfSource::new("owner/repo", "README.md").with_endpoint(server.uri());
        let oid = src.resolve_sha256().await.unwrap();
        assert_eq!(oid, None);
    }

    #[tokio::test]
    async fn resolve_sha256_returns_none_when_filename_absent() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/models/owner/repo/tree/main"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(tree_response(
                    "other-file.gguf",
                    Some("zzz"),
                    1,
                )),
            )
            .mount(&server)
            .await;

        let src = HfSource::new("owner/repo", "missing.gguf").with_endpoint(server.uri());
        let oid = src.resolve_sha256().await.unwrap();
        assert_eq!(oid, None);
    }

    #[tokio::test]
    async fn resolve_sha256_propagates_4xx() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/models/owner/repo/tree/main"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let src = HfSource::new("owner/repo", "x.gguf").with_endpoint(server.uri());
        let r = src.resolve_sha256().await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn stream_blob_yields_bytes_in_order() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/owner/repo/resolve/main/x.gguf"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello-world"))
            .mount(&server)
            .await;

        let src = HfSource::new("owner/repo", "x.gguf").with_endpoint(server.uri());
        let mut s = src.stream_blob().await.unwrap();
        let mut collected = Vec::new();
        while let Some(chunk) = s.next().await {
            collected.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(&collected, b"hello-world");
    }

    #[tokio::test]
    async fn stream_blob_propagates_4xx() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/owner/repo/resolve/main/x.gguf"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let src = HfSource::new("owner/repo", "x.gguf").with_endpoint(server.uri());
        let r = src.stream_blob().await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn auth_header_set_when_token_present() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/models/private/repo/tree/main"))
            .and(wiremock::matchers::header("authorization", "Bearer secret"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(tree_response(
                    "x.gguf",
                    Some("def"),
                    1,
                )),
            )
            .mount(&server)
            .await;

        let src = HfSource::new("private/repo", "x.gguf")
            .with_endpoint(server.uri())
            .with_token(Some("secret".to_string()));
        let oid = src.resolve_sha256().await.unwrap();
        assert_eq!(oid, Some("def".to_string()));
    }

    #[test]
    fn source_spec_carries_canonical_fields() {
        let src = HfSource::new("owner/repo", "x.gguf").with_revision("abc1234");
        let s = src.source_spec();
        assert_eq!(s["type"], "hf");
        assert_eq!(s["repo"], "owner/repo");
        assert_eq!(s["filename"], "x.gguf");
        assert_eq!(s["revision"], "abc1234");
    }

    #[test]
    fn url_construction_uses_revision_and_endpoint() {
        let src = HfSource::new("owner/repo", "x.gguf")
            .with_endpoint("https://hf.example/")
            .with_revision("v1");
        assert_eq!(src.download_url(), "https://hf.example/owner/repo/resolve/v1/x.gguf");
        assert_eq!(
            src.tree_url(),
            "https://hf.example/api/models/owner/repo/tree/v1?recursive=true"
        );
    }
}
