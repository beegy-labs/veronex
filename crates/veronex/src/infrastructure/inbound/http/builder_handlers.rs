//! Owner-scoped app builder API. Runtime creation is explicit and disabled by default.
use axum::Json;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::FromRow;
use uuid::Uuid;

use super::error::AppError;
use super::middleware::jwt_auth::RequireBuilderManage;
use super::state::AppState;

#[derive(Serialize, FromRow)]
pub struct Repository {
    id: Uuid,
    owner_id: Uuid,
    name: String,
    remote_url: String,
    author_name: String,
    author_email: String,
    default_branch: String,
    created_at: chrono::DateTime<chrono::Utc>,
}
#[derive(Serialize, FromRow)]
pub struct Workspace {
    id: Uuid,
    owner_id: Uuid,
    repository_id: Uuid,
    name: String,
    branch: String,
    cli: String,
    status: String,
    pod_name: Option<String>,
    pod_ip: Option<String>,
    generation: i64,
    last_error: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}
#[derive(Serialize, FromRow)]
pub struct Preview {
    id: Uuid,
    workspace_id: Uuid,
    owner_id: Uuid,
    name: String,
    command: String,
    port: i32,
    status: String,
    last_error: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}
#[derive(Deserialize)]
pub struct CreateRepository {
    name: String,
    remote_url: String,
    default_branch: Option<String>,
    author_name: String,
    author_email: String,
}
#[derive(Deserialize)]
pub struct CreateWorkspace {
    repository_id: Uuid,
    name: String,
    branch: String,
    cli: Option<String>,
}
#[derive(Deserialize)]
pub struct SwitchCli {
    cli: String,
}
#[derive(Deserialize)]
pub struct CreatePreview {
    name: String,
    command: String,
    port: u16,
}
#[derive(Deserialize, Serialize)]
pub struct GitAction {
    message: Option<String>,
}

async fn enabled(state: &AppState) -> Result<(), AppError> {
    let settings = state
        .lab_settings_repo
        .get()
        .await
        .map_err(|e| AppError::Internal(e))?;
    if !settings.builder_enabled {
        return Err(AppError::NotFound("builder disabled".into()));
    }
    Ok(())
}

fn check_git_url(url: &str) -> bool {
    // Reject file://, local paths and internal SSRF targets. Deployment may add
    // a stricter host allowlist through BUILDER_GIT_HOSTS.
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" || parsed.username() != "" || parsed.password().is_some() {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    if host == "localhost" || host.ends_with(".local") || host.parse::<std::net::IpAddr>().is_ok() {
        return false;
    }
    if let Ok(allowed) = std::env::var("BUILDER_GIT_HOSTS")
        && !allowed.trim().is_empty()
    {
        return allowed
            .split(',')
            .any(|entry| entry.trim().eq_ignore_ascii_case(host));
    }
    true
}

fn check_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch.len() <= 128
        && branch
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._/".contains(&c))
        && !branch.contains("..")
        && !branch.starts_with('/')
        && !branch.ends_with('/')
}

pub async fn list_repositories(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    enabled(&state).await?;
    let rows = sqlx::query_as::<_, Repository>(
        "SELECT * FROM builder_repositories WHERE owner_id=$1 ORDER BY created_at DESC",
    )
    .bind(claims.sub)
    .fetch_all(&state.pg_pool)
    .await?;
    Ok(Json(json!({"repositories": rows})))
}

pub async fn create_repository(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Json(body): Json<CreateRepository>,
) -> Result<(StatusCode, Json<Repository>), AppError> {
    enabled(&state).await?;
    if body.name.trim().is_empty()
        || !check_git_url(&body.remote_url)
        || body.author_name.trim().is_empty()
        || !body.author_email.contains('@')
    {
        return Err(AppError::BadRequest(
            "valid name, HTTPS Git URL and author identity required".into(),
        ));
    }
    let branch = body.default_branch.unwrap_or_else(|| "main".into());
    if !check_branch(&branch) {
        return Err(AppError::BadRequest("invalid branch".into()));
    }
    let row = sqlx::query_as::<_, Repository>("INSERT INTO builder_repositories(id,owner_id,name,remote_url,default_branch,author_name,author_email) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(owner_id,remote_url) DO NOTHING RETURNING *")
        .bind(Uuid::now_v7()).bind(claims.sub).bind(body.name.trim()).bind(body.remote_url.trim()).bind(branch)
        .bind(body.author_name.trim()).bind(body.author_email.trim())
        .fetch_optional(&state.pg_pool).await?;
    match row {
        Some(row) => Ok((StatusCode::CREATED, Json(row))),
        None => {
            let existing = sqlx::query_as::<_, Repository>(
                "SELECT * FROM builder_repositories WHERE owner_id=$1 AND remote_url=$2",
            )
            .bind(claims.sub)
            .bind(body.remote_url.trim())
            .fetch_one(&state.pg_pool)
            .await?;
            Ok((StatusCode::OK, Json(existing)))
        }
    }
}

pub async fn list_workspaces(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    enabled(&state).await?;
    let rows = sqlx::query_as::<_, Workspace>(
        "SELECT * FROM builder_workspaces WHERE owner_id=$1 ORDER BY updated_at DESC",
    )
    .bind(claims.sub)
    .fetch_all(&state.pg_pool)
    .await?;
    Ok(Json(json!({"workspaces": rows})))
}

pub async fn create_workspace(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Json(body): Json<CreateWorkspace>,
) -> Result<(StatusCode, Json<Workspace>), AppError> {
    enabled(&state).await?;
    if body.name.trim().is_empty() || !check_branch(&body.branch) {
        return Err(AppError::BadRequest(
            "invalid workspace name or branch".into(),
        ));
    }
    let cli = body.cli.unwrap_or_else(|| "codex".into());
    if !matches!(cli.as_str(), "codex" | "claude" | "gemini" | "local") {
        return Err(AppError::BadRequest("unsupported CLI".into()));
    }
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM builder_repositories WHERE id=$1 AND owner_id=$2)",
    )
    .bind(body.repository_id)
    .bind(claims.sub)
    .fetch_one(&state.pg_pool)
    .await?;
    if !owned {
        return Err(AppError::NotFound("repository not found".into()));
    }
    let row = sqlx::query_as::<_, Workspace>("INSERT INTO builder_workspaces(id,owner_id,repository_id,name,branch,cli) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(repository_id,branch) DO NOTHING RETURNING *")
        .bind(Uuid::now_v7()).bind(claims.sub).bind(body.repository_id).bind(body.name.trim()).bind(&body.branch).bind(cli)
        .fetch_optional(&state.pg_pool).await?;
    match row {
        Some(row) => Ok((StatusCode::CREATED, Json(row))),
        None => {
            let existing = sqlx::query_as::<_, Workspace>(
                "SELECT * FROM builder_workspaces WHERE repository_id=$1 AND branch=$2",
            )
            .bind(body.repository_id)
            .bind(&body.branch)
            .fetch_one(&state.pg_pool)
            .await?;
            Ok((StatusCode::OK, Json(existing)))
        }
    }
}

