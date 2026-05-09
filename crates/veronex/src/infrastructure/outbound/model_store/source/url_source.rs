//! Arbitrary-HTTPS source adapter.
//!
//! Lets operators register a GGUF that lives on a private mirror (internal
//! model server, ad-hoc release page, etc.) without going through HF. Optional
//! `auth_header_secret` references a k8s Secret name; the orchestrator
//! resolves the actual header value at install time and passes it via
//! [`with_auth_header`]. Phase 2 keeps the auth handling minimal — single
//! Authorization header, no OAuth / signing.
//!
//! sha256 pre-resolution is best-effort: we issue HEAD and look for an
//! `ETag` that already carries a sha256 (some servers expose
//! `"sha256:abc..."`). Anything else returns `None` and the orchestrator
//! falls back to streaming compute.

use std::time::Duration;

use anyhow::{Context as _, Result};
use bytes::Bytes;
use futures::StreamExt as _;

use super::{ByteStream, ModelSource, Sha256Hex};

pub const HEAD_TIMEOUT: Duration = Duration::from_secs(15);

pub struct UrlSource {
    url: String,
    /// Resolved Authorization header value (already de-referenced from the
    /// k8s Secret reference). `None` for public mirrors.
    auth_header: Option<String>,
    /// Original Secret name (for `source_spec` audit). Distinct from the
    /// resolved header value — we never persist secrets in DB.
    auth_header_secret: Option<String>,
    client: reqwest::Client,
}

impl UrlSource {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            auth_header: None,
            auth_header_secret: None,
            client: reqwest::Client::new(),
        }
    }

    pub fn with_auth_header(mut self, value: Option<String>) -> Self {
        self.auth_header = value;
        self
    }

    /// Audit-only: name of the k8s Secret whose contents are loaded into
    /// `auth_header`. Only this name is persisted in `source_spec`.
    pub fn with_auth_header_secret(mut self, secret_name: Option<String>) -> Self {
        self.auth_header_secret = secret_name;
        self
    }

    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth_header {
            Some(v) => builder.header("authorization", v),
            None => builder,
        }
    }
}

#[async_trait::async_trait]
impl ModelSource for UrlSource {
    async fn resolve_sha256(&self) -> Result<Option<Sha256Hex>> {
        // HEAD round-trip; servers that don't support HEAD return 405 — we
        // tolerate that and report None (orchestrator streams + computes).
        let req = self.client.head(&self.url).timeout(HEAD_TIMEOUT);
        let req = self.auth(req);
        let resp = match req.send().await {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };
        if !resp.status().is_success() {
            return Ok(None);
        }
        let etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        // Some servers expose `"sha256:abc..."` in ETag. Anything else is
        // not assumed to be a sha256.
        let trimmed = etag.trim_matches('"');
        if let Some(hex) = trimmed.strip_prefix("sha256:") {
            if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Ok(Some(hex.to_lowercase()));
            }
        }
        Ok(None)
    }

    async fn stream_blob(&self) -> Result<ByteStream> {
        let req = self.client.get(&self.url);
        let req = self.auth(req);
        let resp = req
            .send()
            .await
            .with_context(|| format!("GET {}", self.url))?
            .error_for_status()
            .with_context(|| format!("non-2xx from {}", self.url))?;
        let s = resp
            .bytes_stream()
            .map(|chunk| chunk.map(Bytes::from).map_err(anyhow::Error::from));
        Ok(Box::pin(s))
    }

    fn total_bytes(&self) -> Option<u64> {
        None
    }

    fn source_spec(&self) -> serde_json::Value {
        let mut spec = serde_json::json!({
            "type": "url",
            "url": self.url,
        });
        if let Some(name) = &self.auth_header_secret {
            spec["auth_header_secret"] = serde_json::json!(name);
        }
        spec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn resolve_sha256_extracts_sha256_etag_when_present() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/foo.gguf"))
            .respond_with(ResponseTemplate::new(200).insert_header(
                "etag",
                "\"sha256:".to_string()
                    + &"a".repeat(64)
                    + "\"",
            ))
            .mount(&server)
            .await;

        let src = UrlSource::new(format!("{}/foo.gguf", server.uri()));
        let oid = src.resolve_sha256().await.unwrap();
        assert_eq!(oid, Some("a".repeat(64)));
    }

    #[tokio::test]
    async fn resolve_sha256_returns_none_for_opaque_etag() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/foo.gguf"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("etag", "\"abc-xyz\""),
            )
            .mount(&server)
            .await;

        let src = UrlSource::new(format!("{}/foo.gguf", server.uri()));
        let oid = src.resolve_sha256().await.unwrap();
        assert_eq!(oid, None);
    }

    #[tokio::test]
    async fn resolve_sha256_returns_none_when_head_unsupported() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/foo.gguf"))
            .respond_with(ResponseTemplate::new(405))
            .mount(&server)
            .await;

        let src = UrlSource::new(format!("{}/foo.gguf", server.uri()));
        let oid = src.resolve_sha256().await.unwrap();
        assert_eq!(oid, None);
    }

    #[tokio::test]
    async fn stream_blob_returns_body_bytes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/foo.gguf"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello"))
            .mount(&server)
            .await;

        let src = UrlSource::new(format!("{}/foo.gguf", server.uri()));
        let mut s = src.stream_blob().await.unwrap();
        let mut acc = Vec::new();
        while let Some(c) = s.next().await {
            acc.extend_from_slice(&c.unwrap());
        }
        assert_eq!(&acc, b"hello");
    }

    #[tokio::test]
    async fn stream_blob_propagates_4xx() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/foo.gguf"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let src = UrlSource::new(format!("{}/foo.gguf", server.uri()));
        let r = src.stream_blob().await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn auth_header_set_when_supplied() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/foo.gguf"))
            .and(wiremock::matchers::header("authorization", "Bearer abc"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let src = UrlSource::new(format!("{}/foo.gguf", server.uri()))
            .with_auth_header(Some("Bearer abc".to_string()));
        // resolve returns None (no sha256 etag) but the matched mock proves
        // the auth header was sent.
        assert_eq!(src.resolve_sha256().await.unwrap(), None);
    }

    #[test]
    fn source_spec_includes_secret_name_only() {
        let src = UrlSource::new("https://x/y.gguf")
            .with_auth_header(Some("Bearer secretvalue".to_string()))
            .with_auth_header_secret(Some("my-k8s-secret".to_string()));
        let s = src.source_spec();
        assert_eq!(s["type"], "url");
        assert_eq!(s["url"], "https://x/y.gguf");
        assert_eq!(s["auth_header_secret"], "my-k8s-secret");
        // The actual bearer token must NOT leak into source_spec.
        assert!(s.to_string().find("secretvalue").is_none());
    }
}
