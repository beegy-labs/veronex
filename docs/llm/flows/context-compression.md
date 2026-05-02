# Flow — Context Compression

> **Last Updated**: 2026-04-06

---

## Multi-Turn Request Flow

```
POST /v1/chat/completions  (conversation_id header present)
  │
  ├─ load conversation from Valkey → S3 (7-day search)
  │
  ├─ [Phase 6] session_handoff::should_handoff()?
  │     YES → generate_master_summary() → new S3 record → new conversation_id
  │     NO  → continue
  │
  ├─ [Phase 4] context_assembler::check_multiturn_eligibility()
  │     FAIL → 400 { code, message }
  │     PASS → continue
  │
  ├─ [Phase 5] compress_input_inline()  (if last user msg > 50% budget)
  │     SUCCESS → replace last user message content
  │     FAIL    → use original (fail-open)
  │
  ├─ [Phase 4] context_assembler::assemble()
  │     → replace messages with: [compressed summaries] + [verbatim window]
  │     → enforce token budget (drop oldest first)
  │
  ├─ submit job → stream response
  │
  └─ [Phase 3, async] compress_turn() via tokio::spawn
        → call compression model
        → rewrite TurnRecord.compressed in S3
        → DEL Valkey conversation cache
```

---

## Compression Router Decision

```
compression_router::decide(turn, lab, model_ctx)
  │
  ├─ compression disabled? → Skip
  ├─ turn too short?       → Skip
  ├─ dedicated model set?  → AsyncDedicated
  ├─ provider idle?        → AsyncIdle
  └─ default              → SyncInline
```

---

## Session Handoff Detail

```
should_handoff(turns, lab, configured_ctx)
  sum(turn.compressed.compressed_tokens)
    > handoff_threshold × configured_ctx ?
  │
  YES:
    generate_master_summary(http_client, record, model, provider_url, timeout_secs)
      → prompt: all compressed summaries → one paragraph
      → call compression model via the shared http_client
        (NEVER reqwest::Client::new() — connection pool reuse)

    perform_handoff(http_client, record, prev_conv_id, owner_id, date,
                    model, provider_url, timeout_secs, store)
      → new ConversationRecord { turns: [HandoffTurn { master_summary }] }
      → S3 put_conversation(new_id)
      → return (new_id, session_renewed=true)

    FAIL → log warn → return (original_id, session_renewed=false)

  NO:
    return (original_id, session_renewed=false)
```

---

## Context Assembly Detail

```
assemble(turns, lab, configured_ctx)
  │
  verbatim_boundary = len(turns) - recent_verbatim_window
  budget = configured_ctx × context_budget_ratio
  │
  for each turn (oldest first):
    if idx >= verbatim_boundary:
      use raw content
    else if turn.compressed exists:
      use turn.compressed.summary
    else:
      use raw content (not yet compressed)
  │
  enforce_budget(messages, budget):
    while estimated_tokens > budget:
      drop oldest non-system message
  │
  return assembled messages
```

---

## Failure Modes (all fail-open)

| Failure | Effect |
|---------|--------|
| Compression model unavailable | Turn stays uncompressed; assembly uses raw |
| Compression timeout | Skip compression; fail-open |
| S3 write fails on handoff | Use original conversation_id |
| Valkey miss on conversation load | S3 fallback (7-day search loop) |
| max_ctx unknown (Valkey miss) | Eligibility check passes (fail-open) |