async fn owned_workspace(state: &AppState, owner: Uuid, id: Uuid) -> Result<Workspace, AppError> {
    sqlx::query_as::<_, Workspace>("SELECT * FROM builder_workspaces WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(owner)
        .fetch_optional(&state.pg_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))
}

pub async fn get_workspace(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Workspace>, AppError> {
    enabled(&state).await?;
    let mut workspace = owned_workspace(&state, claims.sub, id).await?;
    if workspace.pod_name.is_some() {
        match k8s_get_pod(&state, &workspace).await {
            Ok(pod) => {
                let ip = pod
                    .pointer("/status/podIP")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let phase = pod
                    .pointer("/status/phase")
                    .and_then(Value::as_str)
                    .unwrap_or("Pending");
                let failed = pod
                    .pointer("/status/initContainerStatuses/0/state/terminated/exitCode")
                    .and_then(Value::as_i64)
                    .is_some_and(|code| code != 0);
                let ready = pod
                    .pointer("/status/containerStatuses")
                    .and_then(Value::as_array)
                    .is_some_and(|containers| {
                        ["cli", "preview"].iter().all(|name| {
                            containers.iter().any(|container| {
                                container.get("name").and_then(Value::as_str) == Some(name)
                                    && container.get("ready").and_then(Value::as_bool) == Some(true)
                            })
                        })
                    });
                let status = if workspace.status == "stopping" {
                    "stopping"
                } else if failed || phase == "Failed" {
                    "failed"
                } else if phase == "Running" && ip.is_some() && ready {
                    "running"
                } else {
                    "starting"
                };
                if workspace.pod_ip != ip || workspace.status != status {
                    workspace = sqlx::query_as::<_, Workspace>("UPDATE builder_workspaces SET pod_ip=$2,status=$3,last_error=$4,updated_at=now() WHERE id=$1 RETURNING *")
                    .bind(id).bind(ip).bind(status).bind(if status == "failed" { Some("workspace Pod failed to start") } else { None })
                    .fetch_one(&state.pg_pool).await?;
                }
                if status == "running" {
                    reconcile_previews(&state, &workspace).await?;
                }
            }
            Err(AppError::NotFound(_)) => {
                workspace = sqlx::query_as::<_, Workspace>("UPDATE builder_workspaces SET pod_name=NULL,pod_ip=NULL,status='suspended',generation=generation+1,updated_at=now() WHERE id=$1 RETURNING *")
                .bind(id).fetch_one(&state.pg_pool).await?;
                sqlx::query("UPDATE builder_previews SET status='suspended' WHERE workspace_id=$1 AND status IN ('running','starting','pending')")
                .bind(id).execute(&state.pg_pool).await?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(Json(workspace))
}

fn k8s_config() -> Result<(String, String, String), AppError> {
    let host = std::env::var("KUBERNETES_SERVICE_HOST")
        .map_err(|_| AppError::ServiceUnavailable("Kubernetes unavailable".into()))?;
    let port = std::env::var("KUBERNETES_SERVICE_PORT").unwrap_or_else(|_| "443".into());
    let namespace = std::env::var("BUILDER_NAMESPACE")
        .map_err(|_| AppError::ServiceUnavailable("builder namespace unset".into()))?;
    let token = std::fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/token")
        .map_err(|_| AppError::ServiceUnavailable("service account token unavailable".into()))?;
    Ok((format!("https://{host}:{port}"), namespace, token))
}

fn k8s_client() -> Result<reqwest::Client, AppError> {
    let ca = std::fs::read("/var/run/secrets/kubernetes.io/serviceaccount/ca.crt")
        .map_err(|_| AppError::ServiceUnavailable("Kubernetes CA unavailable".into()))?;
    let cert = reqwest::Certificate::from_pem(&ca)
        .map_err(|_| AppError::ServiceUnavailable("invalid Kubernetes CA".into()))?;
    reqwest::Client::builder()
        .add_root_certificate(cert)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Internal(e.into()))
}

async fn k8s_get_pod(_state: &AppState, workspace: &Workspace) -> Result<Value, AppError> {
    let (base, namespace, token) = k8s_config()?;
    let name = workspace
        .pod_name
        .as_deref()
        .ok_or_else(|| AppError::NotFound("Pod missing".into()))?;
    let response = k8s_client()?
        .get(format!("{base}/api/v1/namespaces/{namespace}/pods/{name}"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(AppError::NotFound("Pod not found".into()));
    }
    if !response.status().is_success() {
        return Err(AppError::BadGateway(format!(
            "Kubernetes returned {}",
            response.status()
        )));
    }
    response
        .json()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))
}

pub async fn start_workspace(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Workspace>, AppError> {
    enabled(&state).await?;
    let workspace = owned_workspace(&state, claims.sub, id).await?;
    if workspace.pod_name.is_some() {
        return Ok(Json(workspace));
    }
    let (base, namespace, token) = k8s_config()?;
    let image = std::env::var("BUILDER_RUNNER_IMAGE")
        .map_err(|_| AppError::ServiceUnavailable("builder image unset".into()))?;
    let pvc = std::env::var("BUILDER_PVC")
        .map_err(|_| AppError::ServiceUnavailable("builder PVC unset".into()))?;
    let secret = std::env::var("BUILDER_RUNNER_SECRET")
        .map_err(|_| AppError::ServiceUnavailable("builder secret unset".into()))?;
    let repo: Repository =
        sqlx::query_as("SELECT * FROM builder_repositories WHERE id=$1 AND owner_id=$2")
            .bind(workspace.repository_id)
            .bind(claims.sub)
            .fetch_one(&state.pg_pool)
            .await?;
    let pod_name = format!("builder-{}", id.simple());
    let subpath = format!("workspaces/{id}");
    let cli_memory = std::env::var("BUILDER_CLI_MEMORY_LIMIT").unwrap_or_else(|_| "2Gi".into());
    let preview_memory =
        std::env::var("BUILDER_PREVIEW_MEMORY_LIMIT").unwrap_or_else(|_| "2Gi".into());
    let manifest = json!({
        "apiVersion":"v1", "kind":"Pod",
        "metadata":{"name":pod_name,"namespace":namespace,"labels":{"app":"veronex-builder","builder.workspace-id":id.to_string()}},
        "spec":{
            "restartPolicy":"Always", "automountServiceAccountToken":false,
            "serviceAccountName": std::env::var("BUILDER_POD_SERVICE_ACCOUNT").unwrap_or_else(|_| "default".into()),
            "securityContext":{"runAsUser":1000,"runAsGroup":1000,"fsGroup":1000},
            "initContainers":[{"name":"init","image":image,"command":["veronex-builder-runner"],
                "env":[{"name":"BUILDER_RUNNER_MODE","value":"init"},{"name":"BUILDER_WORKSPACE_ROOT","value":format!("/volume/{subpath}")},
                    {"name":"BUILDER_REMOTE_URL","value":repo.remote_url},{"name":"BUILDER_BRANCH","value":workspace.branch},
                    {"name":"BUILDER_GIT_TOKEN","valueFrom":{"secretKeyRef":{"name":secret,"key":"GIT_TOKEN"}}},
                    {"name":"BUILDER_AUTHOR_NAME","value":repo.author_name},{"name":"BUILDER_AUTHOR_EMAIL","value":repo.author_email}],
                "volumeMounts":[{"name":"shared","mountPath":"/volume"},{"name":"runtime-sqlite","mountPath":"/workspace/sqlite"}]}],
            "containers":[
                {"name":"cli","image":image,"ports":[{"containerPort":7777}],
                    "env":[{"name":"BUILDER_RUNNER_MODE","value":"cli"},{"name":"PORT","value":"7777"},
                        {"name":"BUILDER_INITIAL_CLI","value":workspace.cli},
                        {"name":"BUILDER_RUNNER_TOKEN","valueFrom":{"secretKeyRef":{"name":secret,"key":"RUNNER_TOKEN"}}},
                        {"name":"BUILDER_GIT_TOKEN","valueFrom":{"secretKeyRef":{"name":secret,"key":"GIT_TOKEN"}}}],
                    "volumeMounts":[{"name":"shared","mountPath":"/workspace","subPath":subpath},
                        {"name":"runtime-sqlite","mountPath":"/workspace/sqlite"}],
                    "resources":{"requests":{"cpu":"100m","memory":"256Mi"},"limits":{"cpu":"2","memory":cli_memory}}},
                {"name":"preview","image":image,"ports":[{"containerPort":7778}],
                    "env":[{"name":"BUILDER_RUNNER_MODE","value":"preview"},{"name":"PORT","value":"7778"},
                        {"name":"BUILDER_RUNNER_TOKEN","valueFrom":{"secretKeyRef":{"name":secret,"key":"PREVIEW_TOKEN"}}}],
                    "volumeMounts":[{"name":"shared","mountPath":"/workspace/repo","subPath":format!("{subpath}/repo")},
                        {"name":"shared","mountPath":"/workspace/previews","subPath":format!("{subpath}/previews")},
                        {"name":"preview-home","mountPath":"/workspace/home"}],
                    "resources":{"requests":{"cpu":"100m","memory":"128Mi"},"limits":{"cpu":"2","memory":preview_memory}}}],
            "volumes":[{"name":"shared","persistentVolumeClaim":{"claimName":pvc}},
                {"name":"runtime-sqlite","emptyDir":{}},
                {"name":"preview-home","emptyDir":{}}]
        }
    });
    let max_active = std::env::var("BUILDER_MAX_ACTIVE_WORKSPACES")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(2)
        .max(1);
    let mut tx = state.pg_pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(7132026)")
        .execute(&mut *tx)
        .await?;
    let active: i64 = sqlx::query_scalar("SELECT count(*) FROM builder_workspaces WHERE status IN ('starting','running') AND id <> $1")
        .bind(id).fetch_one(&mut *tx).await?;
    if active >= max_active {
        return Err(AppError::TooManyRequests { retry_after: 30 });
    }
    let response = k8s_client()?
        .post(format!("{base}/api/v1/namespaces/{namespace}/pods"))
        .bearer_auth(token)
        .json(&manifest)
        .send()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    if response.status() == StatusCode::CONFLICT {
        let existing = k8s_get_pod(
            &state,
            &Workspace {
                pod_name: Some(pod_name.clone()),
                ..workspace
            },
        )
        .await?;
        if existing
            .pointer("/metadata/labels/builder.workspace-id")
            .and_then(Value::as_str)
            != Some(&id.to_string())
        {
            return Err(AppError::Conflict("Pod name collision".into()));
        }
    } else if !response.status().is_success() {
        return Err(AppError::BadGateway(format!(
            "Pod creation failed: {}",
            response.status()
        )));
    }
    let row = sqlx::query_as::<_, Workspace>("UPDATE builder_workspaces SET pod_name=$2,status='starting',updated_at=now() WHERE id=$1 RETURNING *")
        .bind(id).bind(pod_name).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(row))
}

pub async fn stop_workspace(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Workspace>, AppError> {
    enabled(&state).await?;
    let workspace = owned_workspace(&state, claims.sub, id).await?;
    let Some(name) = workspace.pod_name else {
        return Ok(Json(workspace));
    };
    if workspace.status == "running" {
        let addr = runner_address(&state, claims.sub, id, 7777).await?;
        runner_post(&state, format!("{addr}/checkpoint"), json!({})).await?;
        sync_native_history(&state, claims.sub, id).await?;
    }
    let (base, namespace, token) = k8s_config()?;
    let response = k8s_client()?
        .delete(format!("{base}/api/v1/namespaces/{namespace}/pods/{name}"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    if !response.status().is_success() && response.status() != StatusCode::NOT_FOUND {
        return Err(AppError::BadGateway(format!(
            "Pod deletion failed: {}",
            response.status()
        )));
    }
    let row = sqlx::query_as::<_, Workspace>(
        "UPDATE builder_workspaces SET status='stopping',updated_at=now() WHERE id=$1 RETURNING *",
    )
    .bind(id)
    .fetch_one(&state.pg_pool)
    .await?;
    Ok(Json(row))
}

fn runner_token() -> Result<String, AppError> {
    std::env::var("BUILDER_RUNNER_TOKEN")
        .map_err(|_| AppError::ServiceUnavailable("runner token unset".into()))
}

fn preview_token() -> Result<String, AppError> {
    std::env::var("BUILDER_PREVIEW_TOKEN")
        .map_err(|_| AppError::ServiceUnavailable("preview token unset".into()))
}
async fn runner_address(
    state: &AppState,
    owner: Uuid,
    id: Uuid,
    port: u16,
) -> Result<String, AppError> {
    let workspace = owned_workspace(state, owner, id).await?;
    let ip = workspace
        .pod_ip
        .ok_or_else(|| AppError::Conflict("workspace is not ready".into()))?;
    Ok(format!("http://{ip}:{port}"))
}
async fn runner_post(state: &AppState, url: String, payload: Value) -> Result<Value, AppError> {
    let token = if url.contains(":7778/") {
        preview_token()?
    } else {
        runner_token()?
    };
    let response = state
        .http_client
        .post(url)
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::BadGateway(format!(
            "runner returned {}",
            response.status()
        )));
    }
    if response.status() == StatusCode::NO_CONTENT {
        return Ok(json!({}));
    }
    response
        .json()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))
}

async fn reconcile_previews(state: &AppState, workspace: &Workspace) -> Result<(), AppError> {
    let Some(ip) = workspace.pod_ip.as_deref() else {
        return Ok(());
    };
    let previews = sqlx::query_as::<_, Preview>(
        "SELECT * FROM builder_previews WHERE workspace_id=$1 AND status='suspended'",
    )
    .bind(workspace.id)
    .fetch_all(&state.pg_pool)
    .await?;
    for preview in previews {
        let result = runner_post(state, format!("http://{ip}:7778/previews"),
            json!({"id":preview.id,"name":preview.name,"command":preview.command,"port":preview.port})).await;
        if result.is_ok() {
            sqlx::query("UPDATE builder_previews SET status='running' WHERE id=$1")
                .bind(preview.id)
                .execute(&state.pg_pool)
                .await?;
        } else {
            tracing::warn!(preview_id=%preview.id, "preview restart is pending");
        }
    }
    Ok(())
}

async fn sync_native_history(state: &AppState, owner: Uuid, id: Uuid) -> Result<(), AppError> {
    let address = runner_address(state, owner, id, 7777).await?;
    let response = state
        .http_client
        .get(format!("{address}/history"))
        .bearer_auth(runner_token()?)
        .send()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::BadGateway(format!(
            "history capture returned {}",
            response.status()
        )));
    }
    let data: Value = response
        .json()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    for event in data
        .get("events")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(cli) = event.get("cli").and_then(Value::as_str) else {
            continue;
        };
        let Some(path) = event.get("source_path").and_then(Value::as_str) else {
            continue;
        };
        let Some(hash) = event.get("source_hash").and_then(Value::as_str) else {
            continue;
        };
        let Some(payload) = event.get("payload") else {
            continue;
        };
        let format = event
            .get("source_format")
            .and_then(Value::as_str)
            .unwrap_or("native");
        let kind = if payload.get("capture_gap").is_some() {
            "capture_gap"
        } else {
            "native"
        };
        sqlx::query("INSERT INTO builder_events(workspace_id,cli,kind,payload,source_format,source_path,source_hash) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING")
            .bind(id).bind(cli).bind(kind).bind(payload).bind(format).bind(path).bind(hash)
            .execute(&state.pg_pool).await?;
    }
    Ok(())
}

