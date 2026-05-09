//! Install-state gate for inference dispatch.
//!
//! Looks up a Modelfile by `model_id` (precise) or `family` (resolves to the
//! row marked `is_default = true`) and maps the result to one of four
//! buckets the HTTP router can map to status codes:
//!
//! - [`Gate::NotRegistered`] → 404 (legacy fall-through expected)
//! - [`Gate::InProgress`] → 503 + Retry-After + status_url
//! - [`Gate::Failed`] → 409 + retry_url
//! - [`Gate::Ready`] → proceed (carries the resolved blob + Modelfile ref)
//!
//! Phase 2 ships this gate as a standalone utility. Phase 3 wires it into
//! the inference handlers (OpenAI / Gemini compat) once the
//! ProcessManager owns the `model_id → llama-server provider_id` mapping.

use std::sync::Arc;

use anyhow::Result;

use crate::application::ports::outbound::modelfile_registry::ModelfileRegistry;
use crate::domain::entities::{ErrorKind, InstallStatus, VeronexModel};

/// Outcome of the gate lookup. Each variant maps to one HTTP response shape.
#[derive(Debug, Clone)]
pub enum Gate {
    /// No Modelfile is registered for the supplied identifier. The caller
    /// SHOULD fall through to provider routing (llama_server / Gemini) so
    /// Phase 2 stays non-disruptive.
    NotRegistered,
    /// The Modelfile exists and is currently installing. Caller returns
    /// 503 with `Retry-After` plus the install/stream URL the admin UI
    /// already consumes.
    InProgress { model: VeronexModel },
    /// The Modelfile exists but its last install attempt failed. Caller
    /// returns 409 with the `retry` URL so an admin can fix the cause and
    /// re-run.
    Failed {
        model: VeronexModel,
        last_error_kind: Option<ErrorKind>,
        last_error_message: Option<String>,
    },
    /// Ready for dispatch — caller proceeds.
    Ready { model: VeronexModel },
}

impl Gate {
    /// HTTP-status mapping is fixed by policy; expose it as a method so the
    /// handlers don't reimplement it inconsistently.
    pub fn http_status(&self) -> u16 {
        match self {
            Self::NotRegistered => 404,
            Self::InProgress { .. } => 503,
            Self::Failed { .. } => 409,
            Self::Ready { .. } => 200,
        }
    }
}

/// Resolve a Modelfile by `model_name` and bucket the result.
///
/// Accepts both precise `"family:quantization"` and bare `"family"` (which
/// hits the family-default row when one exists). The dual-shape lookup
/// matches the Phase 2 design: a client can ask for `qwen3-coder` and get
/// whichever quantization the operator promoted.
pub async fn resolve_gate(
    registry: &Arc<dyn ModelfileRegistry>,
    model_name: &str,
) -> Result<Gate> {
    // Precise lookup first.
    if let Some(model) = registry.get(model_name).await? {
        return Ok(classify(model));
    }
    // Fall back to family-default if the request omitted `:quantization`
    // (and therefore couldn't possibly match a precise model_id either).
    if !model_name.contains(':') {
        if let Some(model) = registry.get_default(model_name).await? {
            return Ok(classify(model));
        }
    }
    Ok(Gate::NotRegistered)
}

fn classify(model: VeronexModel) -> Gate {
    match model.install_status {
        InstallStatus::Ready => Gate::Ready { model },
        InstallStatus::Failed => Gate::Failed {
            last_error_kind: model.last_error_kind.as_deref().and_then(parse_kind),
            last_error_message: model.last_error_message.clone(),
            model,
        },
        InstallStatus::Pending
        | InstallStatus::Downloading
        | InstallStatus::Uploading
        | InstallStatus::Verifying => Gate::InProgress { model },
    }
}

