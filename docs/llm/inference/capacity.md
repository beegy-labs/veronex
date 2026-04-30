# VRAM Pool Capacity Management

> SSOT | **Last Updated**: 2026-03-22 | Classification: Operational | Exception: >200 lines (capacity subsystem — VRAM pool, KV cache, thermal, AIMD, dispatch)

## Core Intent

Manage VRAM as a **global pool** per provider. Instead of fixed per-model slots, any model combination is allowed as long as VRAM is available.

```
128GB GPU Server
├── 70B Q4_K_M (40GB weight) + KV 640MB  → allocated ✓
├── 20B Q4_K_M (12GB weight) + KV 320MB  → allocated ✓
├── 20B Q4_K_M (already loaded) + KV 320MB  → KV only ✓
├── 40B Q5_K_M (28GB weight) + KV 480MB  → check remaining VRAM → allocated ✓
└── 70B extra request (already loaded) + KV 640MB  → KV only ✓
```

**Core rules**:
- Model **already loaded** → deduct **KV cache only**
- Model **not loaded** → deduct **weight + KV cache** (Ollama auto-loads)
- On completion → release **KV cache only** (weight stays in VRAM)

**Model lifecycle**: VramPool + provider `keep_alive` window (default `OLLAMA_KEEP_ALIVE=10m` per low-power policy; lifecycle probes use `LIFECYCLE_KEEP_ALIVE=30m`) manages model retention. `OllamaModelManager` is disabled — its `ensure_loaded(max_loaded=1)` sends `keep_alive=0` which physically unloads other models, destroying multi-model co-residence.

**Phase 1 entry point** (`MCP_LIFECYCLE_PHASE=on`, see `flows/model-lifecycle.md`):
- `OllamaAdapter::ensure_ready(model)` is the SSOT for "is model loaded on
  this provider". Warm hit → `VramPool::loaded_model_names` lookup; cold miss
  → zero-prompt `/api/generate` probe → on success, adapter calls
  `VramPool::record_loaded(provider_id, model)` so subsequent dispatches
  observe the model present.
- `OllamaAdapter::evict(model, reason)` is the eviction entry point and updates
  VramPool symmetrically.

---

## Phase 1: Initial Probe