pub async fn switch_cli(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SwitchCli>,
) -> Result<Json<Value>, AppError> {
    enabled(&state).await?;
    if !matches!(body.cli.as_str(), "codex" | "claude" | "gemini" | "local") {
        return Err(AppError::BadRequest("unsupported CLI".into()));
    }
    sync_native_history(&state, claims.sub, id).await?;
    let addr = runner_address(&state, claims.sub, id, 7777).await?;
    let result = runner_post(
        &state,
        format!("{addr}/cli/switch"),
        json!({"cli":body.cli}),
    )
    .await?;
    sqlx::query("UPDATE builder_workspaces SET cli=$2,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(&body.cli)
        .execute(&state.pg_pool)
        .await?;
    sqlx::query(
        "INSERT INTO builder_events(workspace_id,cli,kind,payload) VALUES($1,$2,'switch',$3)",
    )
    .bind(id)
    .bind(&body.cli)
    .bind(&result)
    .execute(&state.pg_pool)
    .await?;
    Ok(Json(result))
}

pub async fn git_action(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path((id, action)): Path<(Uuid, String)>,
    Json(body): Json<GitAction>,
) -> Result<Json<Value>, AppError> {
    enabled(&state).await?;
    if !matches!(action.as_str(), "status" | "fetch" | "commit" | "push") {
        return Err(AppError::BadRequest("unsupported Git action".into()));
    }
    let addr = runner_address(&state, claims.sub, id, 7777).await?;
    let result = runner_post(
        &state,
        format!("{addr}/git/{action}"),
        serde_json::to_value(body).map_err(|e| AppError::Internal(e.into()))?,
    )
    .await?;
    sqlx::query("INSERT INTO builder_events(workspace_id,cli,kind,payload) SELECT id,cli,$2,$3 FROM builder_workspaces WHERE id=$1")
        .bind(id).bind(format!("git.{action}")).bind(&result).execute(&state.pg_pool).await?;
    Ok(Json(result))
}

pub async fn list_previews(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    enabled(&state).await?;
    let workspace = owned_workspace(&state, claims.sub, id).await?;
    let mut rows = sqlx::query_as::<_, Preview>(
        "SELECT * FROM builder_previews WHERE workspace_id=$1 AND owner_id=$2 ORDER BY created_at",
    )
    .bind(id)
    .bind(claims.sub)
    .fetch_all(&state.pg_pool)
    .await?;
    if workspace.status == "running"
        && let Some(ip) = workspace.pod_ip
    {
        let response = state
            .http_client
            .get(format!("http://{ip}:7778/previews"))
            .bearer_auth(preview_token()?)
            .send()
            .await
            .map_err(|e| AppError::BadGateway(e.to_string()))?;
        if response.status().is_success() {
            let live: Value = response
                .json()
                .await
                .map_err(|e| AppError::BadGateway(e.to_string()))?;
            let live = live.get("previews").and_then(Value::as_array);
            for preview in &mut rows {
                if preview.status == "stopped" {
                    continue;
                }
                let current = live.and_then(|items| {
                    items.iter().find(|item| {
                        item.get("id").and_then(Value::as_str)
                            == Some(preview.id.to_string().as_str())
                    })
                });
                let new_status = match current {
                    Some(item) if item.get("running").and_then(Value::as_bool) == Some(false) => {
                        "failed"
                    }
                    Some(item) if item.get("ready").and_then(Value::as_bool) == Some(true) => {
                        "running"
                    }
                    Some(_) => "starting",
                    None => "failed",
                };
                if preview.status != new_status {
                    sqlx::query("UPDATE builder_previews SET status=$2 WHERE id=$1")
                        .bind(preview.id)
                        .bind(new_status)
                        .execute(&state.pg_pool)
                        .await?;
                    preview.status = new_status.into();
                }
            }
        }
    }
    Ok(Json(json!({"previews":rows})))
}

pub async fn start_preview(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreatePreview>,
) -> Result<(StatusCode, Json<Preview>), AppError> {
    enabled(&state).await?;
    if body.name.trim().is_empty() || body.command.trim().is_empty() || body.port < 1024 {
        return Err(AppError::BadRequest("invalid preview".into()));
    }
    let addr = runner_address(&state, claims.sub, id, 7778).await?;
    let existing: Option<Preview> = sqlx::query_as(
        "SELECT * FROM builder_previews WHERE workspace_id=$1 AND port=$2 AND owner_id=$3",
    )
    .bind(id)
    .bind(i32::from(body.port))
    .bind(claims.sub)
    .fetch_optional(&state.pg_pool)
    .await?;
    if existing.is_none() {
        let max_previews = std::env::var("BUILDER_MAX_PREVIEWS")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(4)
            .max(1);
        let active: i64 = sqlx::query_scalar("SELECT count(*) FROM builder_previews WHERE workspace_id=$1 AND status IN ('pending','running','suspended')")
            .bind(id).fetch_one(&state.pg_pool).await?;
        if active >= max_previews {
            return Err(AppError::TooManyRequests { retry_after: 30 });
        }
    }
    let inserted = sqlx::query_as::<_, Preview>("INSERT INTO builder_previews(id,workspace_id,owner_id,name,command,port,status) VALUES($1,$2,$3,$4,$5,$6,'pending') ON CONFLICT(workspace_id,port) DO NOTHING RETURNING *")
        .bind(Uuid::now_v7()).bind(id).bind(claims.sub).bind(&body.name).bind(&body.command).bind(i32::from(body.port))
        .fetch_optional(&state.pg_pool).await?;
    let (preview, status) = if let Some(preview) = inserted {
        (preview, StatusCode::CREATED)
    } else {
        let existing = sqlx::query_as::<_, Preview>(
            "SELECT * FROM builder_previews WHERE workspace_id=$1 AND port=$2 AND owner_id=$3",
        )
        .bind(id)
        .bind(i32::from(body.port))
        .bind(claims.sub)
        .fetch_one(&state.pg_pool)
        .await?;
        if existing.name != body.name || existing.command != body.command {
            return Err(AppError::Conflict(
                "preview port belongs to a different profile".into(),
            ));
        }
        (existing, StatusCode::OK)
    };
    if preview.status != "running" {
        let result = runner_post(&state, format!("{addr}/previews"),
            json!({"id":preview.id,"name":preview.name,"command":preview.command,"port":preview.port})).await;
        if let Err(error) = result {
            sqlx::query("UPDATE builder_previews SET status='failed',last_error=$2 WHERE id=$1")
                .bind(preview.id)
                .bind(error.to_string())
                .execute(&state.pg_pool)
                .await?;
            return Err(error);
        }
    }
    let row = sqlx::query_as::<_, Preview>(
        "UPDATE builder_previews SET status='running',last_error=NULL WHERE id=$1 RETURNING *",
    )
    .bind(preview.id)
    .fetch_one(&state.pg_pool)
    .await?;
    Ok((status, Json(row)))
}

pub async fn stop_preview(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path((id, preview_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    enabled(&state).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM builder_previews WHERE id=$1 AND workspace_id=$2 AND owner_id=$3)")
        .bind(preview_id).bind(id).bind(claims.sub).fetch_one(&state.pg_pool).await?;
    if !exists {
        return Err(AppError::NotFound("preview not found".into()));
    }
    let addr = runner_address(&state, claims.sub, id, 7778).await?;
    runner_post(
        &state,
        format!("{addr}/previews/{preview_id}/stop"),
        json!({}),
    )
    .await?;
    sqlx::query("UPDATE builder_previews SET status='stopped' WHERE id=$1")
        .bind(preview_id)
        .execute(&state.pg_pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn preview_logs(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path((id, preview_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, AppError> {
    enabled(&state).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM builder_previews WHERE id=$1 AND workspace_id=$2 AND owner_id=$3)")
        .bind(preview_id).bind(id).bind(claims.sub).fetch_one(&state.pg_pool).await?;
    if !exists {
        return Err(AppError::NotFound("preview not found".into()));
    }
    let addr = runner_address(&state, claims.sub, id, 7778).await?;
    let response = state
        .http_client
        .get(format!("{addr}/previews/{preview_id}/logs"))
        .bearer_auth(preview_token()?)
        .send()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::BadGateway(format!(
            "runner returned {}",
            response.status()
        )));
    }
    Ok(Json(
        response
            .json()
            .await
            .map_err(|e| AppError::BadGateway(e.to_string()))?,
    ))
}

pub async fn list_events(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    enabled(&state).await?;
    let workspace = owned_workspace(&state, claims.sub, id).await?;
    if workspace.status == "running" {
        sync_native_history(&state, claims.sub, id).await?;
    }
    let rows: Vec<(i64, String, String, Value, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id,cli,kind,payload,created_at FROM builder_events WHERE workspace_id=$1 ORDER BY id DESC LIMIT 200")
        .bind(id).fetch_all(&state.pg_pool).await?;
    Ok(Json(
        json!({"events":rows.iter().map(|(id,cli,kind,payload,created_at)| json!({"id":id,"cli":cli,"kind":kind,"payload":payload,"created_at":created_at})).collect::<Vec<_>>() }),
    ))
}

async fn preview_response(
    state: &AppState,
    owner: Uuid,
    id: Uuid,
    preview_id: Uuid,
    path: &str,
) -> Result<Response, AppError> {
    let address = runner_address(state, owner, id, 7778).await?;
    let preview: Preview = sqlx::query_as(
        "SELECT * FROM builder_previews WHERE id=$1 AND workspace_id=$2 AND owner_id=$3",
    )
    .bind(preview_id)
    .bind(id)
    .bind(owner)
    .fetch_optional(&state.pg_pool)
    .await?
    .ok_or_else(|| AppError::NotFound("preview not found".into()))?;
    if preview.status != "running" {
        return Err(AppError::Conflict("preview stopped".into()));
    }
    let ip = address
        .trim_start_matches("http://")
        .split(':')
        .next()
        .unwrap_or_default();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Internal(e.into()))?;
    let response = client
        .get(format!("http://{ip}:{}{path}", preview.port))
        .send()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    let status = response.status();
    let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(AppError::BadGateway("preview response too large".into()));
    }
    let mut result = Response::builder()
        .status(status)
        .body(axum::body::Body::from(bytes))
        .map_err(|e| AppError::Internal(e.into()))?;
    if let Some(content_type) = content_type {
        result
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
    }
    result
        .headers_mut()
        .insert("x-veronex-preview", HeaderValue::from_static("1"));
    result.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "sandbox allow-scripts allow-forms allow-popups; frame-ancestors *",
        ),
    );
    Ok(result)
}

pub async fn view_preview_root(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path((id, preview_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, AppError> {
    enabled(&state).await?;
    preview_response(&state, claims.sub, id, preview_id, "/").await
}

pub async fn view_preview_path(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path((id, preview_id, path)): Path<(Uuid, Uuid, String)>,
) -> Result<Response, AppError> {
    enabled(&state).await?;
    if path.contains("..") {
        return Err(AppError::BadRequest("invalid path".into()));
    }
    preview_response(&state, claims.sub, id, preview_id, &format!("/{path}")).await
}

#[derive(Deserialize)]
pub struct TerminalSize {
    rows: u16,
    cols: u16,
}

pub async fn resize_terminal(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(size): Json<TerminalSize>,
) -> Result<StatusCode, AppError> {
    enabled(&state).await?;
    let address = runner_address(&state, claims.sub, id, 7777).await?;
    let response = state
        .http_client
        .post(format!("{address}/terminal/resize"))
        .bearer_auth(runner_token()?)
        .json(&json!({"rows":size.rows,"cols":size.cols}))
        .send()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    if response.status().is_success() {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::BadGateway("resize failed".into()))
    }
}

pub async fn terminal(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    enabled(&state).await?;
    let allowed_origin = std::env::var("BUILDER_BROWSER_ORIGIN")
        .map_err(|_| AppError::ServiceUnavailable("builder origin not configured".into()))?;
    if headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        != Some(allowed_origin.as_str())
    {
        return Err(AppError::Forbidden("untrusted terminal origin".into()));
    }
    let address = runner_address(&state, claims.sub, id, 7777).await?;
    let token = runner_token()?;
    Ok(ws
        .on_upgrade(move |socket| proxy_terminal(socket, address, token))
        .into_response())
}

async fn proxy_terminal(browser: WebSocket, address: String, token: String) {
    let uri = format!("{}/terminal", address.replace("http://", "ws://"));
    let Ok(mut request) = uri.into_client_request() else {
        return;
    };
    if let Ok(value) = format!("Bearer {token}").parse() {
        request.headers_mut().insert("Authorization", value);
    }
    let Ok((runner, _)) = tokio_tungstenite::connect_async(request).await else {
        return;
    };
    let (mut browser_tx, mut browser_rx) = browser.split();
    let (mut runner_tx, mut runner_rx) = runner.split();
    loop {
        tokio::select! {
            from_browser = browser_rx.next() => match from_browser {
                Some(Ok(Message::Text(value))) => { if runner_tx.send(tokio_tungstenite::tungstenite::Message::Text(value.to_string().into())).await.is_err() { break; } },
                Some(Ok(Message::Binary(value))) => { if runner_tx.send(tokio_tungstenite::tungstenite::Message::Binary(value)).await.is_err() { break; } },
                _ => break,
            },
            from_runner = runner_rx.next() => match from_runner {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(value))) => { if browser_tx.send(Message::Text(value.to_string().into())).await.is_err() { break; } },
                Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(value))) => { if browser_tx.send(Message::Binary(value)).await.is_err() { break; } },
                _ => break,
            }
        }
    }
}

