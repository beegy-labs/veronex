//! Phase 2 wiring (Modelfile registry + CAS blob store).
//!
//! Two-tier config: env > DB > default.
//!
//! - Env wins so a docker-compose / k8s operator can pin a value (and the
//!   `app_config` row, if any, is treated as advisory).
//! - DB wins next so first-install wizards persist across restarts.
//! - Default lands last for non-required values (region, paths, etc.).
//!
//! `app_config_repo` is always wired when the master encryption key is set.
//! Phase 2 components (BlobStore / LocalPv / registries / orchestrator)
//! are only wired when the four required S3 keys (endpoint + access +
//! secret + bucket) resolve to non-empty values. Until then admin endpoints
//! return 503 and the wizard at `/v1/setup/storage` collects the config.
//!
//! Restart-required: changing storage config flips AppState components from
//! None to Some, which is not safe to do live (clients hold long-lived
//! connections through `Arc<dyn ...>`). The wizard returns
//! `{restart_required: true}` and the operator restarts; on the next boot
//! this loader picks up the new app_config rows.

use std::sync::Arc;

use anyhow::Result;
use sqlx::PgPool;

use veronex::application::ports::outbound::app_config_repository::{
    self as cfg, AppConfigEntry, AppConfigRepository,
};
use veronex::application::ports::outbound::blob_registry::BlobRegistry;
use veronex::application::ports::outbound::install_attempts_log::InstallAttemptsLog;
use veronex::application::ports::outbound::modelfile_registry::ModelfileRegistry;
use veronex::infrastructure::outbound::model_store::{
    BlobStore, InstallOrchestrator, LocalPv,
};
use veronex::infrastructure::outbound::model_store::startup_recovery;
use veronex::infrastructure::outbound::persistence::app_config_repository::PostgresAppConfigRepository;
use veronex::infrastructure::outbound::persistence::blob_registry::PostgresBlobRegistry;
use veronex::infrastructure::outbound::persistence::install_attempts_log::PostgresInstallAttemptsLog;
use veronex::infrastructure::outbound::persistence::modelfile_registry::PostgresModelfileRegistry;

/// Defaults — used only when neither env nor DB sets the key.
const DEFAULT_S3_REGION: &str = "us-east-1";
const DEFAULT_MODEL_BUCKET: &str = "veronex-models";
const DEFAULT_LOCAL_PATH: &str = "/var/lib/veronex/models";
const DEFAULT_MAX_DISK_GB: u64 = 100;

/// Wired Phase 2 components. All fields are `Option` so AppState can fall
/// through to the 503 path when storage config is missing.
pub struct ModelStoreWiring {
    pub app_config_repo: Option<Arc<dyn AppConfigRepository>>,
    pub modelfile_registry: Option<Arc<dyn ModelfileRegistry>>,
    pub blob_registry: Option<Arc<dyn BlobRegistry>>,
    pub install_attempts_log: Option<Arc<dyn InstallAttemptsLog>>,
    pub install_orchestrator: Option<InstallOrchestrator>,
    pub blob_store: Option<BlobStore>,
    pub local_pv: Option<LocalPv>,
}

impl ModelStoreWiring {
    /// All-None — used when the master encryption key isn't set so we can't
    /// even read the secret rows in `app_config`.
    fn empty() -> Self {
        Self {
            app_config_repo: None,
            modelfile_registry: None,
            blob_registry: None,
            install_attempts_log: None,
            install_orchestrator: None,
            blob_store: None,
            local_pv: None,
        }
    }
}

/// Resolved storage config in env > DB > default order. `None` for the
/// four required S3 keys means Phase 2 stays unwired.
struct ResolvedConfig {
    s3_endpoint: Option<String>,
    s3_region: String,
    s3_access_key: Option<String>,
    s3_secret_key: Option<String>,
    s3_model_bucket: String,
    local_path: String,
    max_disk_gb: u64,
}

impl ResolvedConfig {
    fn is_storage_ready(&self) -> bool {
        self.s3_endpoint.is_some()
            && self.s3_access_key.is_some()
            && self.s3_secret_key.is_some()
    }
}

