# Agent Refactoring SDD

> **Status**: Pending | **Last Updated**: 2026-03-15
> **Branch**: feat/agent-refactoring (not yet created)
> **Scope**: only modify `crates/veronex-agent/src/`. No changes to Redpanda, OTel Collector, or ClickHouse.

---

## Design Principle: Graceful Degradation

**No server info (node-exporter) → basic features only**
**Server info available (node-exporter) → full core features**

```
┌─────────────────────────────────────────────────────────┐
│         Server info available (node-exporter healthy)    │
│                                                         │
│  ✅ VRAM-aware dispatch (routing based on live VRAM)    │
│  ✅ Thermal gate (auto-block above 85°C)                │
│  ✅ AIMD stabilization (load-based request rate tuning) │
│  ✅ Capacity learning (capacity learning & prediction)  │
│  ✅ ClickHouse metrics analysis                         │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│       No server info (node-exporter / agent failure)     │
│                                                         │
│  ✅ Inference request processing (basic dispatch cont.)  │
│  ✅ Static registered VRAM-based routing (stale fallback)│
│  ⚠️  Thermal gate disabled (no temperature data)         │
│  ⚠️  AIMD/capacity degraded (no metrics)                 │
│  ❌ Real-time VRAM analysis unavailable                  │
└─────────────────────────────────────────────────────────┘
```

> Agent and node-exporter are **support roles** — basic inference must continue on failure.
> Core features (thermal, VRAM-aware) activate only when server info is available.
> This behavior is already implemented in code but **not explicitly guaranteed/tested** → formalized in this refactoring.

---

## Goals

While maintaining current functionality:
1. **Formalize Graceful Degradation** — explicitly test basic feature guarantees when server info is unavailable
2. **Agent failure isolation** (K8s 3-probe auto-recovery for hung state)
3. Add agent self-observability (self-metrics)
4. Improve OTLP push reliability (retry)
5. Improve error handling consistency

---

## Failure Isolation Analysis

### Behavior on node-exporter failure

```
node-exporter DOWN
  ↓
health_checker: fetch_node_metrics() → Err → return (no Valkey update)
  ↓
Valkey cache 60s TTL expires → no hw data
  ↓
get_ollama_available_vram_mb() cache miss
  ├── total_vram_mb == 0 → i64::MAX (treated as unlimited, provider_router.rs:520)
  └── total_vram_mb  > 0 → static registered VRAM value (provider_router.rs:523)
  ↓
dispatcher: continues dispatch ✅
```

**Already works** — inference continues regardless of node-exporter failure.

| Item | node-exporter healthy | Failure (after 60s TTL expiry) |
|------|-------------------|----------------------|
| VRAM accuracy | Real-time | Static registered value (stale) |
| Thermal gate | Blocks above 85°C | **Disabled** (assumes 0°C) |
| Inference available | ✅ | ✅ |

> **Tradeoff**: thermal protection is disabled when node-exporter fails.
> Inference cannot be blocked when GPU temperature is unknown, so this behavior is intentional.
> Ollama itself also has built-in GPU overheat protection.

---

### Agent failure impact on veronex core functions

```
┌─────────────────────┐        ┌──────────────────────────────┐
│   veronex-agent     │        │   veronex (main service)     │
│                     │        │                              │
│ scrape → OTLP push  │──────► │  ClickHouse (analytics)      │
│                     │        │                              │
│ GET /v1/metrics/    │──────► │  target discovery API        │
│   targets           │        │  (query what agent scrapes)  │
└─────────────────────┘        │                              │
                                │  health_checker ─────────►  │
                                │  node-exporter direct poll   │
                                │    ↓                         │
                                │  Valkey (60s TTL)            │
                                │    ↓                         │
                                │  dispatcher (routing decision)│
                                └──────────────────────────────┘
```

