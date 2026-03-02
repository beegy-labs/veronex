# Dynamic Concurrency Control + Thermal Throttle

> **Status**: Implemented | **Branch**: `feat/api-key-usage` | **Migration**: single init migration: 0000000001_init.sql

## Overview

Replaces the old `busy_providers: HashSet<Uuid>` (1 job/provider hard limit) with a
VRAM-aware, thermally-safe dynamic concurrency system.

| Component | Implementation | Location |
|-----------|---------------|----------|
| `ConcurrencySlotMap` | `(provider_id, model_name)` → `Arc<Semaphore>` | `infrastructure/outbound/capacity/slot_map.rs` |
| `ThermalThrottleMap` | `provider_id` → `ThrottleLevel` + cooldown | `infrastructure/outbound/capacity/thermal.rs` |
| `CapacityAnalyzer` | 5-min background loop | `infrastructure/outbound/capacity/analyzer.rs` |

## Two Completely Separate Paths

```
[Request dispatch — ~0.1ms, NO LLM]
  BLPOP → thermal.get() → slot_map.try_acquire() → spawn run_job(permit)
    ↑ DashMap read       ↑ Semaphore try_acquire (non-blocking)

[Background analysis — every N minutes]
  Ollama /api/ps + /api/show → PostgreSQL throughput stats
  → qwen2.5:3b recommends slots → slot_map.update_capacity() + DB upsert
```

## Thermal Throttle States

| Threshold | State | Effect |
|-----------|-------|--------|
| < 78°C (+ no cooldown) | Normal | Full slot capacity |
| 78–85°C (hysteresis zone) | Previous state | No change |
| ≥ 85°C | Soft | Cap to 0 new slots if any active |
| ≥ 92°C | Hard | Dispatch fully suspended |
| Hard → < 78°C | Cooldown (60s) | Normal not yet re-activated |

Thermal state is updated in `health_checker` every 30 s from `hw_metrics` (Valkey).

## KV Cache Formula (exact, model-architecture-based)

```
kv_bytes_per_token = 2 × num_layers × num_kv_heads × head_dim × bytes_per_element
                     K+V              GQA-aware      usually 128  (1=q8_0, 2=BF16/FP16)

effective_ctx = min(configured_ctx, max_ctx)   ← clamp: prevents over-estimating small models
                                                  configured_ctx from /api/show parameters field
                                                  max_ctx from model_info context_length

worst_case_mb = kv_bytes_per_token × effective_ctx / 1_048_576
realistic_mb  = kv_bytes_per_token × avg_tokens    / 1_048_576
```

Architecture parameters come from Ollama `/api/show` `model_info`:
- `*.block_count` → `num_layers`
- `*.attention.head_count_kv` → `num_kv_heads` (GQA-aware; llama3/qwen2.5 have num_kv_heads << num_heads)
- `*.attention.key_length` → `head_dim` (default: 128 if absent)
- `parameters` field (`num_ctx <N>`) → `configured_ctx`
- `model_info.*.context_length` → `max_ctx` (model native maximum)

**`bytes_per_element` is always `1`** in current deployments (all providers use `OLLAMA_KV_CACHE_TYPE=q8_0`).

## Slot Recommendation Logic

```
available_mb = vram_total - vram_model_loaded - 512_MB_buffer
math_slots   = clamp(1 + min(by_realistic, by_worst * 2), 1, OLLAMA_NUM_PARALLEL)
final_slots  = llm_recommend ?? math_slots  (fallback if LLM fails)
```

Slot ceiling is `OLLAMA_NUM_PARALLEL` (default `1`), not a hardcoded `8`.
Recommending more slots than Ollama's parallel capacity is pointless — Ollama serialises excess requests.

The LLM (`qwen2.5:3b` by default) receives the full context in a JSON prompt
and responds with `{recommended_slots, concern, reason}`.

## DB Schema

### `model_capacity` (PRIMARY KEY: provider_id, model_name)

| Column | Type | Description |
|--------|------|-------------|
| `vram_model_mb` | INT | Loaded model VRAM from /api/ps |
| `vram_kv_per_slot_mb` | INT | Realistic KV per slot (avg_tokens) |
| `vram_kv_worst_case_mb` | INT | Worst-case KV (num_ctx) |
| `recommended_slots` | SMALLINT | Current concurrency setting |
| `avg_tokens_per_sec` | FLOAT8 | Generation speed (last 1h) |
| `p95_latency_ms` | FLOAT8 | P95 end-to-end latency |
| `llm_concern` / `llm_reason` | TEXT | LLM analysis narrative |

