# llama-server: Automatic Allocation Flow

> SSOT | **Last Updated**: 2026-03-24 | Classification: Operational
> End-to-end automatic llama-server node allocation flow and scheduling logic.

## Automatic llama-server Allocation — End-to-End Flow

Once an llama-server provider is registered, everything works automatically: model sync, VRAM management, concurrency limits, and throughput learning.
Admins just register the provider and link a server — that's it.

### Full Lifecycle

```
1. REGISTER     POST /v1/providers {name, provider_type: "llama-server", url}
                → health check → status: online/offline
                → POST /v1/servers {name, node_exporter_url}
                → PATCH /v1/providers/{id} {server_id, gpu_index}

2. AUTO LEARN   In-process AIMD controller (per-provider admission window)
                → live request outcomes feed AimdController + SLO module
                → no fleet-wide /api/tags scrape; capacity flows from
                  veronex-llm-agent heartbeats and on-demand /health probes

3. REQUEST      POST /v1/chat/completions {model: "qwen3:8b", ...}
                → provider selection → VRAM gate → concurrency gate → dispatch

4. LEARN        Completed job → throughput recorded → next sync uses for AIMD
                → 3+ samples: AIMD adjusts max_concurrent
                → 10+ samples: LLM batch recommends optimal allocation

5. RESTART      Server restart → restore learned data from DB → apply immediately
```

### Phase 1: Provider Registration → Automatic Model Discovery

```
POST /v1/providers {name: "gpu-server", provider_type: "llama-server", url: "https://llama-server.example.com"}
  │
  ├── health check: NodeClient.health(port) (llama-server `/health`)
  │   → 200: status = "online", available for routing
  │   → fail: status = "offline", admin must investigate
  │
  ├── model selection: providers opt in to existing Modelfile rows
  │   POST /v1/admin/models      ← registers a Modelfile (catalog SSOT)
  │   PATCH /v1/providers/{id}/selected-models/{model_name} { is_enabled: true }
  │   → upserts provider_selected_models (no per-provider /api/tags scrape)
  │   → Valkey cache: veronex:models:{provider_id} (TTL 30s)
  │
  └── server link (optional):
      POST /v1/servers {name, node_exporter_url}
      PATCH /v1/providers/{id} {server_id, gpu_index: 0}
      → enables GPU VRAM and temperature collection from node-exporter
```

### Phase 2: Request → Provider Selection → Allocation

```
POST /v1/chat/completions {model: "qwen3:8b", messages: [...]}
  │
  ├── 1. API Key auth → verify account_id, tier (free/paid)
  │
  ├── 2. Enqueue in Valkey ZSET (tier-based score)
  │     paid → veronex:queue:zset  score = now_ms - TIER_BONUS_PAID (300,000ms)
  │     free → veronex:queue:zset  score = now_ms - TIER_BONUS_STANDARD (100,000ms)
  │     test → veronex:queue:zset  score = now_ms (no tier bonus)
  │
  ├── 3. queue_dispatcher_loop pops via Lua priority pop
  │
  ├── 4. Provider selection (pick_best_provider)
  │     a. List active llama-server providers
  │     b. Model filter: only providers whose provider_selected_models row
  │        for this model has is_enabled = true (Modelfile-backed)
  │     c. VRAM sort: highest available VRAM first (most headroom among servers)
  │     d. Tier sort: paid key → non-free-tier first, free key → free-tier first
  │     e. ProcessManager.ensure_running(provider, model) — lazy spawn on first
  │        request; subsequent callers wait on per-provider Mutex
  │
  ├── 5. Gate checks (in order)
  │     a. Circuit Breaker: skip providers with consecutive failures
  │     b. Thermal: ≥85°C Soft (skip if active>0), ≥92°C Hard (fully blocked)
  │     c. Concurrency: block if exceeds max_concurrent (cold start=1)
  │     d. VRAM: vram_pool.try_reserve() → reserve KV cache + (weight if needed)
  │
  ├── 6. Dispatch → llama-server API
  │     LlamaServerAdapter: POST {url}/v1/chat/completions (SSE)
  │     The llama-server process is already running with the model loaded
  │     (one model per process — `ensure_running` guaranteed it pre-dispatch).
  │
  └── 7. Completion → Cleanup
        Drop(VramPermit) → release KV cache, active_count -= 1
        circuit_breaker.on_success/on_failure
        Save result to inference_jobs table
```

### Phase 3: Automatic Learning — Cold Start → AIMD → LLM Batch

