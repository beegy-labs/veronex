# LLM Scheduling: Queue & Implementation — 2026

> SSOT | **Last Updated**: 2026-03-24 | Classification: Reference
> Queue demand sampling, production summary, implementation recommendations, and references.

## 5. Queue Demand Sampling — Without Dequeuing

### The Problem

The priority queue uses Valkey LPUSH/RPUSH + BLPOP for FIFO per priority level.
We need to know **how many requests per model** are waiting without consuming them.

### Solutions

### Option A: Sorted Set Shadow Queue (Recommended)

Maintain a **parallel sorted set** alongside the main list queue:
- On enqueue: `ZADD queue:demand {timestamp} {job_id}` + store job metadata in hash
- On dequeue: `ZREM queue:demand {job_id}`
- To sample: `ZRANGEBYSCORE queue:demand -inf +inf` → look up model from job metadata hash

O(log N + M) scan, non-destructive. Each job_id maps to `{model_id, priority}` in a hash.

**Aggregate demand per model**:
```
demand = {}
for job_id in ZRANGE queue:demand 0 -1:
    model = HGET job:{job_id} model_id
    demand[model] = demand.get(model, 0) + 1
```

Use a Lua script to make this atomic:
```lua
local jobs = redis.call('ZRANGE', KEYS[1], 0, -1)
local counts = {}
for _, jid in ipairs(jobs) do
    local m = redis.call('HGET', 'job:' .. jid, 'model_id')
    counts[m] = (counts[m] or 0) + 1
end
return cjson.encode(counts)
```

### Option B: Per-Model Counters (Simpler)

Maintain `INCR demand:{model_id}` on enqueue, `DECR demand:{model_id}` on dequeue.
Atomic counter gives instant O(1) demand read per model.

**Trade-off**: No information about individual job priorities or ages, but sufficient for
placement decisions.

### Option C: LRANGE Peek (Existing List Queue)

For a simple list-based queue: `LRANGE queue:priority_high 0 99` to inspect the first
100 jobs without consuming them. O(N) but works on existing data structure.

### Recommendation for Veronex

Use **Option B (per-model counters)** as primary signal for fast placement decisions,
combined with **Option A (sorted set)** for priority-aware scheduling when needed.

Update the scheduler every N seconds (configurable, suggested: 5s) by reading counters.

---

## 6. 2026 State of the Art — Production Summary

### The Converged Architecture (2025-2026)

Every major production LLM serving system has converged on this stack:

```
[Priority Queue (Valkey)]
    ↓
[Scheduler / Router Layer]
  - Demand sampling (per-model counters)
  - Cache-affinity routing (consistent hash or dual-hash)
  - Load-aware fallback (power-of-two)
  - Proactive preload trigger
    ↓
[Per-Server Inference Pool]
  - Each server: Ollama instance with VRAM-aware model set
  - Model eviction: LRU + demand-weighted
  - Health monitoring: latency P95, queue depth
```

### Key Papers and Systems (Chronological)

| Year | System | Venue | Key Contribution |
|------|--------|-------|-----------------|
| 2023 | Orca | OSDI | Iteration-level scheduling, continuous batching |
| 2023 | AlpaServe | OSDI | Statistical multiplexing with model parallelism |
| 2024 | DistServe / Splitwise | OSDI / ASPLOS | P/D disaggregation |
| 2024 | Mooncake | FAST (Best Paper) | KVCache-centric disaggregated architecture |
| 2024 | Llumnix | OSDI | Live KV cache migration across instances |
| 2025 | SGLang v0.4 | — | Cache-aware LB: 75% hit rate, 2x throughput |
| 2025 | Helix | ASPLOS | Max-flow placement on heterogeneous GPUs |
| 2025 | Aegaeon | SOSP | Token-level GPU pooling, 82% GPU savings |
| 2025 | NVIDIA Dynamo | GTC 2025 | Inference-engine-agnostic disaggregated serving |
| 2025 | vLLM Router | Dec 2025 | Production P/D-aware LB, consistent hashing |
| 2026 | DualMap | arXiv Feb 2026 | Dual-hash-ring: cache affinity + load balance |
| 2026 | GORGO | arXiv Feb 2026 | Cross-region KV cache routing |

### What Matters for Ollama-Based Systems

Since Ollama is a black-box inference engine (no vLLM internals, no KV cache API):