| Agent failure scenario | Inference routing impact | Analytics data impact |
|----------------------|----------------|----------------|
| Agent crash/restart | **None** | ClickHouse collection temporarily paused |
| OTLP push failure | **None** | Data loss for that cycle |
| Target discovery failure | **None** (empty targets → skip) | That cycle skipped |
| Agent hung (infinite wait) | **None** (separate process) | K8s cannot detect ← problem |

**Conclusion**: agent is **fully decoupled** from veronex inference routing.
- Routing (thermal gate, VRAM check) is handled by veronex's internal `health_checker` polling node-exporter directly
- Agent is only responsible for ClickHouse analytics data

**However, current issue**: no K8s liveness probe → agent in hung state is never restarted.

---

## Current State Analysis

### Working well (no changes needed)

| Item | File | Status |
|------|------|------|
| Shard hashing logic | `shard.rs` | Correct — proptest 17/17 pass |
| Scrape loop structure | `main.rs` | `biased select` + graceful shutdown correct |
| DOS protection | `scraper.rs` | body size, label count, model count all limited |
| CPU mode filter | `scraper.rs` | only user/system/iowait/idle pass (55% volume reduction) |
| Metrics allowlist | `scraper.rs` | appropriate for GPU server monitoring |
| **Graceful Degradation** | `thermal.rs:260`, `provider_router.rs:520` | Without server info: `ThrottleLevel::Normal` + static VRAM — basic dispatch continues ✅ |

### Items Needing Improvement

#### 1. No agent self-observability (MEDIUM)

**Current**: no metrics representing the agent's own operational state.
Slow scrape cycles or OTLP push failures are invisible in ClickHouse.

**Self-metrics to add** (pushed via OTLP alongside node-exporter/Ollama metrics):

| Metric name | Type | Description |
|------------|------|------|
| `veronex_agent_scrape_duration_seconds` | gauge | Duration of last scrape cycle |
| `veronex_agent_scrape_targets_total` | gauge | Number of targets scraped this cycle |
| `veronex_agent_gauges_collected_total` | gauge | Number of gauges collected this cycle |
| `veronex_agent_otlp_push_errors_total` | gauge | Cumulative OTLP push failure count |
| `veronex_agent_uptime_seconds` | gauge | Time elapsed since agent start |

> No Redpanda/OTel changes — sent via existing OTLP pipeline.

#### 2. No OTLP push retry (LOW)

**`otlp.rs:77`**: single POST attempt, data lost on failure.

**Current**:
```rust
let resp = client.post(endpoint).json(&payload).send().await?;
// On failure, only error log, cycle continues
```

**Improvement**: 1 retry + 5s wait (2 attempts total). Excessive retries cause backpressure, so kept minimal.

```rust
for attempt in 0..2 {
    match push_once(client, endpoint, &payload).await {
        Ok(_) => return Ok(()),
        Err(e) if attempt == 0 => {
            tracing::warn!("otlp push failed, retrying in 5s: {e}");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        Err(e) => {
            tracing::error!("otlp push failed after retry: {e}");
            // increment error counter
        }
    }
}
```

#### 3. Silent on target discovery failure (LOW)

**`main.rs:80`**:
```rust
.json::<Vec<SdTarget>>().await.unwrap_or_default()
```

Returns empty vector on JSON parse failure — no logging.

**Improvement**: add `tracing::warn!` on parse failure:
```rust
match resp.json::<Vec<SdTarget>>().await {
    Ok(targets) => targets,
    Err(e) => {
        tracing::warn!(url = sd_url, "sd target parse failed: {e}");
        vec![]
    }
}
```

#### 4. No K8s probes (HIGH)

**`deploy/helm/veronex/templates/veronex-agent-statefulset.yaml`**: `startupProbe`, `livenessProbe`, `readinessProbe` all undefined.

- **No startupProbe** → K8s checks liveness immediately on container start → may restart before first scrape
- **No livenessProbe** → cannot detect scrape loop hung, permanent unhealthy state
- **No readinessProbe** → can receive traffic before first scrape completes (target discovery incomplete)

