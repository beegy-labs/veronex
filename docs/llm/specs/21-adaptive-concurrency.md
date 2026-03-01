# Spec 21 — Adaptive Concurrency: N-slot Auto-allocation for Local LLM

> **Status**: ⚠️ Superseded — implemented differently; see `docs/llm/backend/capacity.md`
> **Scope**: Ollama (local GPU/CPU) backends only. Gemini is cloud-managed, unaffected.
> Last updated: 2026-02-26 (archived 2026-03-02)

## ⚠️ Implementation Note (2026-03-02)

This spec was a roadmap. The feature was implemented on branch `feat/api-key-usage` with several
design differences from this spec. **Refer to `docs/llm/backend/capacity.md` for the authoritative SSOT.**

Key divergences from this spec:

| This spec | Actual implementation |
|-----------|----------------------|
| `DashMap<Uuid, (usize, usize)>` load counter | `DashMap<(Uuid, String), (Arc<Semaphore>, u32)>` per (backend, model) |
| `CapacityOracle` trait | `ConcurrencySlotMap` (infrastructure struct, no port trait) |
| VRAM memory snapshot at TTFT | `/api/show` arch params + KV cache formula (2 × layers × kv_heads × head_dim × 2) |
| ClickHouse history as primary source | `inference_jobs` PostgreSQL throughput aggregation (`PERCENTILE_CONT`) |
| `model_memory_profiles` table | `model_capacity` table (broader: includes KV arch params, LLM analysis) |
| Phase-based rollout | Delivered as single feature: slot_map + thermal + capacity_analyzer + settings API |
| No thermal throttle | `ThermalThrottleMap`: 85°C soft / 92°C hard / 78°C hysteresis + 60s cooldown |
| No LLM advisor | `qwen2.5:3b` background advisor (5-min loop, fail-open) |

---

---

## 1. Problem Statement

### Current behavior (v0.1)

```
queue_dispatcher_loop
  └─ BLPOP → pick 1 job
       └─ busy_backends: HashSet<BackendId>  ← 0 or 1 per backend
            └─ if backend not busy → claim → tokio::spawn(run_job)
            └─ if all busy       → LPUSH back + sleep 2s
```

**Limitations:**
- Each Ollama backend processes exactly **1 job at a time**, always.
- Modern GPUs (especially AMD Radeon AI Max+) have sufficient VRAM for 2–4 concurrent
  small-model jobs (e.g., two 7B models simultaneously or one 7B + one 3B).
- No memory history is consulted — the system never learns how much VRAM a model actually uses.
- Under-utilization: a 24 GB GPU running a 4B model (uses ~3 GB) sits 87% idle.

### Goal

> For each local backend, automatically derive **N** (the max concurrent job slots)
> from available hardware resources and historical per-model memory usage,
> and dispatch up to N jobs simultaneously.

---

## 2. Key Concepts

### 2.1 Slot-based concurrency

Replace the binary `busy / not-busy` model with a **slot counter**:

```
Before:  busy_backends: HashSet<BackendId>          → 0 or 1
After:   backend_load:  HashMap<BackendId, usize>   → 0..N
         backend_cap:   HashMap<BackendId, usize>   → N (computed)
```

`N = floor(available_resource / estimated_job_cost)`

A job can be dispatched to a backend when `load[id] < cap[id]`.

### 2.2 Resource dimensions

Two independent constraints, both must be satisfied:

| Resource | Source | Constraint |
|----------|--------|-----------|
| **VRAM** | node-exporter `node_drm_*` OR Ollama `/api/ps` | Primary for GPU inference |
| **RAM** | node-exporter `node_memory_MemAvailable_bytes` | CPU offload layers, KV cache |

Effective capacity = `min(vram_slots, ram_slots)`

```
vram_slots = floor(free_vram_mb  / estimated_job_vram_mb)
ram_slots  = floor(free_ram_mb   / estimated_job_ram_mb)
N          = min(vram_slots, ram_slots)
```

### 2.3 Estimated job memory

The cost of running one inference job for model `M` on backend `B`:

```
estimated_vram_mb(M, B) = model_load_vram_mb(M)
                        + avg_kv_cache_mb(M)   ← from history
                        + safety_margin_mb     ← configurable (default 512 MB)

estimated_ram_mb(M, B)  = cpu_offload_layers(M, B) * layer_size_mb
                        + estimated_vram_mb(M, B) * 0.5  ← conservative fallback
```