| Capability | Approach | Complexity |
|-----------|----------|-----------|
| Demand sampling | Per-model Valkey counters | Low |
| Cache-affinity routing | Consistent hash on (model, session_id) | Low |
| Load balancing | Power-of-two choices on `active_requests` | Low |
| Model placement | Demand-weighted VRAM bin packing | Medium |
| Proactive preloading | Preload top-K demand models when idle | Medium |
| Eviction | LRU + demand-weighted score | Medium |
| P/D disaggregation | **Not applicable** (Ollama black-box) | — |
| KV cache migration | **Not applicable** (no API) | — |

---

## 7. Concrete Implementation Recommendations for Veronex

### Phase 1 — Demand Visibility (Low Effort, High Value)

```rust
// On enqueue: INCR demand:{model_id}
// On dequeue: DECR demand:{model_id}
// Scheduler poll: MGET demand:{model_1} demand:{model_2} ...
```

### Phase 2 — Cache-Affinity Routing (Medium Effort)

Implement a consistent hash ring over servers, keyed on `(model_id + session_id)`:
- Ring slots weighted proportional to `server_vram_total`
- Fallback to least-loaded when preferred server is at capacity

### Phase 3 — Proactive Preloading (Medium Effort)

Every scheduler tick (5s):
1. Read `demand[m]` for all models
2. Identify top-K models by demand not currently loaded anywhere
3. Find servers with sufficient free VRAM
4. Send preload request: `POST /api/generate {"model": m, "prompt": "", "keep_alive": -1}`

### Phase 4 — Demand-Weighted Eviction (Medium Effort)

When a server needs VRAM for a new model:
```
evict_score(m) = demand[m] * time_since_last_used(m)
evict = argmin(evict_score)  // evict lowest-score model
```

### Scheduler Loop Architecture

```
loop every 5s:
  demand = read_model_demand_counters()
  server_states = poll_server_health()  // VRAM free, active requests

  placement = bin_pack(demand, server_states)

  for (server, model) in placement.to_load:
    trigger_preload(server, model)

  for (server, model) in placement.to_evict:
    trigger_evict(server, model)   // set keep_alive=0, send empty request
```

---

## References

- [Mooncake paper (arXiv 2407.00079)](https://arxiv.org/html/2407.00079v1) — KVCache-centric disaggregated architecture
- [Disaggregated Inference: 18 Months Later — Hao AI Lab](https://haoailab.com/blogs/distserve-retro/) — landscape overview
- [Aegaeon — SOSP'25](https://ennanzhai.github.io/pub/sosp25-aegaeon.pdf) — GPU pooling, 82% savings
- [DualMap — arXiv 2602.06502](https://arxiv.org/html/2602.06502v1) — dual-hash-ring cache+load balance
- [SGLang v0.4 Cache-Aware LB](https://github.com/sgl-project/sglang) — 75% cache hit rate
- [vLLM Router Dec 2025](https://blog.vllm.ai/2025/12/13/vllm-router-release.html) — production P/D-aware LB
- [Llumnix — OSDI'24](https://www.usenix.org/system/files/osdi24-sun-biao.pdf) — live KV cache migration
- [NVIDIA Dynamo](https://developer.nvidia.com/blog/introducing-nvidia-dynamo-a-low-latency-distributed-inference-framework-for-scaling-reasoning-ai-models/) — inference-engine-agnostic framework
- [llm-d KV cache routing — Red Hat](https://developers.redhat.com/articles/2025/10/07/master-kv-cache-aware-routing-llm-d-efficient-ai-inference)
- [LLM Inference Scheduling Survey — Oct 2025](https://www.techrxiv.org/users/994660/articles/1355915/master/file/data/LLM_Scheduling_Survey_Arxiv_06Oct2025/LLM_Scheduling_Survey_Arxiv_06Oct2025.pdf?inline=true)
- [Priority Queues with Redis Sorted Sets](https://oneuptime.com/blog/post/2026-01-21-redis-priority-queues-sorted-sets/view)
- [Valkey Sorted Set ZRANGEBYSCORE](https://valkey.io/commands/zrangebyscore/)
- [Helix — ASPLOS'25](https://www.cs.cmu.edu/~rvinayak/papers/Helix_ASPLOS_2025_Serving_LLMs_over_Heterogeneous_GPUs_and_Network_via_Max_Flow.pdf) — max-flow on heterogeneous GPUs
- Source: `docs/llm/research/backend/llm-scheduling-2026.md`