use tokio_tungstenite::tungstenite::client::IntoClientRequest;

#[derive(Deserialize)]
pub struct CreatePull {
    title: String,
    body: String,
}

fn contains_forbidden_attribution(text: &str) -> bool {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| matches!(part, "ai" | "llm" | "codex" | "claude" | "gpt" | "copilot"))
}

fn gitea_endpoint(repo: &Repository) -> Result<String, AppError> {
    let url = reqwest::Url::parse(&repo.remote_url)
        .map_err(|_| AppError::BadRequest("invalid Git URL".into()))?;
    let parts: Vec<_> = url
        .path()
        .trim_matches('/')
        .trim_end_matches(".git")
        .split('/')
        .collect();
    if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
        return Err(AppError::BadRequest(
            "Gitea URL must have owner/repo path".into(),
        ));
    }
    Ok(format!(
        "{}/api/v1/repos/{}/{}/pulls",
        url.origin().ascii_serialization(),
        parts[0],
        parts[1]
    ))
}

fn git_token() -> Result<String, AppError> {
    std::env::var("BUILDER_GIT_TOKEN")
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::ServiceUnavailable("Git token not configured".into()))
}

fn git_api_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Internal(e.into()))
}

async fn workspace_repo(
    state: &AppState,
    owner: Uuid,
    id: Uuid,
) -> Result<(Workspace, Repository), AppError> {
    let workspace = owned_workspace(state, owner, id).await?;
    let repo = sqlx::query_as::<_, Repository>(
        "SELECT * FROM builder_repositories WHERE id=$1 AND owner_id=$2",
    )
    .bind(workspace.repository_id)
    .bind(owner)
    .fetch_one(&state.pg_pool)
    .await?;
    Ok((workspace, repo))
}

