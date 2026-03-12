# VRAM Pool Capacity Management

> **Status**: Implemented | **Last Updated**: 2026-03-12 | Adaptive Learning + Thermal 5-State + AIMD num_parallel Cap + Placement Planner

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

**Model lifecycle**: VramPool + `OLLAMA_KEEP_ALIVE=-1` manages model retention. `OllamaModelManager` is disabled — its `ensure_loaded(max_loaded=1)` sends `keep_alive=0` which physically unloads other models, destroying multi-model co-residence.

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

| State | VRAM Capacity | Behavior |
|-------|--------------|----------|
| **Unknown** | `None` | Execute first request → learn from success/failure |
| **Estimated** | Max observed `size_vram` | Allocate within estimate |
| **Confirmed** | node-exporter or repeated observation | Allocate within confirmed value |

**VRAM total estimation**:

```
estimated_total = max_observed_sum_size_vram * 1.15
```

node-exporter provides exact value via DRM metrics when available.

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
        provider.safety_factor = min(provider.safety_factor + 0.05, 0.30)
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
                                  → 3 consecutive OOM → exclude model from provider
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

#### Hybrid Mamba+Attention Support

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
    providers: DashMap<Uuid, Arc<ProviderVramState>>,
    probe_permits: Arc<AtomicI32>,
    probe_rate: Arc<AtomicU32>,
}

struct ProviderVramState {
    total_mb: AtomicU32,
    reserved_kv_mb: Arc<AtomicU32>,   // global KV reservation across all models
    safety_permil: AtomicU32,         // e.g. 200 = 20%, increases on OOM (range 100–500)
    models: DashMap<String, ModelState>,
    is_standby: AtomicBool,           // Scale-In flag (routing excluded)
    transition_until: AtomicU64,      // Scale-In/Out transition guard (Unix ms)
}