fn parse_kind(s: &str) -> Option<ErrorKind> {
    // The string form is the SSOT in the DB; map back to the enum so the
    // 409 body can render `retryable` consistently with the Phase 2 model.
    Some(match s {
        "network" => ErrorKind::Network,
        "rate_limit" => ErrorKind::RateLimit,
        "s3_error" => ErrorKind::S3Error,
        "sha256_mismatch" => ErrorKind::Sha256Mismatch,
        "timeout" => ErrorKind::Timeout,
        "startup_recovery" => ErrorKind::StartupRecovery,
        "source_404" => ErrorKind::Source404,
        "auth" => ErrorKind::Auth,
        "disk_full" => ErrorKind::DiskFull,
        "invalid_gguf" => ErrorKind::InvalidGguf,
        "quantization_unsupported" => ErrorKind::QuantizationUnsupported,
        _ => ErrorKind::Unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::outbound::modelfile_registry::{
        ListFilter, ModelfilePatch,
    };
    use chrono::Utc;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory registry stub for gate tests. Only the handful of methods
    /// the gate exercises are implemented; the rest panic so a test that
    /// drifts and starts using them gets a loud failure.
    struct InMemRegistry {
        by_id: Mutex<HashMap<String, VeronexModel>>,
        by_default: Mutex<HashMap<String, VeronexModel>>,
    }

    impl InMemRegistry {
        fn new() -> Self {
            Self {
                by_id: Mutex::new(HashMap::new()),
                by_default: Mutex::new(HashMap::new()),
            }
        }

        fn insert(&self, m: VeronexModel) {
            if m.is_default {
                self.by_default
                    .lock()
                    .unwrap()
                    .insert(m.family.clone(), m.clone());
            }
            self.by_id.lock().unwrap().insert(m.model_id.clone(), m);
        }
    }

    #[async_trait::async_trait]
    impl ModelfileRegistry for InMemRegistry {
        async fn create(&self, m: &VeronexModel) -> Result<()> {
            self.insert(m.clone());
            Ok(())
        }
        async fn get(&self, model_id: &str) -> Result<Option<VeronexModel>> {
            Ok(self.by_id.lock().unwrap().get(model_id).cloned())
        }
        async fn get_default(&self, family: &str) -> Result<Option<VeronexModel>> {
            Ok(self.by_default.lock().unwrap().get(family).cloned())
        }
        async fn list(&self, _: &ListFilter) -> Result<Vec<VeronexModel>> {
            Ok(self.by_id.lock().unwrap().values().cloned().collect())
        }
        async fn patch(&self, _: &str, _: &ModelfilePatch) -> Result<bool> {
            unimplemented!("not used by gate tests")
        }
        async fn update_install_status(
            &self,
            _: &str,
            _: InstallStatus,
            _: Option<ErrorKind>,
            _: Option<&str>,
            _: Option<chrono::DateTime<Utc>>,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn swap_blob(&self, _: &str, _: &str, _: &str) -> Result<()> {
            unimplemented!()
        }
        async fn promote_default(&self, _: &str) -> Result<()> {
            unimplemented!()
        }
        async fn delete(&self, _: &str) -> Result<bool> {
            unimplemented!()
        }
    }

    fn fake_model(model_id: &str, family: &str, status: InstallStatus, is_default: bool) -> VeronexModel {
        VeronexModel {
            model_id: model_id.into(),
            family: family.into(),
            quantization: model_id.split(':').nth(1).unwrap_or("q4").into(),
            blob_sha256: "0".repeat(64),
            display_name: None,
            source_spec: serde_json::json!({}),
            runtime: serde_json::json!({}),
            defaults: serde_json::json!({}),
            chat_template: serde_json::json!({}),
            stop_tokens: None,
            system_prompt: None,
            is_default,
            install_status: status,
            last_error_kind: None,
            last_error_message: None,
            last_attempt_at: None,
            tags: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn precise_lookup_returns_ready_for_ready_status() {
        let reg = InMemRegistry::new();
        reg.insert(fake_model("qwen:q4", "qwen", InstallStatus::Ready, false));
        let arc: Arc<dyn ModelfileRegistry> = Arc::new(reg);
        let gate = resolve_gate(&arc, "qwen:q4").await.unwrap();
        assert!(matches!(gate, Gate::Ready { .. }));
        assert_eq!(gate.http_status(), 200);
    }

    #[tokio::test]
    async fn in_progress_statuses_all_bucket_to_inprogress() {
        for s in [
            InstallStatus::Pending,
            InstallStatus::Downloading,
            InstallStatus::Uploading,
            InstallStatus::Verifying,
        ] {
            let reg = InMemRegistry::new();
            reg.insert(fake_model("x:q4", "x", s, false));
            let arc: Arc<dyn ModelfileRegistry> = Arc::new(reg);
            let gate = resolve_gate(&arc, "x:q4").await.unwrap();
            assert!(matches!(gate, Gate::InProgress { .. }), "for {s:?}");
            assert_eq!(gate.http_status(), 503);
        }
    }

    #[tokio::test]
    async fn failed_status_bucket_carries_error_metadata() {
        let mut m = fake_model("x:q4", "x", InstallStatus::Failed, false);
        m.last_error_kind = Some("source_404".into());
        m.last_error_message = Some("404 not found".into());
        let reg = InMemRegistry::new();
        reg.insert(m);
        let arc: Arc<dyn ModelfileRegistry> = Arc::new(reg);
        let gate = resolve_gate(&arc, "x:q4").await.unwrap();
        match gate {
            Gate::Failed { last_error_kind, last_error_message, .. } => {
                assert_eq!(last_error_kind, Some(ErrorKind::Source404));
                assert_eq!(last_error_message.as_deref(), Some("404 not found"));
            }
            _ => panic!("expected Failed"),
        }
    }

    #[tokio::test]
    async fn family_lookup_resolves_to_default_when_no_quantization() {
        let reg = InMemRegistry::new();
        reg.insert(fake_model("qwen:q4", "qwen", InstallStatus::Ready, true));
        reg.insert(fake_model("qwen:q8", "qwen", InstallStatus::Ready, false));
        let arc: Arc<dyn ModelfileRegistry> = Arc::new(reg);

        // Bare family resolves to the default row.
        let gate = resolve_gate(&arc, "qwen").await.unwrap();
        match gate {
            Gate::Ready { model } => assert_eq!(model.model_id, "qwen:q4"),
            _ => panic!("expected Ready"),
        }
    }

    #[tokio::test]
    async fn precise_with_colon_does_not_fall_back_to_default() {
        let reg = InMemRegistry::new();
        reg.insert(fake_model("qwen:q4", "qwen", InstallStatus::Ready, true));
        let arc: Arc<dyn ModelfileRegistry> = Arc::new(reg);

        // Wrong precise model_id (with colon) must NOT silently fall through
        // to the default.
        let gate = resolve_gate(&arc, "qwen:q8").await.unwrap();
        assert!(matches!(gate, Gate::NotRegistered));
        assert_eq!(gate.http_status(), 404);
    }

    #[tokio::test]
    async fn unknown_returns_not_registered() {
        let reg = InMemRegistry::new();
        let arc: Arc<dyn ModelfileRegistry> = Arc::new(reg);
        let gate = resolve_gate(&arc, "unknown").await.unwrap();
        assert!(matches!(gate, Gate::NotRegistered));
    }
}
