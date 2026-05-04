# Job Event & Stats Streaming

> **Last Updated**: 2026-03-28

---

## Architecture

```
                    InferenceUseCase
                         │
                    broadcast_event()
                         │
                         ▼
              ┌─ broadcast::channel<JobStatusEvent> (cap=256) ─┐
              │                    │                            │
              ▼                    ▼                            ▼
        Ring Buffer Task     Bucket Counter Task          SSE Clients
        (last 100 events)   (60×1s sliding window)       (job_events_sse)
              │                    │
              ▼                    ▼
        VecDeque<(evt,ts)>   incoming_buckets[60]
        replay on connect    completed_buckets[60]
                                   │
                                   ▼
                          ┌─ Stats Ticker (1s) ─┐
                          │  reads Valkey        │
                          │  pending/running     │
                          │  counters            │
                          ▼                      │
                   broadcast::channel<FlowStats> │
                          (cap=16)               │
                          │                      │
                          ▼                      │
                     SSE Clients ◄───────────────┘
```

---

## Ring Buffer

```
on broadcast recv:
  ts = now_ms()
  if buf.len >= EVENT_BUFFER_CAPACITY:
    buf.pop_front()
  buf.push_back((event, ts))
```

Late-connecting SSE clients receive all buffered events on connect,
then transition to live `tokio::select!` on both channels.

---

## FlowStats Ticker

```
every STATS_TICK_INTERVAL:
  rotate bucket_idx = (tick_count % 60)
  clear incoming_buckets[new_idx], completed_buckets[new_idx]

  incoming    = sum(incoming_buckets[idx-9..=idx])    // req/s (10s window)
  incoming_60 = sum(incoming_buckets[0..60])           // req/m
  completed   = sum(completed_buckets[0..60])

  if valkey available:
    queued  = GET JOBS_PENDING_COUNTER
    running = GET JOBS_RUNNING_COUNTER
    if tick_count % 60 == 0:
      reconcile counters from DB (SELECT COUNT GROUP BY status)
  else:
    cached DB query every 10 ticks

  broadcast FlowStats { incoming, incoming_60s, queued, running, completed }
```

---

## SSE Endpoint (`GET /v1/dashboard/jobs/stream`)

```
1. try_acquire_sse() — atomic counter, 429 if >= SSE_MAX_CONNECTIONS
2. subscribe to job_event_tx AND stats_tx BEFORE reading ring buffer
3. replay ring buffer as "job_status" SSE events (with ts field)
4. loop tokio::select! (unbiased — fair interleaving):
     job_rx.recv()  → yield SSE event "job_status"  (JSON + ts)
     stats_rx.recv() → yield SSE event "flow_stats" (JSON)
5. with_sse_timeout wraps stream (SSE_TIMEOUT hard deadline)
6. SseDropGuard decrements counter on stream drop
```

---

## Constants

| Constant | Value | Location |
|----------|-------|----------|
| `EVENT_BUFFER_CAPACITY` | 100 | `bootstrap/background.rs` |
| `STATS_TICK_INTERVAL` | 1s | `domain/constants.rs` |
| `SSE_MAX_CONNECTIONS` | 100 | `http/constants.rs` |
| `SSE_KEEP_ALIVE` | 15s | `http/constants.rs` |
| `SSE_TIMEOUT` | 1700s | `http/constants.rs` (held strictly below Cilium gateway 1800s; see `inference/mcp.md § timeouts`) |
| `INFERENCE_ROUTER_TIMEOUT` | 1750s | `http/constants.rs` (non-streaming router fallback above SSE_TIMEOUT) |
| broadcast job events cap | 256 | `bootstrap/background.rs` |
| broadcast stats cap | 16 | `bootstrap/background.rs` |
| Bucket window (req/s) | 10 buckets | `bootstrap/background.rs` |
| Bucket window (req/m) | 60 buckets | `bootstrap/background.rs` |
| Counter reconcile | every 60 ticks | `bootstrap/background.rs` |

---

## Files

| File | Role |
|------|------|
| `crates/veronex/src/bootstrap/background.rs` | Ring buffer task, bucket counter, stats ticker |
| `crates/veronex/src/infrastructure/inbound/http/dashboard_handlers.rs` | `job_events_sse()` SSE endpoint |
| `crates/veronex/src/infrastructure/inbound/http/handlers.rs` | `try_acquire_sse()`, `sse_response()`, `with_sse_timeout()` |
| `crates/veronex/src/infrastructure/inbound/http/constants.rs` | SSE constants |
| `crates/veronex/src/domain/constants.rs` | `STATS_TICK_INTERVAL` |
| `crates/veronex/src/domain/value_objects.rs` | `FlowStats`, `JobStatusEvent` |