struct ModelState {
    weight_mb: u32,
    is_loaded: bool,
    kv_per_request_mb: u32,            // from throughput stats during sync
    active_kv_mb: Arc<AtomicU32>,      // per-model KV reservation
    active_count: Arc<AtomicU32>,      // per-model active request count
    max_concurrent: AtomicU32,         // adaptive concurrency limit (0 = unlimited, capped at num_parallel)
    baseline_tps: AtomicU32,           // baseline tps × 100 for AIMD
    baseline_p95_ms: AtomicU32,        // baseline p95 latency (ms) for AIMD
    probe_counter: AtomicU32,          // per-model counter for probe scheduling
    // Phase 7 fields
    last_active_at: Arc<AtomicU64>,    // Unix ms, updated on VramPermit::drop
    is_preloading: AtomicBool,         // prevents duplicate preload requests
    is_pulling: AtomicBool,            // model pull in progress
    sample_count: AtomicU32,           // AIMD measurement count (reset on evict)
    preload_fail_count: AtomicU32,     // consecutive failures (reset on success)
    preload_failed_at: AtomicU64,      // Unix ms of 3rd consecutive failure (300s exclusion)
    learning_epoch_started_at: AtomicU64, // ClickHouse query window start
    dispatch_blocked: AtomicBool,      // manual dispatch block
    pre_hard_max_concurrent: AtomicU32,  // snapshot before Hard thermal (for RampUp restore)
}
```

**Weight vs KV cache separation**:
- **Model weight**: stays in VRAM after load (`is_loaded` tracking). Never released on completion.
- **KV cache**: reserved per request, released via RAII (`VramPermit`) on completion.

```rust
struct VramPermit {
    kv_mb: u32,
    reserved_kv: Option<Arc<AtomicU32>>,       // provider-global KV counter
    active_count: Option<Arc<AtomicU32>>,      // per-model request count
    last_active_at: Option<Arc<AtomicU64>>,    // updates on drop for idle tracking
    release_tx: Option<oneshot::Sender<u32>>,  // distributed release (Valkey)
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

**Two counters tracked by VramPermit**:
1. `reserved_kv_mb` (provider-global): total KV reservation, used for available VRAM calculation.
2. `active_count` (per-model): active requests per model, used for dashboard + thermal throttle.

**Provider-total active requests**: `provider_active_requests(provider_id)` sums all models' `active_count`. Used by Soft thermal gate (per-provider, not per-model).

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
- External memory pressure (30s sync detects `mem_available_mb` drop ≥15%): all models on that provider reset

**LLM Batch Analysis**:
- Activates when total samples ≥ 10
- Sends all loaded model snapshots to LLM
- Analyzes model combination, VRAM usage, throughput patterns
- **±2 change clamp** per cycle to prevent oscillation from LLM hallucination
- Upper bound: `num_parallel × 2` (replaced weight-based heuristic)

### Cooldown RampUp

When thermal state transitions from Hard → Normal (below `normal_below` threshold):
- 60s Cooldown hold, then RampUp phase begins
- During RampUp: `max_concurrent` is forced to **1** for all models on the provider
- `pre_hard_max_concurrent` snapshot preserves the pre-Hard limit
- RampUp ends when AIMD naturally reaches `pre_hard_max_concurrent` → full capacity restored
- Prevents thermal oscillation from immediately resuming high concurrency after cooling

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

---

## APU VRAM Management

On APU systems (Ryzen AI 395+ — shared CPU/GPU memory), DRM reports ~1GB VRAM which is far below actual model sizes (5–51GB). The VramPool uses `mem_available_mb` from node-exporter instead.

### VRAM Total Calculation

```
total_mb = mem_available_mb × (1 - safety_permil / 1000)
```

- `mem_available_mb`: system available memory from node-exporter, refreshed every 30s
- `safety_permil`: safety margin in permil (default 100 = 10%), absorbs memory drift from non-Ollama processes

### safety_permil Rules

| Event | Change | Range |
|-------|--------|-------|
| OOM detected (try_reserve fail or Ollama 429) | `+50` | up to 500 (50%) |
| 30s sync loop, no OOM (stable) | `-10` | down to 100 (10%) |

**Recovery asymmetry is intentional**: `+50` recovery takes 5 cycles (150s) at `-10/30s`. Combined with AIMD `max_concurrent` recovery at `+1/30s`, this creates a ~150s low-utilization window after OOM. OOM can halt the entire service, so safety over speed is the correct trade-off.

**OOM dual correction**: On OOM, both `safety_permil +50` (shrinks available VRAM ceiling) and `max_concurrent ×3/4` (AIMD multiplicative decrease) apply simultaneously. The two paths are independent — AIMD optimizes throughput, `try_reserve + safety_permil` ensures memory safety.

---

## Thermal Throttle

Per-provider configurable thresholds via `ThermalThresholds`. Soft gate checks **provider-total** active requests (not per-model).

### Auto-Detection

Thermal profile is set automatically by health_checker based on `gpu_vendor` from node-exporter:

| `gpu_vendor` | Profile | Source |
|-------------|---------|--------|
| `"nvidia"` | GPU (80/88/93°C) | sysfs vendor `0x10de` |
| `"amd"` | CPU (75/82/90°C) | sysfs vendor `0x1002` (Ryzen AI = APU/iGPU) |
| empty/unknown | CPU (default) | no agent or no GPU detected |

Detection path: `node-exporter` exposes sysfs vendor info → `health_checker` determines `gpu_vendor` from metrics → cached in Valkey (`HwMetrics`) → `health_checker` calls `thermal.set_thresholds()` every 30s cycle.

### Threshold Profiles

| Profile | Normal below | Soft at | Hard at | Use case |
|---------|-------------|---------|---------|----------|
| `CPU` (default) | 75°C | 82°C | 90°C | Ryzen AI 395+, CPU/iGPU inference |
| `GPU` | 80°C | 88°C | 93°C | NVIDIA discrete GPU |

### State Machine (5-state)

| Temperature | State | Effect |
|-------------|-------|--------|
| < normal_below | Normal | Full capacity |
| normal_below–soft_at | Hysteresis | No change (keep previous) |
| ≥ soft_at | Soft | Block if provider has ANY active request |
| ≥ hard_at | Hard | Block all requests |
| Hard → < normal_below | Cooldown (60s) | Hold before resuming |
| Cooldown expired | RampUp | `max_concurrent=1`, AIMD ramps back up gradually |

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

PK: `provider_id`

| Column | Type | Description |
|--------|------|-------------|
| `provider_id` | UUID | Ollama provider |
| `vram_total_mb` | INT NULL | Confirmed total VRAM (NULL = unknown) |
| `vram_total_source` | TEXT | `probe` / `node_exporter` / `manual` |
| `safety_factor` | FLOAT4 | 0.10 – 0.30 (increases on OOM) |
| `kv_cache_type` | TEXT | `f16` / `q8_0` / `q4_0` |
| `num_parallel` | SMALLINT | Ollama NUM_PARALLEL setting |

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
OLLAMA_KEEP_ALIVE=-1              # disable auto-unload (VramPool manages lifecycle)
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
  │     Soft: block if provider has any active request
  │     Hard: block all → Cooldown(60s) → RampUp(max_concurrent=1)
  ├── Success → /api/ps measurement → VRAM profile learning
  ├── Failure (OOM) → estimate ×1.2 + safety_permil +50 + max_concurrent ×3/4
  └── drop(VramPermit) → KV cache released + last_active_at updated

[Background loops]
  ├── Sync (30s): /api/ps weight + /api/show arch + KV calculation
  │     AIMD: TPS ratio + p95 spike → max_concurrent (capped at num_parallel)
  │     LLM Batch: all-model analysis → ±2 clamp
  │     DB persist → restored on restart
  ├── Placement Planner (5s): Scale-Out + Preload + Evict(idle 180s) + Scale-In
  │     Evict resets: sample_count=0, learning_epoch_started_at=now
  ├── Promote Overdue (30s): EMERGENCY_BONUS for jobs waiting >250s
  └── Demand Resync (60s): ZSET-based ground truth → demand_counter correction
```