async fn stored_pull(
    state: &AppState,
    id: Uuid,
) -> Result<Option<(i64, String, String, String)>, AppError> {
    Ok(sqlx::query_as(
        "SELECT remote_number,url,head_sha,status FROM builder_pull_requests WHERE workspace_id=$1",
    )
    .bind(id)
    .fetch_optional(&state.pg_pool)
    .await?)
}

pub async fn get_pull(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    enabled(&state).await?;
    let (_, repo) = workspace_repo(&state, claims.sub, id).await?;
    let Some((number, url, _, status)) = stored_pull(&state, id).await? else {
        return Ok(Json(json!({"pull":null})));
    };
    let response = git_api_client()?
        .get(format!("{}/{number}", gitea_endpoint(&repo)?))
        .header("Authorization", format!("token {}", git_token()?))
        .send()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::BadGateway(format!(
            "Gitea returned {}",
            response.status()
        )));
    }
    let remote: Value = response
        .json()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    Ok(Json(
        json!({"pull":{"number":number,"url":url,"status":status,"remote":remote}}),
    ))
}

pub async fn create_pull(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreatePull>,
) -> Result<Json<Value>, AppError> {
    enabled(&state).await?;
    if body.title.trim().is_empty()
        || body.title.len() > 200
        || body.body.len() > 16_000
        || contains_forbidden_attribution(&body.title)
        || contains_forbidden_attribution(&body.body)
    {
        return Err(AppError::BadRequest("invalid pull request text".into()));
    }
    let (workspace, repo) = workspace_repo(&state, claims.sub, id).await?;
    if stored_pull(&state, id).await?.is_some() {
        return get_pull(RequireBuilderManage(claims), State(state), Path(id)).await;
    }
    let endpoint = gitea_endpoint(&repo)?;
    let client = git_api_client()?;
    let authorization = format!("token {}", git_token()?);
    // Reconcile an earlier request whose remote result arrived after a timeout.
    let existing = client
        .get(&endpoint)
        .query(&[("state", "open"), ("head", workspace.branch.as_str())])
        .header("Authorization", &authorization)
        .send()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    if !existing.status().is_success() {
        return Err(AppError::BadGateway(format!(
            "Gitea returned {}",
            existing.status()
        )));
    }
    let found: Vec<Value> = existing
        .json()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    let remote = if let Some(found) = found.into_iter().find(|pull| {
        pull.pointer("/head/ref").and_then(Value::as_str) == Some(workspace.branch.as_str())
    }) {
        found
    } else {
        let response = client.post(&endpoint).header("Authorization", &authorization)
            .json(&json!({"title":body.title,"body":body.body,"head":workspace.branch,"base":repo.default_branch}))
            .send().await.map_err(|e| AppError::BadGateway(e.to_string()))?;
        if !response.status().is_success() {
            return Err(AppError::BadGateway(format!(
                "Gitea returned {}",
                response.status()
            )));
        }
        response
            .json()
            .await
            .map_err(|e| AppError::BadGateway(e.to_string()))?
    };
    let number = remote
        .get("number")
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::BadGateway("Gitea returned no PR number".into()))?;
    let url = remote
        .get("html_url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let sha = remote
        .pointer("/head/sha")
        .and_then(Value::as_str)
        .unwrap_or_default();
    sqlx::query("INSERT INTO builder_pull_requests(workspace_id,remote_number,url,head_sha) VALUES($1,$2,$3,$4) ON CONFLICT(workspace_id) DO NOTHING")
        .bind(id).bind(number).bind(url).bind(sha).execute(&state.pg_pool).await?;
    Ok(Json(json!({"pull":remote})))
}

