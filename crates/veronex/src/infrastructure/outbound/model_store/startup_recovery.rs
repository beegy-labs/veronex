#![allow(clippy::collapsible_if, clippy::let_unit_value, clippy::unused_unit)]

//! Startup recovery — sweep zombie installs left behind by an app crash.
//!
//! Any Modelfile whose `install_status` is one of the in-progress states
//! at boot time corresponds to a previous process that died mid-install.
//! There is no in-memory continuation we can resume — the source stream is
//! gone, the AWS SDK multipart context is gone, and any `.tmp.*` file in
//! Local PV would have an unknown sha256.
//!
//! Policy: mark every zombie row as `failed` with `error_kind =
//! StartupRecovery`. Admin re-clicks `[Retry]` (no automatic retry — the
//! repository's policy from day one).
//!
//! In addition the sweep removes orphan `.tmp.*` files in `blobs/` that
//! exceed a stale threshold so they don't leak disk forever. Active
//! installs (post-recovery) reclaim the directory cleanly.

use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use tokio::fs;

use crate::application::ports::outbound::install_attempts_log::{
    AttemptFailure, InstallAttemptsLog,
};
use crate::application::ports::outbound::modelfile_registry::{
    ListFilter, ModelfileRegistry,
};
use crate::domain::entities::{AttemptStage, ErrorKind, InstallStatus};

use super::local_pv::{LocalPv, BLOBS_SUBDIR};

/// `.tmp.*` files older than this are considered orphaned by a previous
/// crash and removed.
pub const TMP_STALE_AFTER: Duration = Duration::from_secs(10 * 60);

/// Run the sweep once. Idempotent — safe to call on every startup.
///
/// Returns `(model_zombies_failed, tmp_files_cleaned)` for logging.
pub async fn run(
    models: &dyn ModelfileRegistry,
    attempts: &dyn InstallAttemptsLog,
    local_pv: &LocalPv,
) -> Result<(u32, u32)> {
    let zombies = mark_zombies_failed(models, attempts).await?;
    let cleaned = clean_orphan_tmp(local_pv).await.unwrap_or(0);
    Ok((zombies, cleaned))
}

/// For each in-progress Modelfile, mark its row failed and close the latest
/// attempt row with `error_kind = StartupRecovery`.
///
/// We list each in-progress status separately so the registry's filter is a
/// simple equality match (no IN clause needed). Three round-trips total —
/// fine for boot.
async fn mark_zombies_failed(
    models: &dyn ModelfileRegistry,
    _attempts: &dyn InstallAttemptsLog,
) -> Result<u32> {
    let mut count: u32 = 0;
    for status in [
        InstallStatus::Pending,
        InstallStatus::Downloading,
        InstallStatus::Uploading,
        InstallStatus::Verifying,
    ] {
        let filter = ListFilter {
            install_status: Some(status),
            ..Default::default()
        };
        let zombies = models.list(&filter).await?;
        for m in zombies {
            // Status transition. Attempt rows with status='in_progress' for
            // the same model_id are not mass-updated here — they remain
            // visible in the audit log so the admin can see what stage the
            // previous run died at. The new attempt the admin starts via
            // [Retry] will get a fresh attempt_no.
            if let Err(e) = models
                .update_install_status(
                    &m.model_id,
                    InstallStatus::Failed,
                    Some(ErrorKind::StartupRecovery),
                    Some("Process restarted while install was in progress."),
                    Some(Utc::now()),
                )
                .await
            {
                tracing::warn!(
                    model_id = %m.model_id,
                    error = %e,
                    "startup recovery: failed to mark zombie"
                );
                continue;
            }

            // Best-effort: also close the latest in-progress attempt row.
            // If the orchestrator already closed it (process died after
            // logging fail), this becomes a no-op. We don't fetch the row
            // back because the filter+UPDATE is atomic at the DB level.
            let recent_attempts = _attempts
                .list(
                    &crate::application::ports::outbound::install_attempts_log::AttemptsFilter {
                        model_id: Some(m.model_id.clone()),
                        limit: Some(5),
                        offset: None,
                    },
                )
                .await
                .unwrap_or_default();
            for a in recent_attempts.iter().filter(|a| {
                a.status == crate::domain::entities::AttemptStatus::InProgress
            }) {
                let _ = _attempts
                    .fail(
                        a.id,
                        &AttemptFailure {
                            stage: a.stage.or(Some(AttemptStage::Download)),
                            error_kind: ErrorKind::StartupRecovery,
                            error_message: "Process restarted while install was in progress."
                                .into(),
                            retryable: true,
                            bytes_downloaded: a.bytes_downloaded,
                            total_bytes: a.total_bytes,
                            duration_ms: None,
                        },
                    )
                    .await;
            }

            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

/// Remove `.tmp.*` files in `blobs/` older than [`TMP_STALE_AFTER`].
async fn clean_orphan_tmp(local_pv: &LocalPv) -> Result<u32> {
    let dir = local_pv.blobs_dir();
    if !fs::try_exists(&dir).await.unwrap_or(false) {
        return Ok(0);
    }
    let now = std::time::SystemTime::now();
    let mut count: u32 = 0;
    let mut rd = fs::read_dir(&dir).await?;
    while let Some(entry) = rd.next_entry().await? {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        if !name.starts_with(".tmp.") {
            continue;
        }
        let meta = match entry.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        let age = now.duration_since(mtime).unwrap_or(Duration::ZERO);
        if age >= TMP_STALE_AFTER {
            if fs::remove_file(entry.path()).await.is_ok() {
                count = count.saturating_add(1);
            }
        }
    }
    let _ = BLOBS_SUBDIR; // silence unused-import warning when feature off
    Ok(count)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    #[tokio::test]
    async fn clean_orphan_tmp_removes_old_tmp_files() {
        let dir = tempfile::tempdir().unwrap();
        let pv = LocalPv::new(dir.path());
        pv.ensure_root().await.unwrap();

        let blobs = pv.blobs_dir();
        let stale = blobs.join(".tmp.aaaa.bin");
        let fresh = blobs.join(".tmp.bbbb.bin");
        let canonical = blobs.join("notatmp.gguf");
        fs::write(&stale, b"x").await.unwrap();
        fs::write(&fresh, b"x").await.unwrap();
        fs::write(&canonical, b"x").await.unwrap();

        // Backdate `stale` past the threshold.
        let past = SystemTime::now() - TMP_STALE_AFTER - Duration::from_secs(60);
        let past_filetime = filetime_from(past);
        // Manually set mtime via std::fs::set_modified (Rust 1.79+).
        std::fs::OpenOptions::new()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(past)
            .unwrap();
        let _ = past_filetime;

        let removed = clean_orphan_tmp(&pv).await.unwrap();
        assert_eq!(removed, 1);
        assert!(!fs::try_exists(&stale).await.unwrap());
        assert!(fs::try_exists(&fresh).await.unwrap());
        assert!(fs::try_exists(&canonical).await.unwrap());
    }

    #[tokio::test]
    async fn clean_orphan_tmp_no_op_on_fresh_dir() {
        let dir = tempfile::tempdir().unwrap();
        let pv = LocalPv::new(dir.path());
        pv.ensure_root().await.unwrap();
        let removed = clean_orphan_tmp(&pv).await.unwrap();
        assert_eq!(removed, 0);
    }

    fn filetime_from(_t: SystemTime) -> () {
        // Helper retained for clarity; std::fs::OpenOptions::set_modified
        // is the cross-platform path.
    }
}