/// Build Phase 2 wiring. Tolerates a missing config — returns the AppConfig
/// repo with the rest as None so admin endpoints can answer 503 and the
/// wizard handler can persist the wizard submission.
pub async fn wire(pg_pool: &PgPool, master_key: [u8; 32]) -> Result<ModelStoreWiring> {
    let app_config_repo: Arc<dyn AppConfigRepository> = Arc::new(
        PostgresAppConfigRepository::new(pg_pool.clone(), master_key),
    );

    // Pull the canonical key set in one round-trip.
    let keys: Vec<&str> = cfg::ALL_KEYS.iter().map(|(k, _)| *k).collect();
    let entries = app_config_repo.get_many(&keys).await?;
    let resolved = resolve(&entries);

    if !resolved.is_storage_ready() {
        tracing::warn!(
            "model store: S3 endpoint / access / secret not configured — \
             admin endpoints (/v1/admin/models, /v1/admin/blobs) will return 503 \
             until the operator runs `/v1/setup/storage`",
        );
        return Ok(ModelStoreWiring {
            app_config_repo: Some(app_config_repo),
            ..ModelStoreWiring::empty()
        });
    }

    // Required fields are all Some past this point.
    let endpoint = resolved.s3_endpoint.unwrap();
    let access = resolved.s3_access_key.unwrap();
    let secret = resolved.s3_secret_key.unwrap();
    let bucket = resolved.s3_model_bucket;
    let region = resolved.s3_region;

    let blob_store = build_blob_store(&endpoint, &region, &access, &secret, &bucket);

    if let Err(e) = blob_store.ensure_bucket().await {
        tracing::warn!(error = %e, "model bucket init failed (non-fatal)");
    }

    let local_pv = LocalPv::new(&resolved.local_path);
    if let Err(e) = std::fs::create_dir_all(&resolved.local_path) {
        tracing::warn!(
            path = %resolved.local_path,
            error = %e,
            "could not create model_store local path",
        );
    }

    // LocalPv's LRU sweep reads `VERONEX_MODEL_MAX_DISK_GB` from env. When
    // the operator set this through the wizard (DB-only), propagate it to
    // env so the eviction path picks up the same value the wizard recorded.
    // Safety: bootstrap runs before any worker / sweep can call LocalPv, so
    // there is no concurrent reader. Only sets when env is unset to keep
    // env-as-override semantics intact.
    if std::env::var_os("VERONEX_MODEL_MAX_DISK_GB").is_none() {
        // SAFETY: single-threaded boot phase; LocalPv has not been used yet.
        unsafe { std::env::set_var("VERONEX_MODEL_MAX_DISK_GB", resolved.max_disk_gb.to_string()) };
    }

    let modelfile_registry: Arc<dyn ModelfileRegistry> =
        Arc::new(PostgresModelfileRegistry::new(pg_pool.clone()));
    let blob_registry: Arc<dyn BlobRegistry> =
        Arc::new(PostgresBlobRegistry::new(pg_pool.clone()));
    let install_attempts_log: Arc<dyn InstallAttemptsLog> =
        Arc::new(PostgresInstallAttemptsLog::new(pg_pool.clone()));

    let orchestrator = InstallOrchestrator::new(
        modelfile_registry.clone(),
        blob_registry.clone(),
        install_attempts_log.clone(),
        blob_store.clone(),
        local_pv.clone(),
    );

    // Sweep zombie in-progress models and stale .tmp files. A previous
    // process could have crashed mid-install; without this the row would
    // stay Downloading forever and the file would leak.
    match startup_recovery::run(
        modelfile_registry.as_ref(),
        install_attempts_log.as_ref(),
        &local_pv,
    )
    .await
    {
        Ok((zombies, tmp)) => {
            if zombies > 0 || tmp > 0 {
                tracing::info!(
                    zombies, tmp_cleaned = tmp,
                    "model store startup recovery completed",
                );
            }
        }
        Err(e) => tracing::warn!(error = %e, "model store startup recovery failed"),
    }

    tracing::info!(
        endpoint = %endpoint,
        bucket = %bucket,
        local_path = %resolved.local_path,
        max_disk_gb = resolved.max_disk_gb,
        "model store ready",
    );

    Ok(ModelStoreWiring {
        app_config_repo: Some(app_config_repo),
        modelfile_registry: Some(modelfile_registry),
        blob_registry: Some(blob_registry),
        install_attempts_log: Some(install_attempts_log),
        install_orchestrator: Some(orchestrator),
        blob_store: Some(blob_store),
        local_pv: Some(local_pv),
    })
}

/// Best-effort wire when the master key isn't set — returns all None so the
/// caller can still build AppState. Storage features are disabled.
pub fn unconfigured() -> ModelStoreWiring {
    ModelStoreWiring::empty()
}

fn resolve(entries: &[AppConfigEntry]) -> ResolvedConfig {
    let by_key: std::collections::HashMap<&str, &str> = entries
        .iter()
        .filter_map(|e| {
            e.value
                .as_deref()
                .filter(|v| !v.is_empty())
                .map(|v| (e.key.as_str(), v))
        })
        .collect();

    let env_or_db = |env_key: &str, cfg_key: &str| -> Option<String> {
        std::env::var(env_key)
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| by_key.get(cfg_key).map(|s| s.to_string()))
    };

    let s3_endpoint = env_or_db("S3_ENDPOINT", cfg::KEY_S3_ENDPOINT);
    let s3_region = env_or_db("S3_REGION", cfg::KEY_S3_REGION)
        .unwrap_or_else(|| DEFAULT_S3_REGION.to_string());
    let s3_access_key = env_or_db("S3_ACCESS_KEY", cfg::KEY_S3_ACCESS_KEY);
    let s3_secret_key = env_or_db("S3_SECRET_KEY", cfg::KEY_S3_SECRET_KEY);
    let s3_model_bucket = env_or_db("VERONEX_MODEL_BUCKET", cfg::KEY_S3_MODEL_BUCKET)
        .unwrap_or_else(|| DEFAULT_MODEL_BUCKET.to_string());
    let local_path = env_or_db("VERONEX_MODEL_LOCAL_PATH", cfg::KEY_MODEL_LOCAL_PATH)
        .unwrap_or_else(|| DEFAULT_LOCAL_PATH.to_string());
    let max_disk_gb = env_or_db("VERONEX_MODEL_MAX_DISK_GB", cfg::KEY_MODEL_MAX_DISK_GB)
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_DISK_GB);

    ResolvedConfig {
        s3_endpoint,
        s3_region,
        s3_access_key,
        s3_secret_key,
        s3_model_bucket,
        local_path,
        max_disk_gb,
    }
}

fn build_blob_store(
    endpoint: &str,
    region: &str,
    access: &str,
    secret: &str,
    bucket: &str,
) -> BlobStore {
    use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
    let creds = Credentials::new(access, secret, None, None, "veronex");
    let s3_config = aws_sdk_s3::Config::builder()
        .endpoint_url(endpoint)
        .region(Region::new(region.to_string()))
        .credentials_provider(creds)
        .force_path_style(true)
        .behavior_version(BehaviorVersion::latest())
        .build();
    let client = aws_sdk_s3::Client::from_conf(s3_config);
    BlobStore::new(client, bucket)
}
