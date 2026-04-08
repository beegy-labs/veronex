# Multi-Instance Pub/Sub Relay

> **Last Updated**: 2026-03-28

---

## Overview

Cross-instance coordination via Valkey for job events, token streaming, and cancellation.
Two transport mechanisms: **pub/sub** for events/cancel, **Streams** for tokens.

---

## Job Event Relay

```
Instance A                        Valkey                       Instance B
    │                               │                              │
    │  publish_job_event()          │                              │
    │  PUBLISH veronex:pubsub:     │                              │
    │    job_events {payload+      │                              │
    │    instance_id}              │                              │
    │ ─────────────────────────►   │                              │
    │                               │  run_job_event_subscriber() │
    │                               │  ◄────────────────────────  │
    │                               │  SUBSCRIBE same channel     │
    │                               │                              │
    │                               │  recv message ──────────►   │
    │                               │  skip if instance_id==self  │
    │                               │  else → local broadcast_tx  │
```

---

## Token Streaming (Valkey Streams)

Uses XADD/XREAD instead of pub/sub to prevent the "initial token black hole"
(tokens published before subscriber connects are lost with plain pub/sub).

```
publish_token(job_id, token):
  key = stream_tokens(job_id)
  EVAL LUA_XADD_EXPIRE:
    XADD key MAXLEN ~ 500 * v=value f=is_final
    EXPIRE key TOKEN_STREAM_TTL_SECS

cleanup_token_stream(job_id):
  DEL stream_tokens(job_id)     // called on job completion
  (fallback: EXPIRE auto-deletes after TTL)
```

Late-connecting subscribers read from `0-0` to catch up on all buffered tokens.

---

## Cancel Relay

```
Instance A (cancel requester)         Instance B (job runner)
    │                                       │
    │  publish_cancel(job_id)               │
    │  PUBLISH veronex:pubsub:cancel:{id}   │
    │ ──────────────────────────────────►    │
    │                                       │
    │        run_cancel_subscriber()        │
    │        PSUBSCRIBE veronex:pubsub:     │
    │          cancel:*                     │
    │                                       │
    │        extract job_id from channel    │
    │        cancel_notifiers.get(job_id)   │
    │          → notify_one()              │
```

---

## Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `TOKEN_STREAM_TTL_SECS` | 600 (10m) | Safety-net EXPIRE on stream keys |
| `MAXLEN` | ~500 | Stream cap per job |
| `PUBSUB_JOB_EVENTS` | `veronex:pubsub:job_events` | Job event channel |
| `PUBSUB_CANCEL_PATTERN` | `veronex:pubsub:cancel:*` | Cancel pattern subscribe |

---

## Files

| File | Role |
|------|------|
| `crates/veronex/src/infrastructure/outbound/pubsub/relay.rs` | All pub/sub logic |
| `crates/veronex/src/infrastructure/outbound/valkey_keys.rs` | Key/channel name constants |
| `crates/veronex/src/bootstrap/background.rs` | Subscriber client init + task spawn |
| `crates/veronex/src/domain/value_objects.rs` | `JobStatusEvent`, `StreamToken` |