**Memory sources (priority order):**

1. **ClickHouse history** (`inference_logs`): aggregate of actual VRAM usage during past jobs
   for this `(model_name, backend_id)` pair.
2. **Ollama `/api/ps`** live: sizes of currently loaded model shards.
3. **Ollama model info** (`/api/show`): `parameter_size` + quantization → approximate footprint.
4. **Fallback**: `total_vram_mb / 2` (conservative, allows at most 2 jobs).

---

## 3. Data Model Changes

### 3.1 New: `model_memory_profiles` table

Persists the learned per-model memory footprint derived from job history.

```sql
CREATE TABLE model_memory_profiles (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    backend_id      UUID NOT NULL REFERENCES llm_backends(id) ON DELETE CASCADE,
    model_name      TEXT NOT NULL,
    -- Observed peak VRAM usage (MB) during inference
    p50_vram_mb     FLOAT NOT NULL DEFAULT 0,
    p95_vram_mb     FLOAT NOT NULL DEFAULT 0,
    -- Observed RAM usage (MB, for CPU-offloaded layers)
    p50_ram_mb      FLOAT NOT NULL DEFAULT 0,
    -- Sample statistics
    sample_count    INT  NOT NULL DEFAULT 0,
    last_updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (backend_id, model_name)
);
```

### 3.2 Extend `inference_jobs`

Add peak memory snapshots captured during job execution:

```sql
ALTER TABLE inference_jobs
    ADD COLUMN peak_vram_mb  FLOAT,   -- snapshot at inference midpoint
    ADD COLUMN peak_ram_mb   FLOAT;
```

### 3.3 Extend `inference_logs` (ClickHouse)

Add columns to the OTel-ingested event:

```
peak_vram_mb   Float32
peak_ram_mb    Float32
backend_id     String
model_name     String
```

---

## 4. Architecture

### 4.1 New component: `CapacityOracle`

A background service that computes and caches `N` for each backend.

```
Port:     application/ports/outbound/capacity_oracle.rs
Impl:     infrastructure/outbound/capacity/capacity_oracle.rs

Trait:
  async fn capacity_for(&self, backend_id: Uuid, model_name: &str) -> usize
  async fn refresh(&self) -> Result<()>
```

**Refresh trigger:**
- On job completion (release slot → recompute)
- Every 30 seconds (background ticker)
- On manual `/v1/backends/{id}/capacity/refresh` API call

**Computation (per backend):**

```rust
fn compute_capacity(
    free_vram_mb:  i64,
    free_ram_mb:   i64,
    job_vram_mb:   i64,   // from profile or fallback
    job_ram_mb:    i64,
    safety_mb:     i64,   // 512 MB default
) -> usize {
    if job_vram_mb == 0 { return 1; }           // unknown → conservative
    let vram_slots = (free_vram_mb - safety_mb).max(0) / job_vram_mb;
    let ram_slots  = (free_ram_mb  - safety_mb).max(0) / job_ram_mb.max(1);
    (vram_slots.min(ram_slots) as usize).max(1) // at least 1 slot always
}
```

### 4.2 Updated `queue_dispatcher_loop`

```
Before:
  loop:
    BLPOP → 1 job
    if backend not in busy_set → claim → spawn

After:
  loop:
    while any_backend_has_free_slot():
      BLPOP(timeout=0.1s) → 1 job
      pick best backend where load[id] < cap[id]
      if found → increment load[id] → spawn(run_job)
      else     → break inner loop
    sleep_until_slot_free_or_timeout(5s)
```

Key change: **inner loop** continuously drains the queue as long as any backend
has available slots. The outer loop resumes on slot release (via `tokio::Notify`).

```rust
// Slot counter type (replaces HashSet)
type BackendSlots = Arc<DashMap<Uuid, (usize, usize)>>;
//                                         ^      ^
//                                      current  capacity
```

### 4.3 Memory snapshot during `run_job`

At the midpoint of inference (after first token / TTFT), take a memory snapshot:

```rust
// After first token arrives:
let snapshot = hw_metrics::snapshot_vram(backend_id).await;
job.peak_vram_mb = Some(snapshot.vram_used_mb as f64);

// After job completes → update profile
capacity_oracle.record_sample(backend_id, model_name, peak_vram_mb).await;
```