pub async fn merge_pull(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    enabled(&state).await?;
    let (_, repo) = workspace_repo(&state, claims.sub, id).await?;
    let Some((number, url, expected_sha, status)) = stored_pull(&state, id).await? else {
        return Err(AppError::NotFound("pull request not found".into()));
    };
    if status == "merged" {
        return Ok(Json(json!({"status":"merged","url":url})));
    }
    let endpoint = format!("{}/{number}", gitea_endpoint(&repo)?);
    let client = git_api_client()?;
    let authorization = format!("token {}", git_token()?);
    let current = client
        .get(&endpoint)
        .header("Authorization", &authorization)
        .send()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    if !current.status().is_success() {
        return Err(AppError::BadGateway(format!(
            "Gitea returned {}",
            current.status()
        )));
    }
    let pull: Value = current
        .json()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    let head_sha = pull
        .pointer("/head/sha")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if head_sha.is_empty() || head_sha != expected_sha {
        return Err(AppError::Conflict(
            "PR head changed; review the new revision before merging".into(),
        ));
    }
    if pull.get("merged").and_then(Value::as_bool) == Some(true) {
        sqlx::query("UPDATE builder_pull_requests SET status='merged' WHERE workspace_id=$1")
            .bind(id)
            .execute(&state.pg_pool)
            .await?;
        return Ok(Json(json!({"status":"merged","url":url})));
    }
    if pull.get("mergeable").and_then(Value::as_bool) != Some(true) {
        return Err(AppError::Conflict("pull request is not mergeable".into()));
    }
    let response = client.post(format!("{endpoint}/merge")).header("Authorization", &authorization)
        .json(&json!({"do":"merge","head_commit_id":head_sha,"force_merge":false,"merge_when_checks_succeed":false}))
        .send().await.map_err(|e| AppError::BadGateway(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::Conflict(format!(
            "Gitea merge returned {}",
            response.status()
        )));
    }
    sqlx::query("UPDATE builder_pull_requests SET status='merged' WHERE workspace_id=$1")
        .bind(id)
        .execute(&state.pg_pool)
        .await?;
    Ok(Json(json!({"status":"merged","url":url})))
}