**Improvement**: add Health HTTP server + 3-probe configuration.

**`main.rs`** — run health server as separate tokio task (port `HEALTH_PORT`, default `9091`):

```rust
struct HealthState {
    start_time: Instant,
    first_scrape_done: bool,          // readiness
    last_scrape_at: Instant,          // liveness
}

// GET /startup → 200 (process alive), 503 (pre-init)
// GET /ready   → 200 (first scrape done), 503 (not yet)
// GET /health  → 200 (last scrape < 3min ago), 503 (hung)
```

**K8s probe design**:

| probe | Purpose | endpoint | Criterion |
|-------|------|----------|-----------|
| `startupProbe` | Wait for initialization (first scrape) | `GET /startup` | 200 if process is alive |
| `readinessProbe` | Confirm first scrape complete | `GET /ready` | `first_scrape_done = true` |
| `livenessProbe` | Detect hung + restart | `GET /health` | last scrape < 180s |

```yaml
# veronex-agent-statefulset.yaml
startupProbe:
  httpGet:
    path: /startup
    port: 9091
  failureThreshold: 12   # 12 × 5s = 60s to complete init or restart
  periodSeconds: 5

readinessProbe:
  httpGet:
    path: /ready
    port: 9091
  initialDelaySeconds: 5
  periodSeconds: 15
  failureThreshold: 2    # NotReady if no first scrape within 30s

livenessProbe:
  httpGet:
    path: /health
    port: 9091
  initialDelaySeconds: 60  # starts after startupProbe
  periodSeconds: 30
  failureThreshold: 3      # restart on 90s unresponsive
```

Port configured via env `HEALTH_PORT` (default: `9091`).

#### 5. OTLP error body masking (LOW)

**`otlp.rs:78`**: `resp.text().await.unwrap_or_default()` — error cause unclear when response body read fails.

**Improvement**: lower error body logging to `debug` level and provide explicit fallback message:
```rust
let body = resp.text().await.unwrap_or_else(|_| "<unreadable>".into());
tracing::warn!(status = status.as_u16(), body = %body, "otlp push rejected");
```

---

## Implementation Plan

### Phase 1 — Add self-metrics struct (`main.rs`)

```rust
/// Agent self-observation state — updated per scrape cycle.
struct AgentStats {
    start_time: std::time::Instant,
    otlp_push_errors: u64,
}
```

### Phase 2 — Collect cycle stats via scrape_cycle() return value (`main.rs`)

```rust
struct CycleResult {
    duration_secs: f64,
    targets_scraped: usize,
    gauges_collected: usize,
}
```

Change `scrape_cycle()` to return `CycleResult`.

### Phase 3 — Convert self-metrics to Gauge vector and include in existing OTLP push (`main.rs`)

```rust
fn agent_self_gauges(stats: &AgentStats, cycle: &CycleResult) -> Vec<Gauge> {
    vec![
        Gauge { name: "veronex_agent_uptime_seconds".into(),
                value: stats.start_time.elapsed().as_secs_f64(), labels: vec![] },
        Gauge { name: "veronex_agent_scrape_duration_seconds".into(),
                value: cycle.duration_secs, labels: vec![] },
        Gauge { name: "veronex_agent_scrape_targets_total".into(),
                value: cycle.targets_scraped as f64, labels: vec![] },
        Gauge { name: "veronex_agent_gauges_collected_total".into(),
                value: cycle.gauges_collected as f64, labels: vec![] },
        Gauge { name: "veronex_agent_otlp_push_errors_total".into(),
                value: stats.otlp_push_errors as f64, labels: vec![] },
    ]
}
```

Append to existing `gauges` vector via `extend` → reuse existing OTLP push code.

### Phase 4 — OTLP retry (`otlp.rs`)

Add 1 retry inside `push()`. Retry interval: 5s.
`AgentStats.otlp_push_errors` increment handled via callback or return value.