```
                     ┌─────────────────────────────────────────────────┐
                     │          Sync Loop (30s tick)                   │
                     │                                                 │
  ┌──────────┐       │  ┌─────────────┐   ┌─────────┐   ┌──────────┐ │
  │ Provider  │──────▶│  │ Cold Start  │──▶│  AIMD   │──▶│ LLM Batch│ │
  │ Register  │       │  │ limit = 1   │   │ ±adjust │   │ optimal  │ │
  │           │       │  │ (all models)│   │(per-model)│  │(all combos)│ │
  └──────────┘       │  └──────┬──────┘   └────┬────┘   └─────┬────┘ │
                     │         │               │              │       │
                     │    sample=0         sample≥3       sample≥10   │
                     │    baseline=0       ratio based    LLM analysis │
                     │                                                 │
                     │  ┌──────────────────────────────────────────┐   │
                     │  │ DB persist: model_vram_profiles          │   │
                     │  │  max_concurrent, baseline_tps            │   │
                     │  │  → auto-restored on server restart       │   │
                     │  └──────────────────────────────────────────┘   │
                     └─────────────────────────────────────────────────┘
```

| Phase | Condition | max_concurrent | Behavior |
|-------|-----------|---------------|----------|
| **Cold Start** | New model, no data | 1 | 1 request per model. Collect baseline |
| **AIMD** | sample ≥ 3, baseline exists | Auto-adjusted | ratio ≥ 0.9 → +1, < 0.7 → ×3/4 |
| **LLM Batch** | total sample ≥ 10 | LLM recommended | All model combinations + VRAM + throughput analysis |

### Phase 4: Multi-Server / Multi-Model Automatic Routing

Registering multiple llama-server nodes enables automatic routing to the optimal server.

```
Example: 3 servers, various models

Server A (128GB GPU)                    Server B (24GB GPU)          Server C (CPU only)
├── qwen3:72b (40GB)    limit=2        ├── qwen3:8b (5GB)  limit=4  ├── qwen3:1.7b  limit=3
├── deepseek-r1:70b (45GB) limit=1     └── phi4:14b (9GB)  limit=3  └── phi4-mini   limit=5
└── available: 35GB                        available: 8GB

Request: model=qwen3:8b
  → Server B selected (has model + VRAM headroom)
  → limit=4, active=2 → allowed

Request: model=deepseek-r1:70b
  → Server A selected (only server with model)
  → limit=1, active=1 → queued (cold start or AIMD limit)

Request: model=qwen3:1.7b
  → Server C selected (has model)
  → VRAM=0 (CPU) → delegated to llama-server, only concurrency gate applied
```

**Routing priority**:
1. Only providers that have the requested model are candidates
2. Only providers with model enabled in model selection
3. Prefer providers with more available VRAM
4. On equal VRAM, paid tier key → non-free-tier provider first
5. Must pass Thermal/Circuit Breaker gates

### Phase 5: Adding a New Model

A new model is registered through the Modelfile registry, not by scraping
the node:

```
admin: POST /v1/admin/models { name, source: { hf | s3 | … }, ... }
  │
  ├── install_orchestrator resolves source → CAS blob (sha256-keyed)
  │   → emits SSE progress on /v1/admin/models/{id}/install/stream
  │   → on success: modelfiles row promoted to ready
  │
  ├── per-provider opt-in:
  │   PATCH /v1/providers/{id}/selected-models/{model} { is_enabled: true }
  │   → provider_selected_models row inserted/updated
  │
  ├── First request for that model:
  │   → ProcessManager.ensure_running spawns llama-server with the
  │     CAS blob path; AIMD window initialised at cold-start size (=1)
  │   → permit acquired, dispatch proceeds
  │
  ├── Capacity capture
  │   → veronex-llm-agent reports VRAM/architecture in its probe payload
  │     (no /api/ps or /api/show — those endpoints don't exist on llama-server)
  │   → weight + KV size derived from the Modelfile registry row
  │   → baseline_tps set after the first completed request
  │
  └── Subsequent automatic learning
      → AIMD: auto-adjusts from sample ≥ 3 (admission window)
      → LLM Batch: full model combination analysis from total sample ≥ 10
```

**Cases requiring manual intervention**:
- Disable a specific model on a specific provider: `PATCH /v1/providers/{id}/selected-models/{model} {is_enabled: false}`
- Change probe policy: `PATCH /v1/dashboard/capacity/settings {probe_permits, probe_rate}`
- Trigger immediate sync: `POST /v1/providers/sync`

### Configuration Reference

| Setting | Default | Location | Description |
|---------|---------|----------|-------------|
| sync_interval_secs | 300 | capacity_settings | Auto sync interval |
| sync_enabled | true | capacity_settings | Auto sync ON/OFF |
| analyzer_model | qwen2.5:3b | capacity_settings | Model for LLM analysis |
| probe_permits | 1 | capacity_settings | +N (probe up), -N (probe down), 0=disabled |
| probe_rate | 3 | capacity_settings | 1 probe per N limit hits |
| CAPACITY_ANALYZER_LLAMA_SERVER_URL | (provider URL) | env | LLM analysis target (can be separate) |