#[derive(Serialize, Deserialize)]
struct PreviewTicket {
    sub: Uuid,
    preview_id: Uuid,
    aud: String,
    exp: usize,
}

fn preview_domain() -> Result<String, AppError> {
    std::env::var("BUILDER_PREVIEW_DOMAIN")
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::ServiceUnavailable("preview domain not configured".into()))
}

fn preview_id_from_host(host: &str, domain: &str) -> Option<Uuid> {
    let host = host.split(':').next()?;
    let label = host.strip_suffix(&format!(".{domain}"))?;
    Uuid::parse_str(label.strip_prefix("p-")?).ok()
}

async fn preview_upstream(
    state: &AppState,
    owner: Uuid,
    preview_id: Uuid,
) -> Result<String, AppError> {
    let row: Option<(Option<String>, i32)> = sqlx::query_as(
        "SELECT w.pod_ip,p.port FROM builder_previews p JOIN builder_workspaces w ON w.id=p.workspace_id WHERE p.id=$1 AND p.owner_id=$2 AND p.status='running' AND w.status='running'")
        .bind(preview_id).bind(owner).fetch_optional(&state.pg_pool).await?;
    let Some((Some(ip), port)) = row else {
        return Err(AppError::NotFound("preview unavailable".into()));
    };
    Ok(format!("{ip}:{port}"))
}

