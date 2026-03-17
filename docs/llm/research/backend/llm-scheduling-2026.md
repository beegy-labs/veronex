# LLM Multi-Server Scheduling — 2026 Best Practices

> **CDD Layer 2** | Editable | **Last Updated**: 2026-03-11
>
> Web-searched findings on queue-aware model assignment for N-server, M-model Ollama clusters.
> Status: `research` — not yet implemented in Veronex.

---

## Problem Statement

- **N** Ollama servers with heterogeneous VRAM (e.g., 16 GB, 32 GB, 64 GB)
- **M** models with different weight sizes
- Requests arrive in a Valkey priority queue (LPUSH/RPUSH)
- Goals: scan queue demand, assign models to servers, preload proactively, maximize goodput

---

## 1. Disaggregated Prefill/Decode — Applicability to Ollama

### What It Is

Prefill (prompt processing) and decode (token generation) have vastly different compute profiles:
- **Prefill**: compute-bound, short duration, high parallelism
- **Decode**: memory-bandwidth-bound, long duration, sequential

Systems like **Mooncake** (Moonshot AI, FAST'25 Best Paper), **Splitwise**, **DistServe**, and
**NVIDIA Dynamo** disaggregate these phases onto separate GPU pools.

### Does It Apply to Ollama?

**No, not at the engine level.** Ollama does not expose prefill/decode separation in its API.
The entire request lifecycle is a black box from the scheduler's perspective.

**However**, the *routing insight* is applicable at the outer layer:
- **Long-context requests** (large prompts → prefill-heavy) should be routed to servers
  with more VRAM and fewer concurrent decode jobs.
- **Short-prompt, long-generation** requests (decode-heavy) benefit from servers with
  fast memory bandwidth (e.g., AMD APU iGPU with HBM-like VRAM bandwidth).

### Practical Recommendation

Do not implement disaggregation. Instead, classify requests by `prompt_token_count`:
- `prompt_tokens > threshold` → route to high-VRAM server (reduces swapping)
- Otherwise → route by queue depth and model affinity

---

## 2. Continuous Batching — Multi-Model, Multi-Server

### What It Is

Continuous batching (Orca, vLLM, SGLang) allows new requests to join an in-flight batch
at iteration boundaries rather than waiting for all prior requests to complete. This eliminates
the "convoy effect" where long requests block short ones.

### Ollama's Behavior

Ollama handles batching internally per-server. The `num_parallel` setting controls how many
requests it batches simultaneously. As of 2025, Ollama does NOT expose iteration-level
scheduling externally.

### Multi-Model, Multi-Server (vLLM Router, SGLang Router)

The production pattern (as of December 2025, vLLM Router release) for multi-model multi-server:

```
Client → Router → [Server A: model-X, model-Y]
                → [Server B: model-Z]
                → [Server C: model-X]
```

**SGLang Cache-Aware Load Balancer** (v0.4, Dec 2024):
- Demonstrated throughput improvement from 82,665 → 158,596 token/s
- Cache hit rate improvement from 20% → 75%
- Algorithm: **power-of-two choices** with cache affinity scoring

### Practical Recommendation for Veronex

Implement iteration-level awareness at the routing layer:
1. Track `active_slots` per (server, model) pair
2. Route to the server with available capacity for that model (avoids model-swap latency)
3. When no server has the model loaded, trigger proactive preload on the least-loaded server

---

## 3. Model Placement Optimization — VRAM Bin Packing

### Academic Systems

| System | Approach | Key Insight |
|--------|----------|-------------|
| **AlpaServe** (OSDI'23) | Statistical multiplexing with model parallelism | 10x higher request rates within SLO via parallel placement |
| **Aegaeon** (SOSP'25, Alibaba) | Token-level auto-scaling, component reuse | 82% GPU savings; 2-2.5x goodput; 97% reduced auto-scaling overhead |
| **Helix** (ASPLOS'25, CMU) | Max-flow on directed weighted graph for heterogeneous GPUs | Optimal placement via MILP on heterogeneous hardware |
| **Shepherd** | Earlier work, smaller models (ResNet-scale) | Less applicable to LLMs |

### Aegaeon — Most Relevant (SOSP'25)

Aegaeon (Alibaba Cloud, deployed at scale serving 1.8B–72B models) uses:
- **Proxy layer** → **GPU pool** → **Memory manager** (three-tier architecture)
- Token-level auto-scaling: preemptively scales down active models, scales up pending ones
- KV cache synchronization with fine-grained memory management
- Reduced GPU count from 1,192 → 213 (82% savings) in production

**Key lesson**: Pack multiple models per server, not one model per server. Use memory-aware
eviction rather than static assignment.

### VRAM Bin Packing Algorithm for Veronex

**Inputs**:
- `server_vram_free[i]` — free VRAM on server i
- `model_vram[m]` — VRAM required for model m (weights + KV buffer)
- `demand[m]` — pending request count for model m (sampled from queue, see §5)

**Algorithm** (First-Fit Decreasing by demand × model_size):

```
1. Sort models by demand[m] descending (highest demand first)
2. For each model m with demand[m] > 0:
   a. Find servers where model m is already loaded → prefer these (no swap cost)
   b. If no loaded server available:
      - Find server i with max(server_vram_free[i]) >= model_vram[m]
      - Assign model m to server i; deduct model_vram[m] from server_vram_free[i]
3. For servers with remaining VRAM after step 2:
   - Preload next-highest-demand model that fits
```

**Eviction policy** (when VRAM is full and new model needed):
- Evict model with lowest `(demand[m] * recency_score)` — LRU weighted by demand

---

## 4. KV Cache-Aware Routing

### State of the Art (2025-2026)

| System | Strategy | Result |
|--------|----------|--------|
| **SGLang Router** | Cache-aware (prefix hash → consistent server) | 75% cache hit rate vs 20% round-robin |
| **DualMap** (Feb 2026) | Dual-hash-ring: two candidate servers, pick better | 2.25x effective capacity vs prior SOTA |
| **GORGO** (Feb 2026) | Regional summaries, local routing decisions | Cross-region without tight coordination |
| **llm-d / Envoy** (Red Hat, 2025) | Envoy + inference scheduler with prefix-cache awareness | 57x faster response on cached prefixes |
| **Llumnix** (OSDI'24, Alibaba) | Live KV cache migration across instances | P99 prefill latency reduced 15x |

### DualMap Algorithm (Best Approach, Feb 2026)

```
hash1 = hash(request_prefix) mod N_servers
hash2 = alternate_hash(request_prefix) mod N_servers

candidate_A = servers[hash1]
candidate_B = servers[hash2]

# Pick the one with better cache AND acceptable load
if cache_hit(candidate_A) and load(candidate_A) < SLO_threshold:
    route to candidate_A
elif cache_hit(candidate_B) and load(candidate_B) < SLO_threshold:
    route to candidate_B
else:
    route to min_load(candidate_A, candidate_B)  # power-of-two fallback
```

### Ollama-Specific KV Cache Limitation

Ollama does NOT expose KV cache state via API. There is no way to query "does server X
have the prefix cached for model Y?" externally.

**Workaround**: Implement **sticky routing by session/prefix hash**:
- Hash the first N tokens of the system prompt + conversation context
- Always route that hash to the same server (consistent hashing ring)
- This ensures KV cache reuse without requiring Ollama to expose cache state

### Practical Recommendation

Use **consistent hashing** keyed on `(model_id, session_id OR prefix_hash)`:
- Same session always hits the same server → implicit KV cache reuse
- On server failure, rehash to next node in ring
- Weight ring nodes by VRAM capacity (larger servers get more hash slots)

---

## 5. Queue Demand Sampling — Without Dequeuing

### The Problem

The priority queue uses Valkey LPUSH/RPUSH + BLPOP for FIFO per priority level.
We need to know **how many requests per model** are waiting without consuming them.

### Solutions

#### Option A: Sorted Set Shadow Queue (Recommended)

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

#### Option B: Per-Model Counters (Simpler)

Maintain `INCR demand:{model_id}` on enqueue, `DECR demand:{model_id}` on dequeue.
Atomic counter gives instant O(1) demand read per model.

**Trade-off**: No information about individual job priorities or ages, but sufficient for
placement decisions.

#### Option C: LRANGE Peek (Existing List Queue)

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