Ollama has no GPU info API (issue #3822), so VRAM capacity is learned via probing.

### 1-1. After Provider Registration

```
POST /v1/servers (register provider)
  → GET {ollama_url}/api/ps     ← already loaded models
  → GET {ollama_url}/api/tags   ← available models + file sizes
```

- `/api/ps` has models: sum `size_vram` → minimum VRAM capacity
- `/api/ps` empty: VRAM capacity unknown, start with pass-through

### 1-2. Progressive VRAM Learning

| State | `total_mb` | Dispatch Behavior |
|-------|-----------|------------------|
| **Unknown** | `0` | Concurrency-headroom score — `available_vram_mb` returns `(max_concurrent - active) * 1_024 MB` (min 1); routing still works, delegates enforcement to Ollama |
| **Known** | `> 0` | Strict reservation — available VRAM checked before every dispatch |

`total_mb` is set by the 30s sync loop. DB column `weight_estimated: bool` tracks whether per-model weight was measured or estimated, but is not consulted at dispatch time.

**VRAM total — priority order** (SSOT precedence; SDD `.specs/veronex/vram-total-ssot-priority-restoration.md` §3.1):

| # | Source | Used when |
|---|--------|-----------|
| 1 | **`llm_providers.total_vram_mb`** (operator-registered, `vram_total_source = manual`) | `> 0` — declared envelope, takes precedence over auto-detection |
| 2 | **agent-pushed mirror** (`veronex-agent` discovery label `total_vram_mb`) | provider DB value 0 but agent has value (analyzer cache miss / staleness window) |
| 3 | **node-exporter DRM** (`node_drm_memory_vram_total_bytes` / `vram_size_bytes`) | unset operator + agent → pass-through; non-APU host |
| 4 | **APU** (`mem_available_mb` from node-exporter, unified memory) | unset operator + agent → pass-through; AMD APU detected (`drm > 0 && mem_avail > drm × 2`) |
| 5 | **Unknown** (no source) | `total_mb = 0` → vram_pool delegates capacity to Ollama (request still dispatches) |

The operator-registered value is the **declared envelope**: AIMD `max_concurrent`, `safety_permil` (auto +50 on OOM, decay −10/cycle), and Ollama's own OOM rejection together provide dynamic correction within the envelope. Inverted priority (auto-detect over operator value) was a regression introduced in commit `4891fbc` and reverted in this SDD.

---

## Phase 2: First Request (No Data)

### 2-1. Unknown VRAM → Pass-Through

```
if provider.vram_total == None:
    dispatch(request)        # Ollama's own scheduler handles OOM
    after_success:
        poll /api/ps → record size_vram
        update vram_total estimate
```

### 2-2. Model VRAM Estimation (No Architecture Info)

**Strategy A — File size based**:
```
estimated_weight_mb = gguf_file_size_mb * 1.15
```
GGUF is mmap format, so file size ≈ weight VRAM. 15% for graph + embedding overhead.

**Strategy B — Similar model reference**:
```
estimated = known_model.vram_model_mb * (target_params / known_params)
```

**Strategy C — Quantization table**:
```
param_count = parse "72B" from /api/show details.parameter_size
bytes_per_param = quantization_table[details.quantization_level]
estimated_weight_mb = param_count * bytes_per_param / 1_048_576

quantization_table = {
    "Q4_K_M": 0.563, "Q4_K_S": 0.563, "Q5_K_M": 0.688, "Q5_K_S": 0.688,
    "Q6_K": 0.820,   "Q8_0": 1.063,   "F16": 2.0,      "F32": 4.0,
    "Q4_0": 0.563,   "Q5_0": 0.688,   "Q5_1": 0.750,
    "IQ4_XS": 0.531, "IQ3_S": 0.430,  "IQ2_XS": 0.289,
}
```

### 2-3. KV Cache Estimation (No Architecture Info)

```
if param_size <= 3B:   kv_per_token_est = 32 KB
elif param_size <= 8B:  kv_per_token_est = 64 KB
elif param_size <= 14B: kv_per_token_est = 96 KB
elif param_size <= 32B: kv_per_token_est = 128 KB
elif param_size <= 72B: kv_per_token_est = 192 KB
else:                   kv_per_token_est = 256 KB
```

Replaced with exact architecture params from `/api/show` after first success.

---

## Phase 3: Failure Learning

### 3-1. OOM Detection

Ollama returns HTTP 500 on OOM:
- `"model requires more system memory (X.XGiB) than is available (Y.YGiB)"`
- `"model runner has unexpectedly stopped"`
- `"exit status 2"` (CUDA OOM crash)

### 3-2. On Failure → Increase Estimate

```
on_inference_failure(provider_id, model_name, error):
    if is_oom_error(error):
        entry.estimated_weight_mb *= 1.20
        entry.estimated_kv_per_slot_mb *= 1.20
        # safety_permil +50 (OOM_SAFETY_BUMP_PERMIL), max 500 (= 50%)
        # AIMD simultaneously applies max_concurrent ×3/4 (independent path)
        provider.safety_permil = min(provider.safety_permil + 50, 500)
```

### 3-3. On Success → Calibrate with Actual

```
on_inference_success(provider_id, model_name):
    ps_response = GET /api/ps
    entry.actual_weight_mb = model.size_vram / 1_048_576
    entry.estimated = false
```

### 3-4. Learning Cycle

```
Unknown → first request → success → /api/ps measurement → Confirmed
                        → failure → estimate ×1.2 → retry
                                  → 3 consecutive OOM → preload_fail_count=3 → 300s preload exclusion
```

---

## Phase 4: VRAM Pool Dispatch

### 4-1. Per-Request VRAM Cost

```
if model loaded on provider: cost = kv_cache_mb(model, context_len)
else: cost = weight_mb(model) + kv_cache_mb(model, context_len)
```

### 4-2. KV Cache Calculation (Throughput-Based)

Computed during `sync_provider()` using architecture info (`/api/show`) and throughput stats.

```
kv_bytes_per_token = 2 * num_layers * num_kv_heads * head_dim * bytes_per_element
effective_ctx = min(configured_ctx, max_ctx) or 4096
tokens = min(max(avg_prompt + avg_output, 128), effective_ctx)
kv_per_request_mb = max(kv_bytes_per_token * tokens / 1_048_576, 32)
```

- `num_layers`: attention-only layers (hybrid models: `block_count / full_attention_interval`)
- `bytes_per_element`: determined by KV cache quantization type (q8_0 = 1)
- Minimum 128 tokens, minimum 32MB

### Hybrid Mamba+Attention Support

```
if attn_interval > 1:
    attn_layers = (block_count + attn_interval - 1) / attn_interval
else:
    attn_layers = block_count

kv_heads = head_count_kv or head_count  // null fallback for hybrid models
```

### 4-3. Dispatch Flow

```
REQUEST(model_name, context_len)
│
├─ 1. Candidate providers (active + provider_type match)
│     Ollama: providers_for_model() → filter to providers that have the model
│
├─ 2. Per-provider VRAM cost calculation
│     if model loaded: cost = kv_only
│     else:            cost = weight + kv
│
├─ 3. Per-provider available VRAM check
│     available = vram_total - vram_used - safety_buffer
│     safety_buffer = DEFAULT_BUFFER_MB = 512 MB (constant floor reservation)
│
├─ 4. Model selection filter (queue path only)
│     list_enabled(provider_id) → skip if model is disabled
│     **Fail fast**: if zero candidates remain after filtering → fail job immediately (no re-enqueue)
│
├─ 5. Sort: VRAM available + model stickiness bonus
│     loaded provider → +100GB bonus → highest priority (model locality)
│     unloaded provider → cost=weight+KV → lower priority
│
├─ 6. Gate chain: thermal(per-provider) → circuit_breaker → concurrency limit
│
├─ 7. VRAM reserve: vram_pool.try_reserve() → VramPermit or None
│     Direct path: None → skip (VRAM unavailable)
│     Queue path: None → re-enqueue with backoff
│
├─ 8. Dispatch → Ollama
│
└─ 9. On completion: drop(VramPermit) → KV cache released
              weight stays loaded (OLLAMA_KEEP_ALIVE=-1)
```

### 4-4. VRAM Pool Data Structure

```rust
struct VramPool {
    providers: Arc<DashMap<Uuid, Arc<ProviderVramState>>>,
    probe_permits: Arc<AtomicI32>,
    probe_rate: Arc<AtomicU32>,
    loaded_models_global: Arc<DashSet<String>>,  // O(1) cross-provider model lookup
}

struct ProviderVramState {
    total_mb: AtomicU64,
    reserved_kv_mb: Arc<AtomicU64>,       // global KV reservation across all models
    safety_permil: AtomicU32,             // e.g. 200 = 20%, increases on OOM (range 100–500)
    models: DashMap<String, ModelState>,
    cached_loaded_weight_mb: AtomicU64,   // O(1) sum of loaded model weights (updated on load/unload)
    is_standby: AtomicBool,               // Scale-In flag (routing excluded)
    transition_until: AtomicU64,          // Scale-In/Out transition guard (Unix ms)
    last_mem_available_mb: AtomicU32,     // APU drift detection (0 = not yet set)
    total_active_count: Arc<AtomicU32>,   // O(1) provider-wide active request count
}

struct ModelState {
    weight_mb: u64,
    is_loaded: bool,
    kv_per_request_mb: u64,            // from throughput stats during sync
    active_kv_mb: Arc<AtomicU64>,      // per-model KV reservation
    active_count: Arc<AtomicU32>,      // per-model active request count
    max_concurrent: AtomicU32,         // adaptive concurrency limit (0 = unlimited, capped at num_parallel)
    baseline_tps: AtomicU32,           // baseline tps × 100 for AIMD
    baseline_p95_ms: AtomicU32,        // baseline p95 latency (ms) for AIMD
    probe_counter: AtomicU32,          // per-model counter for probe scheduling
    // Phase 7 fields
    last_active_at: Arc<AtomicU64>,    // Unix ms, updated on VramPermit::drop
    is_preloading: AtomicBool,         // prevents duplicate preload requests
    is_pulling: AtomicBool,            // model pull in progress
    sample_count: AtomicU32,              // AIMD measurement count (reset on evict)
    preload_fail_count: AtomicU32,        // consecutive failures (reset on success)
    preload_failed_at: AtomicU64,         // Unix ms of 3rd consecutive failure (300s exclusion)
    learning_epoch_started_at: AtomicU64, // ClickHouse query window start
    dispatch_blocked: AtomicBool,         // governor: share=0 → block dispatch
    governor_cap: AtomicU32,              // governor: fair-share cap (0 = no cap)
    pre_hard_max_concurrent: AtomicU32,   // per-model snapshot before Hard (restore on RampUp)
    stable_cycle_count: AtomicU32,        // consecutive stable AIMD cycles (baseline update gate: ≥3)
}

// Note: pre_hard_total (Σ max_concurrent snapshot at Hard entry) is stored in
// ThermalThrottleMap::ThrottleState, not in VramPool. RampUp→Normal exits when
// sum_max_concurrent >= pre_hard_total (AIMD restored to pre-Hard level).
```

**Weight vs KV cache separation**:
- **Model weight**: stays in VRAM after load (`is_loaded` tracking). Never released on completion.
- **KV cache**: reserved per request, released via RAII (`VramPermit`) on completion.

```rust
struct VramPermit {
    kv_mb: u64,
    reserved_kv: Option<Arc<AtomicU64>>,             // provider-global KV counter
    active_count: Option<Arc<AtomicU32>>,            // per-model request count
    release_tx: Option<oneshot::Sender<u64>>,        // distributed release (Valkey)
    last_active_at: Option<Arc<AtomicU64>>,          // updates on drop for idle tracking
    provider_active_count: Option<Arc<AtomicU32>>,   // provider-total active count (O(1) provider_active_requests)
}

impl Drop for VramPermit {
    fn drop(&mut self) {
        // Decrement provider-global KV reservation
        // Decrement per-model active request count
        // Store current Unix ms in last_active_at (idle tracking for eviction)
        // Notify Valkey for distributed release (if present)
    }
}
```

**VramPermit constructors**:
- `with_last_active()` — local single-instance permit with `last_active_at` tracking.
- `combined()` — distributed permit: local atomic decrement + async Valkey release via oneshot channel.
- `VramPermit::new()` was removed; all callers use `with_last_active()` or `combined()`.

**Three counters tracked by VramPermit**:
1. `reserved_kv_mb` (provider-global): total KV reservation, used for available VRAM calculation.
2. `active_count` (per-model): active requests per model, used for dashboard + thermal update.
3. `provider_active_count` (provider-total): O(1) provider-wide active count, decremented on drop alongside `active_count`.

**Provider-total active requests**: `provider_active_requests(provider_id)` → O(1) via `total_active_count` (cached `Arc<AtomicU32>`, incremented/decremented alongside per-model `active_count`). Used by `thermal.update()` for Soft→Normal hysteresis (requires `active_count == 0`) and RampUp→Normal check.

---

## Phase 5: Background Sync

### 5-1. /api/ps Periodic Polling (30s)

```
every 30s per provider:
    ps = GET /api/ps
    observed_used = sum(model.size_vram for model in ps.models)
    loaded_models_cache.update(provider_id, ps.models)
    if provider.total_mb.is_none():
        provider.total_mb = Some(max(observed_used * 1.15, previous_estimate))
```

### 5-2. /api/show Architecture Cache

```
on sync_provider() per model:
    show = POST /api/show {"model": model_name}
    // Parse architecture (hybrid Mamba+Attention support)
    // Compute KV per request from throughput stats
```

---

## Phase 6: Adaptive Learning (Cold Start → AIMD → LLM Batch)

Even with sufficient VRAM, high concurrent requests on CPU-bound servers cause severe throughput degradation. AIMD automatically learns optimal per-model concurrency.

### Learning Phases

| Phase | Trigger | max_concurrent | Method |
|-------|---------|---------------|--------|
| **Cold Start** | Provider first registered / model evict+reload | `num_parallel` | Top-down: start at provider's NUM_PARALLEL, AIMD decreases if needed |
| **AIMD** | sample_count ≥ 3 | AIMD adjusted, capped at `num_parallel` | TPS ratio ≥0.9 → +1 (capped), <0.7 or p95 spike → ×3/4 |
| **LLM Batch** | total samples ≥ 10 across all models | LLM recommended (±2 clamp, upper = num_parallel×2) | All-model combination analysis |

**Cold Start** (`num_parallel` top-down policy):
- New/reloaded models start with `max_concurrent = num_parallel` (from `provider_vram_budget.num_parallel`)
- APU memory safety is handled independently by `try_reserve` + `safety_permil`, so starting high is safe
- AIMD rapidly decreases if throughput degrades, converging to optimal within a few 30s cycles
- Multi-model simultaneous cold start defense: Preloader sets `initial = min(num_parallel, num_parallel - committed_parallel)` where `committed_parallel` = sum of all loaded models' max_concurrent

**AIMD**:
- Activates when `sample_count ≥ 3` (per model×provider)
- TPS maintained + p95 stable → additive increase (+1), **capped at `num_parallel`**
- TPS 30%+ drop **or p95 > 2× baseline** → multiplicative decrease (×3/4), minimum 1
- p95 spike detection: catches tail latency degradation even when average TPS looks normal
- `sample_count` resets to 0 on model evict → Cold Start restarts with fresh `learning_epoch_started_at`
- ClickHouse queries only aggregate data after `learning_epoch_started_at` to prevent stale measurements from contaminating new learning epochs

**sample_count reset triggers**:
- Model evict (idle 180s, demand=0): `sample_count=0`, `learning_epoch_started_at=now_ms`
- Model pull (weight replacement): same reset + `baseline_tps=0`, `baseline_p95_ms=0`
- External memory pressure (30s sync detects `mem_available_mb` drop ≥15%): all models on that provider get `stable_cycle_count=0`, `baseline_tps=0`, `baseline_p95_ms=0` reset (NOT `sample_count` — in-flight measurement continues; `decay_safety_permil()` is also skipped on this drift cycle)

**LLM Batch Analysis**:
- Activates when total samples ≥ 10
- **Gate**: LLM correction is blocked when `stable_cycle_count < 3` — AIMD must remain stable for 3+ consecutive cycles before LLM can propose changes (prevents noise during AIMD descent)
- Sends all loaded model snapshots to LLM
- Analyzes model combination, VRAM usage, throughput patterns
- **Increase-only**: `change_floor = current`, `change_ceil = current + 2`. LLM can nudge up by at most 2; AIMD owns all decreases.
- Upper bound: `num_parallel × 2` (replaced weight-based heuristic)

### Cooldown RampUp

When thermal state transitions Hard → Cooldown → RampUp → Normal:
- **Hard entry**: `ThermalThrottleMap` snapshots `pre_hard_total = Σ max_concurrent` for all models on the provider. Preserved through Cooldown and RampUp.
- **Hard forced drain** (placement_planner): after 60s of Hard, cancels in-flight jobs. 90s watchdog logs error. Calls `thermal.set_cooldown()` once `active_count == 0`.
- **Hard → Cooldown**: `temp < hard_at` AND (`set_cooldown()` called OR 300s elapsed since Hard entry as fallback).
- **Cooldown** (300s min, 900s max = `cooldown_secs × 3`): No dispatch. If temp re-surges above `hard_at`, cooldown timer resets (stays in Cooldown). Transitions to RampUp when `cooldown_elapsed (300s)` AND `temp < soft_at`. At max 900s, forced exit regardless: `temp ≥ soft_at → Soft`, `temp < soft_at → RampUp`.
- **RampUp**: `max_concurrent` forced to **1** for all models. Dispatch resumes (not blocked like Soft/Hard).
- **RampUp → Normal**: exits when `sum_max_concurrent >= pre_hard_total` (AIMD restored to pre-Hard level) OR when Hard was never entered (`pre_hard_total == 0`).
- Prevents thermal oscillation from immediately resuming high concurrency after cooling.

### DB Restore (on server restart)

```
on startup:
  for p in capacity_repo.list_all():
    if p.max_concurrent > 0: vram_pool.set_max_concurrent(...)
    if p.baseline_tps > 0:   vram_pool.set_baseline_tps(...)
    if p.baseline_p95_ms > 0: vram_pool.set_baseline_p95_ms(...)
```

### Algorithm (TCP congestion control)

```
per-model state:
  max_concurrent: u32
  baseline_tps: f64
  baseline_p95_ms: u32

every sync cycle (~30s):
  stats = compute_throughput_stats(provider_id, model, 1h)
  if stats.sample_count < 3: skip

  if baseline_tps == 0:
    baseline_tps = stats.avg_tokens_per_sec
    baseline_p95_ms = stats.p95_latency_ms
    return

  ratio = stats.avg_tokens_per_sec / baseline_tps
  p95_spike = baseline_p95_ms > 0 && stats.p95_latency_ms > baseline_p95_ms * 2

  if ratio < 0.7 || p95_spike:
    max_concurrent = max(1, max_concurrent * 3 / 4)
  elif ratio >= 0.9:
    max_concurrent += 1
    increment_stable_cycle_count()
    if stable_cycle_count >= 3:  // only update baseline after 3 consecutive stable cycles
      baseline_tps = max(baseline_tps, stats.avg_tokens_per_sec)
      baseline_p95_ms = min(baseline_p95_ms, stats.p95_latency_ms)

at dispatch time (try_reserve):
  if probe_permits > 0:  // Probe UP
    hard_cap = max_concurrent + probe_permits
    if active >= hard_cap: block
    elif active >= max_concurrent:
      if hit_count % probe_rate == 0: allow (probe)
      else: block
  elif probe_permits < 0:  // Probe DOWN
    effective = max(1, max_concurrent + probe_permits)
    if active >= max_concurrent: block
    elif active >= effective:
      if hit_count % probe_rate == 0: block
  else:
    if active >= max_concurrent: block
```

### Provider-Wide Pressure Governor

When `provider_total_active > num_parallel` at the start of the AIMD sync loop, the governor activates fair-share budgeting instead of running AIMD increase/decrease (both are suppressed while the governor is active).

**Activation condition**: `provider_total_active > num_parallel`

**Candidates**: loaded models where `active_count > 0 OR demand_counter > 0`. The demand guard prevents deadlock — models with pending demand must always receive share ≥ 1.

**Distribution** (sorted by `oldest_queued_ms` ascending — oldest queued job gets priority):
- If `n ≤ budget`: `base = budget / n`, remainder distributed 1-by-1 to oldest models. All candidates get share ≥ 1.
- If `n > budget`: top `budget` models get `share = 1`, rest get `share = 0`.

**Enforcement** (`governor_cap` field in `ModelState`):
- `share > 0`: `governor_cap = min(max_concurrent, share)`. `max_concurrent` is NOT modified (preserves AIMD learning values).
- `share = 0`: `dispatch_blocked = true`. No dispatch until next cycle.

**Dispatch check** (`should_block()`): checks `dispatch_blocked` first (→ block immediately), then applies `effective_limit = min(max_concurrent, governor_cap)` when `governor_cap > 0`.

**Reset**: At the start of each AIMD sync cycle, all `dispatch_blocked = false` and `governor_cap = 0` are reset before re-evaluation.

**Baseline freeze**: During governor-active cycles, `baseline_tps` is NOT updated — governor-capped TPS is not the model's true throughput.

**Deactivation**: When `provider_total_active ≤ num_parallel`, governor is inactive. At the start of each sync cycle, `dispatch_blocked = false` and `governor_cap = 0` are reset for all loaded models before re-evaluation, regardless of active state. AIMD increase/decrease then resumes normally.

---

## APU VRAM Management

On APU systems (Ryzen AI 395+ — shared CPU/GPU memory), DRM reports ~1GB VRAM which is far below actual model sizes (5–51GB). The VramPool uses `mem_available_mb` from node-exporter instead.

**APU detection** (`is_apu` in analyzer.rs):
```rust
is_apu = gpu_vendor == "amd" && drm_vram_mb > 0 && mem_available_mb > drm_vram_mb * 2
```
All three conditions required: AMD GPU driver detected + DRM VRAM present + system RAM is more than 2× DRM VRAM (indicates shared-memory APU, not discrete GPU).

### VRAM Total Calculation

```
total_mb = mem_available_mb × (1 - safety_permil / 1000)
```

- `mem_available_mb`: system available memory from node-exporter, refreshed every 30s
- `safety_permil`: safety margin in permil (default 100 = 10%), absorbs memory drift from non-Ollama processes

### safety_permil Constants

| Constant | Value | Meaning |
|----------|-------|---------|
| `DEFAULT_SAFETY_PERMIL` | 100 | Initial / minimum margin (10%) |
| `OOM_SAFETY_BUMP_PERMIL` | 50 | +5% per OOM event |
| `SAFETY_DECAY_PERMIL` | 10 | −1% per stable cycle (APU only) |

### safety_permil Rules

| Event | Change | Range |
|-------|--------|-------|
| OOM detected (try_reserve fail or Ollama 429) | `+50` (OOM_SAFETY_BUMP_PERMIL) | up to 500 (50%) |
| 30s sync loop, no OOM (stable) — **APU only** | `-10` (SAFETY_DECAY_PERMIL) | down to 100 (10%) |

**Recovery asymmetry is intentional**: `+50` recovery takes 5 cycles (150s) at `-10/30s`. Combined with AIMD `max_concurrent` recovery at `+1/30s`, this creates a ~150s low-utilization window after OOM. OOM can halt the entire service, so safety over speed is the correct trade-off.

**OOM dual correction**: On OOM, both `safety_permil +50` (shrinks available VRAM ceiling) and `max_concurrent ×3/4` (AIMD multiplicative decrease) apply simultaneously. The two paths are independent — AIMD optimizes throughput, `try_reserve + safety_permil` ensures memory safety.

---

## Thermal Throttle

Per-provider configurable thresholds via `ThermalThresholds`. Soft gate checks **provider-total** active requests (not per-model).

### Auto-Detection

Thermal profile is set automatically by health_checker based on `gpu_vendor` from node-exporter:

| `gpu_vendor` | Profile | Source |
|-------------|---------|--------|
| `"amd"` | CPU (75/82/90°C) | DRM GPU metrics present (`node_drm_*`) → amdgpu driver → AMD/APU |
| `""` (empty) | CPU (default) | No DRM metrics (NVIDIA proprietary driver, or no GPU) |

Detection path: `health_checker` checks DRM metric presence in node-exporter → sets `gpu_vendor="amd"` or `""` → cached in Valkey (`HwMetrics`) → calls `thermal.set_thresholds()` every 30s cycle.

**Note**: NVIDIA GPU profile (80/88/93°C) is defined but currently unreachable — NVIDIA does not expose DRM metrics, so `gpu_vendor` is never set to `"nvidia"`.

### Threshold Profiles

| Profile | Normal below | Soft at | Hard at | Use case |
|---------|-------------|---------|---------|----------|
| `CPU` (default) | 75°C | 82°C | 90°C | Ryzen AI 395+, CPU/iGPU inference |
| `GPU` | 80°C | 88°C | 93°C | NVIDIA discrete GPU |

### State Machine (5-state)

| Temperature / Condition | State | Effect |
|------------------------|-------|--------|
| < normal_below AND active_count == 0 | Normal | Full capacity |
| normal_below–soft_at | Hysteresis | No change (keep previous state) |
| ≥ soft_at | Soft | Block new requests when `active_count > 0`; if `active_count == 0`, allow one in (drain-first policy) |
| Soft → Normal | `temp < normal_below` **AND** `active_count == 0` | Both conditions required — prevents mid-stream state release |
| ≥ hard_at | Hard | Block all requests; snapshot `pre_hard_total = Σ max_concurrent`; 60s → drain, 90s → watchdog |
| `temp < hard_at` + (`active==0` or 300s fallback) | Cooldown | 300s hold (max 900s); no dispatch; timer resets if temp re-surges |
| `cooldown_elapsed (300s)` AND `temp < soft_at` | RampUp | `max_concurrent=1`, dispatch resumes; AIMD ramps back up |
| RampUp: `sum_max_concurrent >= pre_hard_total` | Normal | AIMD restored to pre-Hard level |

### API

```rust
thermal.set_thresholds(provider_id, ThermalThresholds::GPU);
thermal.set_thresholds(provider_id, ThermalThresholds { normal_below: 70.0, soft_at: 80.0, hard_at: 88.0 });
```

### Gate Chain Priority (Dispatch)

```
thermal(per-provider) → circuit_breaker → concurrency_limit(AIMD)
```

- Thermal checked first — Hard/Cooldown blocks regardless of other gates
- Circuit breaker (existing impl in `score_and_claim()`): consecutive failure tracking, independent of thermal
- Concurrency limit (AIMD): `max_concurrent` enforcement via `try_reserve`
- During RampUp: thermal gate passes but AIMD forces `max_concurrent=1`

### score_and_claim() Algorithm (dispatcher.rs)

Called for each job to select and claim a provider slot:

```
score_and_claim(job, candidates):
  1. Filter: available_vram > 0 for Ollama; Gemini always passes
  2. Score per provider:
       Gemini: score = i64::MAX (always preferred when available)
       Ollama: score = available_vram_mb + locality_bonus
                locality_bonus = MODEL_LOCALITY_BONUS_MB (100_000 MB = +100GB)
                                 when model already loaded on that provider
  3. Tier sort: score used to rank; highest score wins
  4. RampUp enforcement: if provider in RampUp state, force max_concurrent = 1
     before calling try_reserve()
  5. age_bonus (anti-starvation, applied to ZSET score before claim):
       age_bonus = wait_ms × 0.25 × perf_factor
       perf_factor = thermal.global_perf_factor() — global minimum across all providers
  6. Standby skip: providers with standby=true are excluded in **filter_candidates() Stage 1** (upstream of score_and_claim)
  7. Adaptive-K peek: ZSET_PEEK_K=20 initial, up to ZSET_PEEK_K_MAX=100 on retry
```

**Constants**: `MODEL_LOCALITY_BONUS_MB=100_000`, `ZSET_PEEK_K=20`, `ZSET_PEEK_K_MAX=100` (defined in `domain/constants.rs`)

**Return**: `Some((provider, VramPermit))` when a provider slot is claimed; `None` when all candidates are blocked (thermal Hard/Cooldown, circuit breaker, or VRAM unavailable). Queue dispatcher re-enqueues on `None`; direct path skips the provider.

---

## Placement Planner: Standby / Scale-In

The placement planner runs every 5s and manages provider lifecycle alongside Scale-Out and preloading.

### Constants

| Constant | Value | Purpose |
|----------|-------|---------|
| `PLANNER_INTERVAL` | 5s | Main loop cadence |
| `SCALE_OUT_THRESHOLD` | 0.80 | VRAM utilization fraction that triggers Scale-Out consideration |
| `EVICT_IDLE_SECS` | 180s | Idle eviction threshold for active providers |
| `STANDBY_EVICT_IDLE_SECS` | 30s | Idle eviction threshold while in standby |
| `TRANSITION_GUARD_SECS` | 30s | Hold-down after any state transition (standby↔active) |
| `SCALE_OUT_HOLDDOWN_SECS` | 60s | Minimum time between consecutive Scale-Out decisions for same model |
| `PRELOAD_LOCK_TTL` | 180s | Distributed lock TTL for concurrent preload safety |
| `SCALEOUT_DECISION_TTL` | 30s | Distributed lock TTL for scale-out decisions |

### Hard Gate Watchdog

Polled every `PLANNER_INTERVAL` (5s) during Hard thermal state:

| Elapsed since Hard entry | Action |
|--------------------------|--------|
| ≥ 60s | `cancel_jobs_for_provider()` — drain in-flight jobs |
| ≥ 90s | `error!` log (watchdog alert only, no state change) |

After cancel, `thermal.set_cooldown()` is called once `active_count == 0`.

### Scale-Out Step① Algorithm

On each cycle, for each model with `scale_out_needed`:
1. Compute `needed_servers = ceil(demand / avg_max_concurrent)` across existing providers
2. For each candidate server: `provisional_free = vram_total - reserved_kv - loaded_weight - DEFAULT_BUFFER_MB`
3. Select the server with **maximum provisional_free** (tie-break: `provider_id` ASC)
4. Deduct `model_weight_mb()` from that server's provisional free (fallback: 2048 MB when weight unknown)
5. If `provisional_free > 0`: add to `scale_out_servers`, trigger preload on that provider

### Standby State (Scale-In)

The placement planner marks a provider as standby when it is idle and not the last server:

- **Trigger (Step ⑤)**: `server_idle` = no loaded models with demand AND `total_active = 0` AND no model preloading. Provider must not be in `scale_out_servers` for this cycle, not in hold-down, and not already standby/transitioning. **Last-server protection**: Step ⑤ only runs when `ollama_providers.len() > 1` — the final provider is never sent to standby.
- **Effect**: `set_standby(provider_id, true)` + `set_transition_until(provider_id, now + 30s)`. Server remains physically running but is excluded from new request routing (dispatcher skips standby providers).
- **`transition_until` guard**: 30-second window after state change (both Scale-In and STANDBY recovery) during which the provider is skipped from further state changes.

### STANDBY Recovery (Step ④)

A standby server is reactivated when:
- **Condition A**: it has a loaded model with `demand > 0`, OR
- **Condition B**: it is the best provisional-free candidate for a `scale_out_needed` model, selected by the same Step① algorithm (max `provisional_free`, tie-break: `provider_id` ASC). Only triggers if no active provider satisfies the scale-out need.

On recovery: `set_standby(false)` + new `transition_until = now + 30s` + added to `scale_out_servers` to prevent immediate re-Scale-In.

### Standby Eviction

While in standby, the eviction threshold for idle models is shortened to **30s** (vs 180s normally).

---

## DB Schema

### `model_vram_profiles`

PK: `(provider_id, model_name)`

| Column | Type | Description |
|--------|------|-------------|
| `provider_id` | UUID | Ollama provider |
| `model_name` | TEXT | Model name |
| `weight_mb` | INT | Measured weight VRAM (MB) |
| `weight_estimated` | BOOL | Whether estimated |
| `kv_per_request_mb` | INT | Throughput-based per-request KV (MB, min 32) |
| `num_layers` | SMALLINT | Attention-only layers (hybrid: block_count / attn_interval) |
| `num_kv_heads` | SMALLINT | KV attention heads |
| `head_dim` | SMALLINT | Head dimension |
| `configured_ctx` | INT | Ollama num_ctx setting |
| `failure_count` | SMALLINT | Consecutive OOM count |
| `llm_concern` | TEXT NULL | LLM analysis concern |
| `llm_reason` | TEXT NULL | LLM analysis reason |
| `max_concurrent` | INT | Adaptive concurrency limit (0 = unlimited) |
| `baseline_tps` | INT | Baseline TPS × 100 |
| `baseline_p95_ms` | INT | Baseline p95 latency (ms) |

### `provider_vram_budget`

PK: `provider_id` — FK → `llm_providers(id)` ON DELETE CASCADE

Persists VRAM management state that must survive restarts. `num_parallel` and `vram_total_mb` live in `llm_providers` (managed via provider API); this table holds the dynamic learned state.

| Column | Type | Description |
|--------|------|-------------|
| `provider_id` | UUID | FK → `llm_providers.id` |
| `safety_permil` | INT | Safety margin ÷1000 (100=10%, max 500). +50 on OOM, -10 per stable cycle |
| `vram_total_source` | TEXT | `probe` / `node_exporter` / `manual` |
| `kv_cache_type` | TEXT | `f16` / `q8_0` / `q4_0` |
| `updated_at` | TIMESTAMPTZ | Last persist timestamp |

**Related fields in `llm_providers`**:
- `total_vram_mb` — confirmed total VRAM (0 = unknown → pass-through)
- `num_parallel` — Ollama NUM_PARALLEL setting (AIMD upper bound)

### `capacity_settings`

| Column | Default | Description |
|--------|---------|-------------|
| `analyzer_model` | `qwen2.5:3b` | LLM for analysis |
| `sync_enabled` | `true` | Auto analysis ON/OFF |
| `sync_interval_secs` | `300` | Analysis interval |
| `probe_permits` | `1` | AIMD probe: +N up, -N down, 0 disabled |
| `probe_rate` | `3` | Every N hits at limit, allow 1 probe |

---

## API Endpoints

```
GET  /v1/dashboard/capacity
     → {providers: [{
           provider_id, provider_name, thermal_state, temp_c,
           vram_total_mb, vram_used_mb, vram_available_mb,
           loaded_models: [{
               model_name, weight_mb, kv_per_request_mb,
               active_requests, max_concurrent,
               llm_concern, llm_reason
           }]
       }]}

GET  /v1/dashboard/capacity/settings
PATCH /v1/dashboard/capacity/settings → update (partial)
POST /v1/dashboard/capacity/sync → 202 | 409
```

---

## Ollama Configuration (Provider Nodes)

```bash
OLLAMA_MAX_LOADED_MODELS=0        # auto (3 × GPU count)
OLLAMA_NUM_PARALLEL=4             # concurrent inference slots per model
OLLAMA_KEEP_ALIVE=10m             # low-power policy — auto-unload on idle (VramPool tracks state)
OLLAMA_GPU_OVERHEAD=5368709120    # 5GB reserved (CUDA/driver)
OLLAMA_FLASH_ATTENTION=1          # Flash Attention (required for KV quant)
OLLAMA_KV_CACHE_TYPE=q8_0         # KV cache quantization (50% VRAM saving)
OLLAMA_LOAD_TIMEOUT=900           # 15 min for large model loading
```

### KV Cache Quantization Effect

| Type | VRAM vs f16 | Quality Impact |
|------|-------------|---------------|
| f16 | 100% | None |
| q8_0 | ~50% | Negligible (+0.002–0.05 ppl) |
| q4_0 | ~25–33% | Moderate (+0.2–0.25 ppl) |

### Ollama Limitations

- `NUM_PARALLEL`, `KV_CACHE_TYPE` are global settings (not per-model)
- Cannot duplicate-load the same model name (requires separate Modelfile)
- No VRAM reservation API — scheduler decides internally
- Memory estimation can overestimate up to 2.2× (issue #10359)

---

## Summary

```
[Server startup]
  ├── model_manager = None (VramPool + OLLAMA_KEEP_ALIVE=-1 manages lifecycle)
  └── Restore max_concurrent / baseline_tps / baseline_p95_ms from DB
      → Learned models: DB limits apply immediately
      → New models: cold start = num_parallel (top-down)

[Request arrival]
  ├── Model filter: providers_for_model() + list_enabled()
  │     + Stage 4: preload exclusion (3-fail 300s cooldown)
  ├── Model stickiness: +100GB sort bonus for providers with model loaded
  ├── Gate chain: thermal → circuit_breaker → concurrency(AIMD)
  │     RampUp: max_concurrent forced to 1
  ├── Adaptive concurrency (AIMD + probe policy)
  │     Cold start: max_concurrent = num_parallel
  │     AIMD increase capped at num_parallel
  │     Learned: restored DB limit
  ├── VRAM gate: try_reserve() → VramPermit or reject
  │     Direct path: None → skip with warning
  │     Queue path: None → re-enqueue
  ├── Thermal gate (per-provider thresholds, auto-detected from gpu_vendor)
  │     Soft: block when active_count>0 (drain first); exit requires temp<normal_below AND active_count==0
  │     Hard: block all → 60s forced drain → Cooldown(300s min) → RampUp(mc=1) → Normal when Σmc≥pre_hard_total
  ├── Success → /api/ps measurement → VRAM profile learning
  ├── Failure (OOM) → estimate ×1.2 + safety_permil +50 + max_concurrent ×3/4
  └── drop(VramPermit) → KV cache released + last_active_at updated

[Background loops]
  ├── Sync (30s): /api/ps weight + /api/show arch + KV calculation
  │     AIMD: TPS ratio + p95 spike → max_concurrent (capped at num_parallel)
  │     LLM Batch: all-model analysis → increase-only (floor=current, ceil=current+2)
  │     DB persist → restored on restart
  ├── Placement Planner (5s): Scale-Out + Preload + Evict(idle 180s) + Scale-In
  │     Evict resets: sample_count=0, learning_epoch_started_at=now
  ├── Promote Overdue (30s): EMERGENCY_BONUS for jobs waiting >250s
  └── Demand Resync (60s): ZSET-based ground truth → demand_counter correction
```