pub async fn preview_ticket(
    RequireBuilderManage(claims): RequireBuilderManage,
    State(state): State<AppState>,
    Path((id, preview_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, AppError> {
    enabled(&state).await?;
    owned_workspace(&state, claims.sub, id).await?;
    preview_upstream(&state, claims.sub, preview_id).await?;
    let ticket = PreviewTicket {
        sub: claims.sub,
        preview_id,
        aud: "builder-preview".into(),
        exp: (chrono::Utc::now().timestamp() + 6 * 3600) as usize,
    };
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &ticket,
        &jsonwebtoken::EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(e.into()))?;
    let scheme = std::env::var("BUILDER_PREVIEW_SCHEME").unwrap_or_else(|_| "https".into());
    Ok(Json(
        json!({"url":format!("{scheme}://p-{preview_id}.{}?ticket={token}", preview_domain()?)}),
    ))
}

pub async fn preview_host_middleware(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let domain = match preview_domain() {
        Ok(value) => value,
        Err(_) => return next.run(req).await,
    };
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let suffix = format!(".{domain}");
    if !host
        .split(':')
        .next()
        .unwrap_or_default()
        .ends_with(&suffix)
    {
        return next.run(req).await;
    }
    let Some(preview_id) = preview_id_from_host(host, &domain) else {
        return AppError::NotFound("preview host invalid".into()).into_response();
    };
    let query = req.uri().query().unwrap_or_default();
    let ticket_param = reqwest::Url::parse(&format!("https://preview.invalid/?{query}"))
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "ticket")
                .map(|(_, value)| value.into_owned())
        });
    let cookie_token = req
        .headers()
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .map(str::trim)
                .find_map(|cookie| cookie.strip_prefix("builder_preview_session="))
        })
        .map(str::to_owned);
    let Some(token) = ticket_param.as_deref().or(cookie_token.as_deref()) else {
        return AppError::Unauthorized("preview ticket required".into()).into_response();
    };
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.set_audience(&["builder-preview"]);
    let claims = match jsonwebtoken::decode::<PreviewTicket>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &validation,
    ) {
        Ok(value) => value.claims,
        Err(_) => return AppError::Unauthorized("invalid preview ticket".into()).into_response(),
    };
    if claims.preview_id != preview_id {
        return AppError::Forbidden("preview ticket mismatch".into()).into_response();
    }
    let address = match preview_upstream(&state, claims.sub, preview_id).await {
        Ok(address) => address,
        Err(error) => return error.into_response(),
    };
    if ticket_param.is_some() {
        let secure =
            std::env::var("BUILDER_PREVIEW_SCHEME").unwrap_or_else(|_| "https".into()) == "https";
        let cookie = format!(
            "builder_preview_session={token}; Path=/; HttpOnly; SameSite={}; Max-Age=21600{}",
            if secure { "None" } else { "Lax" },
            if secure { "; Secure" } else { "" }
        );
        let mut response = Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, req.uri().path())
            .header(header::SET_COOKIE, cookie)
            .header(header::REFERRER_POLICY, "no-referrer")
            .body(axum::body::Body::empty())
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }
    let path = req
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/")
        .to_owned();
    let origin = std::env::var("BUILDER_BROWSER_ORIGIN").unwrap_or_default();
    let is_ws = req
        .headers()
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    if is_ws {
        use axum::extract::FromRequestParts;
        let (mut parts, _) = req.into_parts();
        let protocols = parts
            .headers
            .get(header::SEC_WEBSOCKET_PROTOCOL)
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                value
                    .split(',')
                    .map(|item| item.trim().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let Ok(ws) = WebSocketUpgrade::from_request_parts(&mut parts, &state).await else {
            return AppError::BadRequest("invalid preview WebSocket".into()).into_response();
        };
        return ws
            .protocols(protocols.clone())
            .on_upgrade(move |socket| proxy_preview_ws(socket, address, path, protocols))
            .into_response();
    }
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(body) => body,
        Err(_) => return AppError::BadRequest("preview request too large".into()).into_response(),
    };
    let client = match git_api_client() {
        Ok(client) => client,
        Err(error) => return error.into_response(),
    };
    let mut outbound = client
        .request(method, format!("http://{address}{path}"))
        .body(body);
    for name in [header::ACCEPT, header::CONTENT_TYPE] {
        if let Some(value) = headers.get(&name) {
            outbound = outbound.header(name, value);
        }
    }
    let upstream = match outbound.send().await {
        Ok(response) => response,
        Err(error) => return AppError::BadGateway(error.to_string()).into_response(),
    };
    let status = upstream.status();
    let mut response_headers = Vec::new();
    for name in [
        header::CONTENT_TYPE,
        header::LOCATION,
        header::SET_COOKIE,
        header::CACHE_CONTROL,
    ] {
        if let Some(value) = upstream.headers().get(&name) {
            response_headers.push((name, value.clone()));
        }
    }
    let bytes = match upstream.bytes().await {
        Ok(bytes) if bytes.len() <= 10 * 1024 * 1024 => bytes,
        _ => return AppError::BadGateway("preview response too large".into()).into_response(),
    };
    let mut response = Response::builder()
        .status(status)
        .body(axum::body::Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    for (name, value) in response_headers {
        response.headers_mut().insert(name, value);
    }
    if let Ok(csp) = format!("frame-ancestors {origin}").parse() {
        response
            .headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, csp);
    }
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn proxy_preview_ws(
    browser: WebSocket,
    address: String,
    path: String,
    protocols: Vec<String>,
) {
    let uri = format!("ws://{address}{path}");
    let Ok(mut request) = uri.into_client_request() else {
        return;
    };
    if !protocols.is_empty() {
        if let Ok(value) = protocols.join(", ").parse() {
            request
                .headers_mut()
                .insert(header::SEC_WEBSOCKET_PROTOCOL, value);
        }
    }
    let Ok((runner, _)) = tokio_tungstenite::connect_async(request).await else {
        return;
    };
    let (mut browser_tx, mut browser_rx) = browser.split();
    let (mut runner_tx, mut runner_rx) = runner.split();
    loop {
        tokio::select! {
            from_browser = browser_rx.next() => match from_browser {
                Some(Ok(Message::Text(value))) => { if runner_tx.send(tokio_tungstenite::tungstenite::Message::Text(value.to_string().into())).await.is_err() { break; } },
                Some(Ok(Message::Binary(value))) => { if runner_tx.send(tokio_tungstenite::tungstenite::Message::Binary(value)).await.is_err() { break; } },
                Some(Ok(Message::Ping(value))) => { if runner_tx.send(tokio_tungstenite::tungstenite::Message::Ping(value)).await.is_err() { break; } },
                _ => break,
            },
            from_runner = runner_rx.next() => match from_runner {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(value))) => { if browser_tx.send(Message::Text(value.to_string().into())).await.is_err() { break; } },
                Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(value))) => { if browser_tx.send(Message::Binary(value)).await.is_err() { break; } },
                Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(value))) => { if browser_tx.send(Message::Ping(value)).await.is_err() { break; } },
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_inputs_reject_local_urls_and_unsafe_branches() {
        assert!(!check_git_url("file:///etc/passwd"));
        assert!(!check_git_url("https://localhost/repo.git"));
        assert!(!check_git_url("https://127.0.0.1/repo.git"));
        assert!(!check_git_url("https://user:secret@git.example/repo.git"));
        assert!(!check_branch("../main"));
        assert!(!check_branch("feature with spaces"));
        assert!(check_branch("builder/new-app"));
    }

    #[test]
    fn gitea_endpoint_preserves_https_port_and_repo_identity() {
        let repo = Repository {
            id: Uuid::now_v7(),
            owner_id: Uuid::now_v7(),
            name: "demo".into(),
            remote_url: "https://git.example:8443/owner/repo.git".into(),
            default_branch: "main".into(),
            author_name: "User".into(),
            author_email: "user@example.com".into(),
            created_at: chrono::Utc::now(),
        };
        assert_eq!(
            gitea_endpoint(&repo).expect("Gitea endpoint"),
            "https://git.example:8443/api/v1/repos/owner/repo/pulls"
        );
    }

    #[test]
    fn preview_hosts_bind_exactly_one_preview_identity() {
        let id = Uuid::now_v7();
        let domain = "preview.example.com";
        assert_eq!(
            preview_id_from_host(&format!("p-{id}.{domain}"), domain),
            Some(id)
        );
        assert_eq!(
            preview_id_from_host(&format!("p-{id}.{domain}:443"), domain),
            Some(id)
        );
        assert_eq!(
            preview_id_from_host(&format!("p-{id}.evil.{domain}"), domain),
            None
        );
        assert_eq!(
            preview_id_from_host(&format!("p-{id}.example.com"), domain),
            None
        );
    }
}