### `capacity_settings` (singleton id=1)

| Column | Default | Description |
|--------|---------|-------------|
| `analyzer_model` | `qwen2.5:3b` | Ollama model for analysis |
| `batch_enabled` | `true` | Enable/disable auto-analysis |
| `batch_interval_secs` | `300` | How often to run (min 60) |
| `last_run_at` | null | Last successful run timestamp |

### `inference_jobs.provider_id`

Added `provider_id UUID` column to track which Ollama provider processed each job.
Required for per-provider throughput aggregation in `compute_throughput_stats()`.

## API Endpoints

All under JWT auth (`/v1/dashboard/...`):

```
GET  /v1/capacity/providers
     → {providers: [{provider_id, provider_name, thermal_state, temp_c,
                    models: [{model_name, recommended_slots, active_slots,
                             available_slots, vram_model_mb, vram_kv_per_slot_mb,
                             avg_tokens_per_sec, p95_latency_ms, llm_concern, ...}]}]}

GET  /v1/dashboard/capacity/settings
     → {analyzer_model, batch_enabled, batch_interval_secs, last_run_at,
         last_run_status, available_models}

PATCH /v1/dashboard/capacity/settings
      body: {analyzer_model?, batch_enabled?, batch_interval_secs?}
      → updated settings

POST /v1/dashboard/capacity/sync
     → 202 {message: "capacity analysis triggered"}
     → 409 {message: "capacity analysis already in progress"}  ← if analysis_lock held
```

## Environment Variables

```bash
CAPACITY_ANALYZER_OLLAMA_URL=http://localhost:11434  # default: same as OLLAMA_URL
# analyzer_model is configured via DB (PATCH /v1/dashboard/capacity/settings)

OLLAMA_NUM_PARALLEL=1   # mirrors the Ollama StatefulSet env var
                        # used as the upper bound for math_slots clamp
                        # default: 1 (AMD APU / single-GPU deployments)
```

### AMD APU Notes (Ryzen AI Max+ 395 / gfx1151)

`OLLAMA_NUM_PARALLEL` **must remain `1`** on Strix Halo APU deployments:
- GTT/VRAM misreporting bug: with `NUM_PARALLEL > 1` Ollama may refuse GPU inference and fall back to CPU
- Bandwidth already saturated at `batch=1` (~215 GB/s LPDDR5X)
- Multi-model parallelism achieved via `OLLAMA_MAX_LOADED_MODELS=2` instead:
  each model processes its own queue sequentially but independently

## Web UI

The Capacity Control panel is part of the **Providers page** (`/providers?s=ollama`), rendered as `OllamaCapacitySection` after `OllamaSyncSection`.

See `docs/llm/frontend/web-providers.md` → **OllamaCapacitySection** for full UI spec.

Summary:
- **Settings card**: analyzer model selector (lists Ollama's available models), auto-analysis toggle, interval field, Save + Sync Now buttons, last-run timestamp/status
- **Capacity table**: per-provider → per-loaded-model: thermal badge (Normal/Soft/Hard), recommended slots, active/max slots, VRAM (model loaded), KV/slot, avg TPS, P95, LLM concern row
- **Sync Now** fires `POST /v1/dashboard/capacity/sync` (202) → background analysis → refreshes after 3 s

## AppState Fields Added

```rust
pub slot_map:                Arc<ConcurrencySlotMap>,
pub thermal:                 Arc<ThermalThrottleMap>,
pub capacity_repo:           Arc<dyn ModelCapacityRepository>,
pub capacity_settings_repo:  Arc<dyn CapacitySettingsRepository>,
pub capacity_manual_trigger: Arc<tokio::sync::Notify>,
pub capacity_analysis_lock:  Arc<tokio::sync::Semaphore>,  // 1-permit; held during each run
pub analyzer_url:            String,
```

`capacity_analysis_lock` is a 1-permit `Semaphore` shared between the analyzer loop and the
`POST /sync` HTTP handler. The loop acquires it with `acquire_owned().await` before running and
drops it on completion. The handler calls `available_permits() == 0` to detect a running analysis
and immediately returns 409 without queuing a second trigger.