---

## 5. Allocation Algorithm (Dispatch Decision)

```
For each queued job J with model M, backend_type Ollama:

1. List active Ollama backends → candidates[]
2. For each candidate B:
     free_slots(B) = cap(B, M) - load(B)
     score(B)      = free_vram(B) - estimated_vram(M)   ← "headroom"
3. Sort by score DESC
4. Pick first B where free_slots(B) > 0
5. If none → re-queue J, wait for slot_released notification
6. Else → increment load(B), spawn run_job(J, B)
```

**Tie-break rule:** Among backends with equal headroom, prefer the one with
lower `load / cap` ratio (less loaded relative to capacity).

---

## 6. Configuration

New env vars / backend record fields:

| Parameter | Default | Meaning |
|-----------|---------|---------|
| `SLOT_SAFETY_VRAM_MB` | `512` | Reserved VRAM buffer per backend (never allocate into) |
| `SLOT_SAFETY_RAM_MB` | `1024` | Reserved RAM buffer |
| `SLOT_HISTORY_WINDOW` | `100` | Number of recent jobs used for p50/p95 profile |
| `SLOT_REFRESH_SECS` | `30` | CapacityOracle background refresh interval |
| `llm_backends.max_concurrent` | `NULL` | Manual cap override (NULL = auto) |

---

## 7. Rollout Phases

### Phase 1 — Instrumentation (prerequisite)
- [ ] Add `peak_vram_mb`, `peak_ram_mb` to `inference_jobs`
- [ ] Snapshot VRAM at TTFT in `run_job`
- [ ] Create `model_memory_profiles` table + updater
- [ ] API: `GET /v1/backends/{id}/profile` → current model profiles

### Phase 2 — Static N (manual override)
- [ ] Add `max_concurrent: Option<usize>` to `LlmBackend`
- [ ] Replace `HashSet` with `DashMap<Uuid, (usize, usize)>`
- [ ] Inner-loop drain in dispatcher
- [ ] API: `PATCH /v1/backends/{id}` accepts `max_concurrent`
- [ ] Web: show current load / capacity badge on backend card

### Phase 3 — Dynamic N (auto from history)
- [ ] `CapacityOracle` trait + impl
- [ ] VRAM + RAM aware capacity computation
- [ ] Slot-release notification (`tokio::Notify`)
- [ ] Fallback chain: ClickHouse history → Ollama /api/ps → Ollama /api/show → `total_vram/2`

### Phase 4 — Web UI
- [ ] Backend card: `[2/3 slots]` load indicator
- [ ] Model memory profile table in Backends page
- [ ] Chart: concurrency over time (ClickHouse)

---

## 8. Example Scenario

**Setup:**
- Backend A: AMD Radeon AI Max+ 395, 24 GB VRAM, 128 GB RAM
- Models in use: `llama3.2:3b` (≈2.0 GB VRAM), `qwen2.5:7b` (≈4.5 GB VRAM)

**Auto-computed N for `llama3.2:3b`:**
```
free_vram   = 22,000 MB  (after OS + driver overhead)
safety      =    512 MB
job_vram    =  2,200 MB  (p95 from 100 job history)
vram_slots  = (22000 - 512) / 2200 = 9

free_ram    = 100,000 MB
job_ram     =  1,000 MB  (CPU offload minimal for 3B)
ram_slots   = (100000 - 1024) / 1000 = 98

N = min(9, 98) = 9
```

**Result:** 9 simultaneous `llama3.2:3b` jobs instead of 1. Throughput improvement: ~9×.

---

## 9. Relation to Current Code

| Current | Target |
|---------|--------|
| `busy_backends: Arc<Mutex<HashSet<Uuid>>>` | `backend_slots: Arc<DashMap<Uuid, (usize, usize)>>` |
| `if !busy.contains(&b.id) && avail > 0` | `if slots.load(id) < slots.cap(id)` |
| `busy.insert(id)` on claim | `slots.increment(id)` |
| `busy.remove(id)` on release | `slots.decrement(id); notify.notify_one()` |
| Single BLPOP per loop | Inner drain loop while slots available |
| No memory profile | `model_memory_profiles` table + `CapacityOracle` |
