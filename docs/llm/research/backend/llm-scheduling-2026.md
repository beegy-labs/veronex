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

## Continued

> Queue demand sampling, production summary, implementation recommendations, and references:
> `docs/llm/research/backend/llm-scheduling-demand-2026.md`
