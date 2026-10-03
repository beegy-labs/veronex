use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Clone)]
struct App {
    root: PathBuf,
    cli: Arc<Mutex<CliState>>,
    previews: Arc<Mutex<HashMap<Uuid, Preview>>>,
    output: broadcast::Sender<Vec<u8>>,
    mode: String,
}

struct CliState {
    name: String,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
}

struct Preview {
    name: String,
    port: u16,
    child: tokio::process::Child,
}

#[derive(Deserialize)]
struct SwitchRequest {
    cli: String,
}
#[derive(Deserialize)]
struct PreviewRequest {
    id: Uuid,
    name: String,
    command: String,
    port: u16,
}
#[derive(Deserialize)]
struct GitRequest {
    message: Option<String>,
    remote: Option<String>,
}
#[derive(Deserialize)]
struct ResizeRequest {
    rows: u16,
    cols: u16,
}

fn err(status: StatusCode, message: impl ToString) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"error": message.to_string()})))
}

fn allowed_cli(name: &str) -> Option<&'static str> {
    match name {
        "codex" => Some("codex"),
        "claude" => Some("claude"),
        "gemini" => Some("gemini"),
        "local" => Some("veronex-local-cli"),
        _ => None,
    }
}

fn prepare_root(root: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(root.join("repo"))?;
    fs::create_dir_all(root.join("home"))?;
    fs::create_dir_all(root.join("history"))?;
    fs::create_dir_all(root.join("previews"))?;
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> anyhow::Result<()> {
    if !from.exists() {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let dest = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

fn checkpoint_cli(app: &App) -> anyhow::Result<()> {
    {
        let mut state = app
            .cli
            .lock()
            .map_err(|_| anyhow::anyhow!("CLI lock poisoned"))?;
        if let Some(mut child) = state.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        state.master = None;
        state.writer = None;
    }
    let sqlite = Path::new("/workspace/sqlite");
    if sqlite.exists() {
        let target = app.root.join("sqlite-backup");
        let pending = app.root.join("sqlite-backup-pending");
        if pending.exists() {
            fs::remove_dir_all(&pending)?;
        }
        copy_tree(sqlite, &pending)?;
        if target.exists() {
            fs::remove_dir_all(&target)?;
        }
        fs::rename(pending, target)?;
    }
    Ok(())
}

async fn checkpoint(State(app): State<App>, headers: HeaderMap) -> impl IntoResponse {
    if !internal_auth(&headers) || app.mode != "cli" {
        return err(StatusCode::UNAUTHORIZED, "unavailable").into_response();
    }
    match checkpoint_cli(&app) {
        Ok(()) => (StatusCode::OK, Json(json!({"checkpointed": true}))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

fn configure_retention(root: &Path) -> anyhow::Result<()> {
    let home = root.join("home");
    let claude_path = home.join(".claude/settings.json");
    let gemini_path = home.join(".gemini/settings.json");
    for path in [&claude_path, &gemini_path] {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut claude: Value = if claude_path.exists() {
        serde_json::from_slice(&fs::read(&claude_path)?)?
    } else {
        json!({})
    };
    let Some(claude_settings) = claude.as_object_mut() else {
        anyhow::bail!("Claude settings must be a JSON object");
    };
    claude_settings.insert("cleanupPeriodDays".into(), json!(3650));
    fs::write(&claude_path, serde_json::to_vec_pretty(&claude)?)?;
    let mut gemini: Value = if gemini_path.exists() {
        serde_json::from_slice(&fs::read(&gemini_path)?)?
    } else {
        json!({})
    };
    let Some(gemini_settings) = gemini.as_object_mut() else {
        anyhow::bail!("Gemini settings must be a JSON object");
    };
    let general = gemini_settings
        .entry("general")
        .or_insert_with(|| json!({}));
    let Some(general) = general.as_object_mut() else {
        anyhow::bail!("Gemini general settings must be a JSON object");
    };
    let retention = general
        .entry("sessionRetention")
        .or_insert_with(|| json!({}));
    let Some(retention) = retention.as_object_mut() else {
        anyhow::bail!("Gemini sessionRetention must be a JSON object");
    };
    retention.insert("enabled".into(), json!(false));
    fs::write(&gemini_path, serde_json::to_vec_pretty(&gemini)?)?;
    Ok(())
}

fn initialize_checkout(root: &Path) -> anyhow::Result<()> {
    let remote = std::env::var("BUILDER_REMOTE_URL")?;
    let branch = std::env::var("BUILDER_BRANCH")?;
    if root.join("repo/.git").exists() {
        return Ok(());
    }
    let helper = "!veronex-builder-runner credential";
    let mut clone = std::process::Command::new("git");
    clone.args([
        "-c",
        &format!("credential.helper={helper}"),
        "clone",
        "--",
        &remote,
        root.join("repo").to_string_lossy().as_ref(),
    ]);
    let status = clone.status()?;
    if !status.success() {
        anyhow::bail!("Git clone failed");
    }
    let status = std::process::Command::new("git")
        .args(["config", "credential.helper", helper])
        .current_dir(root.join("repo"))
        .status()?;
    if !status.success() {
        anyhow::bail!("Git credential configuration failed");
    }
    for (key, value) in [
        ("user.name", std::env::var("BUILDER_AUTHOR_NAME")?),
        ("user.email", std::env::var("BUILDER_AUTHOR_EMAIL")?),
    ] {
        let status = std::process::Command::new("git")
            .args(["config", key, &value])
            .current_dir(root.join("repo"))
            .status()?;
        if !status.success() {
            anyhow::bail!("Git author configuration failed");
        }
    }
    let status = std::process::Command::new("git")
        .args(["checkout", "-B", &branch])
        .current_dir(root.join("repo"))
        .status()?;
    if !status.success() {
        anyhow::bail!("Git branch checkout failed");
    }
    Ok(())
}

#[derive(Serialize)]
struct NativeEvent {
    cli: String,
    source_path: String,
    source_hash: String,
    source_format: String,
    payload: Value,
}

fn native_roots(root: &Path) -> [(String, PathBuf); 3] {
    let home = root.join("home");
    [
        ("codex".into(), home.join(".codex/sessions")),
        ("claude".into(), home.join(".claude/projects")),
        ("gemini".into(), home.join(".gemini/tmp")),
    ]
}

fn scan_native(root: &Path) -> Vec<NativeEvent> {
    let mut events = Vec::new();
    for (cli, base) in native_roots(root) {
        let mut stack = vec![base.clone()];
        let mut files = 0;
        while let Some(path) = stack.pop() {
            if files >= 100 || events.len() >= 2000 {
                events.push(NativeEvent {
                    cli: cli.clone(),
                    source_path: base.strip_prefix(root).unwrap_or(&base).to_string_lossy().to_string(),
                    source_hash: format!("scan-limit-{files}"),
                    source_format: "capture-gap".into(),
                    payload: json!({"capture_gap":"native history scan limit reached; original files remain on volume"}),
                });
                break;
            }
            if path.is_dir() {
                if let Ok(entries) = fs::read_dir(path) {
                    for entry in entries.flatten() {
                        stack.push(entry.path());
                    }
                }
                continue;
            }
            let format = match path.extension().and_then(|value| value.to_str()) {
                Some("jsonl") => "jsonl",
                Some("json") if cli == "gemini" => "json",
                _ => continue,
            };
            files += 1;
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if metadata.len() > 8 * 1024 * 1024 {
                events.push(NativeEvent { cli: cli.clone(), source_path: relative, source_hash: format!("oversize-{}", metadata.len()),
                    source_format: format.into(), payload: json!({"capture_gap":"native file exceeds 8 MiB; original remains on volume"}) });
                continue;
            }
            let Ok(contents) = fs::read(&path) else {
                continue;
            };
            let chunks: Vec<&[u8]> = if format == "jsonl" {
                contents.split(|byte| *byte == b'\n').collect()
            } else {
                vec![&contents]
            };
            for chunk in chunks {
                if events.len() >= 2000 {
                    break;
                }
                let Ok(payload) = serde_json::from_slice::<Value>(chunk) else {
                    continue;
                };
                events.push(NativeEvent {
                    cli: cli.clone(),
                    source_path: relative.clone(),
                    source_hash: format!("{:x}", Sha256::digest(chunk)),
                    source_format: format.into(),
                    payload,
                });
            }
        }
    }
    events
}

async fn native_history(State(app): State<App>, headers: HeaderMap) -> impl IntoResponse {
    if !internal_auth(&headers) || app.mode != "cli" {
        return err(StatusCode::UNAUTHORIZED, "unavailable").into_response();
    }
    Json(json!({"events": scan_native(&app.root)})).into_response()
}

fn start_cli(app: &App, name: &str) -> anyhow::Result<()> {
    let binary = allowed_cli(name).ok_or_else(|| anyhow::anyhow!("unsupported CLI"))?;
    let system = native_pty_system();
    let pair = system.openpty(PtySize {
        rows: 32,
        cols: 120,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command = CommandBuilder::new(binary);
    if name == "claude" {
        let tmp = app.root.join("home/claude-tmp");
        fs::create_dir_all(&tmp)?;
        command.env("CLAUDE_CODE_TMPDIR", tmp);
    }
    if name == "codex" {
        command.arg("--config");
        command.arg("sqlite_home=/workspace/sqlite");
    }
    let has_native = scan_native(&app.root).iter().any(|event| event.cli == name);
    let handoff_prompt = app
        .root
        .join("history/latest-handoff.md")
        .exists()
        .then_some("Read /workspace/history/latest-handoff.md, preserve its requirements and unfinished work, then continue in this repository.");
    if has_native {
        match name {
            "codex" => {
                command.arg("resume");
                command.arg("--last");
                if let Some(prompt) = handoff_prompt {
                    command.arg(prompt);
                }
            }
            "claude" => {
                command.arg("--continue");
                if let Some(prompt) = handoff_prompt {
                    command.arg(prompt);
                }
            }
            "gemini" => {
                command.arg("--resume");
                command.arg("latest");
                if let Some(prompt) = handoff_prompt {
                    command.arg("--prompt-interactive");
                    command.arg(prompt);
                }
            }
            _ => {}
        }
    } else if let Some(prompt) = handoff_prompt {
        match name {
            "codex" | "claude" => {
                command.arg(prompt);
            }
            "gemini" => {
                command.arg("--prompt-interactive");
                command.arg(prompt);
            }
            _ => {}
        }
    }
    command.cwd(app.root.join("repo"));
    command.env("HOME", app.root.join("home"));
    command.env("TERM", "xterm-256color");
    let child = pair.slave.spawn_command(command)?;
    let mut reader = pair.master.try_clone_reader()?;
    let tx = app.output.clone();
    let path = app
        .root
        .join("history")
        .join(format!("{name}.terminal.log"));
    std::thread::spawn(move || {
        let mut file = OpenOptions::new().create(true).append(true).open(path).ok();
        let mut buffer = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0 {
                break;
            }
            if let Some(ref mut file) = file {
                let _ = file.write_all(&buffer[..n]);
            }
            let _ = tx.send(buffer[..n].to_vec());
        }
    });
    let writer = pair.master.take_writer()?;
    let mut state = app
        .cli
        .lock()
        .map_err(|_| anyhow::anyhow!("CLI lock poisoned"))?;
    state.name = name.to_owned();
    state.master = Some(pair.master);
    state.writer = Some(writer);
    state.child = Some(child);
    Ok(())
}

async fn terminal(
    State(app): State<App>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if app.mode != "cli" {
        return err(StatusCode::NOT_FOUND, "terminal unavailable").into_response();
    }
    if !internal_auth(&headers) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    ws.on_upgrade(move |socket| terminal_socket(app, socket))
        .into_response()
}

fn internal_auth(headers: &HeaderMap) -> bool {
    let Some(expected) = std::env::var("BUILDER_RUNNER_TOKEN").ok() else {
        return false;
    };
    let bearer = format!("Bearer {expected}");
    headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(bearer.as_str())
}

async fn terminal_socket(app: App, socket: WebSocket) {
    let (mut send, mut receive) = socket.split();
    let name = app
        .cli
        .lock()
        .ok()
        .map(|s| s.name.clone())
        .unwrap_or_default();
    let path = app
        .root
        .join("history")
        .join(format!("{name}.terminal.log"));
    if let Ok(mut file) = File::open(path) {
        let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        let _ = file.seek(SeekFrom::Start(len.saturating_sub(131_072)));
        let mut replay = Vec::new();
        if file.read_to_end(&mut replay).is_ok()
            && send.send(Message::Binary(replay.into())).await.is_err()
        {
            return;
        }
    }
    let mut output = app.output.subscribe();
    loop {
        tokio::select! {
            incoming = receive.next() => match incoming {
                Some(Ok(Message::Text(text))) => write_pty(&app, text.as_bytes()),
                Some(Ok(Message::Binary(bytes))) => write_pty(&app, &bytes),
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {},
            },
            chunk = output.recv() => if let Ok(chunk) = chunk {
                if send.send(Message::Binary(chunk.into())).await.is_err() { break; }
            },
        }
    }
}

fn write_pty(app: &App, bytes: &[u8]) {
    if let Ok(mut state) = app.cli.lock() {
        if let Some(writer) = state.writer.as_mut() {
            let _ = writer.write_all(bytes);
        }
    }
}

async fn resize(
    State(app): State<App>,
    headers: HeaderMap,
    Json(req): Json<ResizeRequest>,
) -> impl IntoResponse {
    if !internal_auth(&headers) {
        return StatusCode::UNAUTHORIZED;
    }
    if let Ok(state) = app.cli.lock() {
        if let Some(master) = &state.master {
            if master
                .resize(PtySize {
                    rows: req.rows.max(1),
                    cols: req.cols.max(1),
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .is_ok()
            {
                return StatusCode::NO_CONTENT;
            }
        }
    }
    StatusCode::CONFLICT
}

fn create_handoff(app: &App, previous: &str, next: &str) -> anyhow::Result<()> {
    let log = app
        .root
        .join("history")
        .join(format!("{previous}.terminal.log"));
    let mut transcript = Vec::new();
    if let Ok(mut file) = File::open(log) {
        let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        let _ = file.seek(SeekFrom::Start(len.saturating_sub(64 * 1024)));
        let _ = file.read_to_end(&mut transcript);
    }
    let start = 0;
    let status = std::process::Command::new("git")
        .args(["status", "--short", "--branch"])
        .current_dir(app.root.join("repo"))
        .output()?;
    let native: Vec<_> = scan_native(&app.root)
        .into_iter()
        .filter(|event| event.cli == previous)
        .rev()
        .take(20)
        .map(|event| event.payload.to_string())
        .collect();
    let native_tail = native.join("\n");
    let native_start = native_tail.len().saturating_sub(64 * 1024);
    let handoff = format!(
        "# Workspace handoff\n\nPrevious CLI: {previous}\nNext CLI: {next}\n\n## Git status\n\n{}\n\n## Recent native records\n\n{}\n\n## Recent terminal output\n\n{}\n",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&native_tail.as_bytes()[native_start..]),
        String::from_utf8_lossy(&transcript[start..])
    );
    let path = app.root.join("history").join("latest-handoff.md");
    fs::write(path, handoff)?;
    Ok(())
}

async fn switch_cli(
    State(app): State<App>,
    headers: HeaderMap,
    Json(req): Json<SwitchRequest>,
) -> impl IntoResponse {
    if !internal_auth(&headers) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    if app.mode != "cli" || allowed_cli(&req.cli).is_none() {
        return err(StatusCode::BAD_REQUEST, "invalid CLI").into_response();
    }
    let previous = app
        .cli
        .lock()
        .ok()
        .map(|state| state.name.clone())
        .unwrap_or_default();
    if let Err(e) = checkpoint_cli(&app) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
    }
    if let Err(e) = create_handoff(&app, &previous, &req.cli) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
    }
    match start_cli(&app, &req.cli) {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"cli": req.cli, "handoff": "/workspace/history/latest-handoff.md"})),
        )
            .into_response(),
        Err(e) => err(StatusCode::SERVICE_UNAVAILABLE, e).into_response(),
    }
}

async fn git(
    State(app): State<App>,
    headers: HeaderMap,
    UrlPath(action): UrlPath<String>,
    Json(req): Json<GitRequest>,
) -> impl IntoResponse {
    if !internal_auth(&headers) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let mut command = tokio::process::Command::new("git");
    command.current_dir(app.root.join("repo"));
    match action.as_str() {
        "status" => {
            command.args(["status", "--short", "--branch"]);
        }
        "diff" => {
            command.args(["diff", "--stat", "HEAD"]);
        }
        "head" => {
            command.args(["rev-parse", "HEAD"]);
        }
        "fetch" => {
            command.args(["fetch", "--prune", "origin"]);
        }
        "commit" => {
            let Some(message) = req.message.as_deref().filter(|m| !m.trim().is_empty()) else {
                return err(StatusCode::BAD_REQUEST, "message required").into_response();
            };
            let lower = message.to_ascii_lowercase();
            if ["ai", "llm", "codex", "claude", "gpt", "copilot"]
                .iter()
                .any(|word| {
                    lower
                        .split(|c: char| !c.is_ascii_alphanumeric())
                        .any(|part| part == *word)
                })
            {
                return err(
                    StatusCode::BAD_REQUEST,
                    "commit message violates repository policy",
                )
                .into_response();
            }
            let staged = tokio::process::Command::new("git")
                .args(["add", "-A"])
                .current_dir(app.root.join("repo"))
                .output()
                .await;
            if !matches!(staged, Ok(ref output) if output.status.success()) {
                return err(StatusCode::CONFLICT, "Git staging failed").into_response();
            }
            command.args(["commit", "-m", message]);
        }
        "push" => {
            command.args(["push", "-u", "origin", "HEAD"]);
        }
        _ => return err(StatusCode::NOT_FOUND, "unknown git action").into_response(),
    }
    if req.remote.is_some() {
        return err(StatusCode::BAD_REQUEST, "remote override unsupported").into_response();
    }
    match command.output().await {
        Ok(output) => (if output.status.success() { StatusCode::OK } else { StatusCode::CONFLICT },
            Json(json!({"stdout": String::from_utf8_lossy(&output.stdout), "stderr": String::from_utf8_lossy(&output.stderr)}))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn start_preview(
    State(app): State<App>,
    headers: HeaderMap,
    Json(req): Json<PreviewRequest>,
) -> impl IntoResponse {
    if !internal_auth(&headers) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    if app.mode != "preview" {
        return err(StatusCode::NOT_FOUND, "preview unavailable").into_response();
    }
    if req.command.trim().is_empty() || req.port < 1024 {
        return err(StatusCode::BAD_REQUEST, "invalid preview profile").into_response();
    }
    let mut previews = app.previews.lock().expect("preview lock");
    previews.retain(|_, preview| preview.child.try_wait().ok().flatten().is_none());
    if let Some(existing) = previews.get(&req.id) {
        return (
            StatusCode::OK,
            Json(json!({"id": req.id, "name": existing.name, "port": existing.port})),
        )
            .into_response();
    }
    if previews.values().any(|p| p.port == req.port) {
        return err(StatusCode::CONFLICT, "port already in use").into_response();
    }
    let log = app.root.join("previews").join(format!("{}.log", req.id));
    let stdout = match OpenOptions::new().create(true).append(true).open(&log) {
        Ok(file) => file,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    let stderr = match stdout.try_clone() {
        Ok(file) => file,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    let mut command = tokio::process::Command::new("sh");
    command
        .args(["-lc", &req.command])
        .current_dir(app.root.join("repo"));
    command
        .env_clear()
        .env("HOME", app.root.join("home"))
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("PORT", req.port.to_string())
        .env("HOST", "0.0.0.0");
    command.stdout(stdout).stderr(stderr);
    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return err(StatusCode::BAD_REQUEST, e).into_response(),
    };
    previews.insert(
        req.id,
        Preview {
            name: req.name.clone(),
            port: req.port,
            child,
        },
    );
    (
        StatusCode::CREATED,
        Json(json!({"id": req.id, "name": req.name, "port": req.port})),
    )
        .into_response()
}

async fn preview_logs(
    State(app): State<App>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<Uuid>,
) -> impl IntoResponse {
    if !internal_auth(&headers) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let path = app.root.join("previews").join(format!("{id}.log"));
    let Ok(mut file) = File::open(path) else {
        return err(StatusCode::NOT_FOUND, "logs not found").into_response();
    };
    let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(len.saturating_sub(64 * 1024)));
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "log read failed").into_response();
    }
    Json(json!({"logs": String::from_utf8_lossy(&bytes)})).into_response()
}

async fn stop_preview(
    State(app): State<App>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<Uuid>,
) -> impl IntoResponse {
    if !internal_auth(&headers) {
        return StatusCode::UNAUTHORIZED;
    }
    if let Some(mut preview) = app.previews.lock().expect("preview lock").remove(&id) {
        let _ = preview.child.start_kill();
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn list_previews(State(app): State<App>, headers: HeaderMap) -> impl IntoResponse {
    if !internal_auth(&headers) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let mut previews = app.previews.lock().expect("preview lock");
    let rows: Vec<_> = previews
        .iter_mut()
        .map(|(id, p)| {
            let running = p.child.try_wait().ok().flatten().is_none();
            let ready = running
                && std::net::TcpStream::connect_timeout(
                    &std::net::SocketAddr::from(([127, 0, 0, 1], p.port)),
                    std::time::Duration::from_millis(100),
                )
                .is_ok();
            json!({"id": id, "name": p.name, "port": p.port, "running": running, "ready": ready})
        })
        .collect();
    Json(json!({"previews": rows})).into_response()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("credential") {
        if let Ok(token) = std::env::var("BUILDER_GIT_TOKEN")
            && !token.is_empty()
        {
            println!("username=oauth2\npassword={token}\n");
        }
        return Ok(());
    }
    tracing_subscriber::fmt().with_env_filter("info").init();
    let root = PathBuf::from(
        std::env::var("BUILDER_WORKSPACE_ROOT").unwrap_or_else(|_| "/workspace".into()),
    );
    let mode = std::env::var("BUILDER_RUNNER_MODE").unwrap_or_else(|_| "cli".into());
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| {
            if mode == "cli" {
                "7777".into()
            } else {
                "7778".into()
            }
        })
        .parse()?;
    prepare_root(&root)?;
    let (output, _) = broadcast::channel(512);
    let app = App {
        root,
        cli: Arc::new(Mutex::new(CliState {
            name: String::new(),
            master: None,
            writer: None,
            child: None,
        })),
        previews: Arc::new(Mutex::new(HashMap::new())),
        output,
        mode,
    };
    if app.mode == "init" {
        initialize_checkout(&app.root)?;
        configure_retention(&app.root)?;
        copy_tree(
            &app.root.join("sqlite-backup"),
            Path::new("/workspace/sqlite"),
        )?;
        return Ok(());
    }
    if app.mode == "cli" {
        configure_retention(&app.root)?;
        let initial = std::env::var("BUILDER_INITIAL_CLI").unwrap_or_else(|_| "codex".into());
        start_cli(&app, &initial)?;
    }
    let shutdown_app = app.clone();
    let router = Router::new()
        .route("/health", get(|| async { StatusCode::OK }))
        .route("/terminal", get(terminal))
        .route("/terminal/resize", post(resize))
        .route("/cli/switch", post(switch_cli))
        .route("/checkpoint", post(checkpoint))
        .route("/history", get(native_history))
        .route("/git/{action}", post(git))
        .route("/previews", get(list_previews).post(start_preview))
        .route("/previews/{id}/stop", post(stop_preview))
        .route("/previews/{id}/logs", get(preview_logs))
        .with_state(app);
    axum::serve(
        tokio::net::TcpListener::bind(("0.0.0.0", port)).await?,
        router,
    )
    .with_graceful_shutdown(async move {
        #[cfg(unix)]
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            term.recv().await;
            if shutdown_app.mode == "cli" {
                let _ = checkpoint_cli(&shutdown_app);
            }
        }
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_history_keeps_provider_identity_and_detects_capture_gaps() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let codex = temp.path().join("home/.codex/sessions/2026/09/28");
        let claude = temp.path().join("home/.claude/projects/test");
        let gemini = temp.path().join("home/.gemini/tmp/project/chats");
        for path in [&codex, &claude, &gemini] {
            fs::create_dir_all(path).expect("directory");
        }
        fs::write(
            codex.join("rollout.jsonl"),
            "{\"type\":\"user\",\"text\":\"retain this\"}\n",
        )
        .expect("codex history");
        fs::write(
            claude.join("session.jsonl"),
            "{\"type\":\"assistant\",\"text\":\"done\"}\n",
        )
        .expect("claude history");
        fs::write(
            gemini.join("session.json"),
            "{\"messages\":[{\"text\":\"hello\"}]}",
        )
        .expect("gemini history");
        let events = scan_native(temp.path());
        assert_eq!(events.len(), 3);
        assert!(
            events
                .iter()
                .any(|event| event.cli == "codex" && event.payload["text"] == "retain this")
        );
        assert!(events.iter().any(|event| event.cli == "claude"));
        assert!(events.iter().any(|event| event.cli == "gemini"));
        fs::write(
            codex.join("oversize.jsonl"),
            vec![b'x'; 8 * 1024 * 1024 + 1],
        )
        .expect("oversize history");
        assert!(
            scan_native(temp.path())
                .iter()
                .any(|event| event.payload.get("capture_gap").is_some())
        );
    }
}