### Phase 5 — Error handling improvements (`main.rs`, `otlp.rs`)

- Target discovery JSON parse failure → add `tracing::warn!`
- OTLP response body read failure → explicit `"<unreadable>"` fallback

### Phase 6 — K8s 3-probe + health endpoint (`main.rs`, `statefulset.yaml`)

Add health HTTP server via tokio spawn in `main.rs` (axum minimal, port `HEALTH_PORT` default 9091).

Shared state:
```rust
struct HealthState {
    first_scrape_done: AtomicBool,
    last_scrape_at: Mutex<Instant>,
}
```

- `first_scrape_done` → set to `true` on first scrape cycle completion
- `last_scrape_at` → updated on each cycle completion

Endpoints:
- `GET /startup` → 200 if process alive (for startupProbe)
- `GET /ready` → 200 if `first_scrape_done`, else 503
- `GET /health` → 200 if `last_scrape_at` elapsed < 180s, else 503

Add 3 probes to `veronex-agent-statefulset.yaml`.
Add `veronexAgent.healthPort: 9091` to `values.yaml`.

Changed files:
- `crates/veronex-agent/src/main.rs`
- `deploy/helm/veronex/templates/veronex-agent-statefulset.yaml`
- `deploy/helm/veronex/values.yaml`

### Phase 7 — Test updates

- `scrape_cycle()` returns `CycleResult` → update existing test signatures
- `agent_self_gauges()` unit test: verify returned metric names/count
- OTLP retry: mock server first request fails → second succeeds
- `/health`: 200 on normal, 503 when 180s exceeded

---

## Changed Files

| File | Changes |
|------|-----------|
| `main.rs` | health server, `AgentStats`, `CycleResult`, `agent_self_gauges()`, target discovery warn |
| `otlp.rs` | 1 retry, response body error explicit |
| `scraper.rs` | No changes |
| `shard.rs` | No changes |
| `veronex-agent-statefulset.yaml` | `livenessProbe` added |
| `values.yaml` | `agent.healthPort: 9091` added |

---

## Not Changed

- Redpanda config, topics, retention
- OTel Collector config
- ClickHouse schema
- Existing metrics allowlist (node-exporter, Ollama)
- CPU mode filter (user/system/iowait/idle)
- Scrape interval default (30s)

---

## Tasks

| # | Task | File | Status |
|---|------|------|--------|
| 1 | `HealthState` shared struct + health HTTP server (`/startup`, `/ready`, `/health`) | `main.rs`, `health.rs` | **done** |
| 2 | `startupProbe` (GET /startup, 12×5s=60s) | `veronex-agent-statefulset.yaml` | **done** |
| 3 | `readinessProbe` (GET /ready, 15s period, 2× fail) | `veronex-agent-statefulset.yaml` | **done** |
| 4 | `livenessProbe` (GET /health, 30s period, 3× fail) | `veronex-agent-statefulset.yaml` | **done** |
| 5 | `veronexAgent.healthPort: 9091` values added | `values.yaml` | **done** |
| 6 | `AgentStats`, `CycleResult` structs added | `main.rs` | **done** |
| 7 | `scrape_cycle()` changed to return `CycleResult` | `main.rs` | **done** |
| 8 | `agent_self_gauges()` implemented + self-metrics OTLP push | `main.rs` | **done** |
| 9 | OTLP push 1 retry + 5s backoff | `otlp.rs` | **done** |
| 10 | OTLP response body error handling improved | `otlp.rs` | **done** |
| 11 | Target discovery JSON parse failure warn added | `main.rs` | **done** |
| 12 | **Graceful Degradation regression test**: verify `ThrottleLevel::Normal` returned without node-exporter | `thermal.rs` | **done** |
| 13 | **Graceful Degradation regression test**: verify static VRAM fallback on Valkey cache miss | `provider_router.rs` | **done** |
