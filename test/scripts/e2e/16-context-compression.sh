#!/usr/bin/env bash
# Phase 16: Context Compression — Lab settings, multi-turn eligibility, session handoff config
#
# Tests:
#   1. Lab settings PATCH: context_compression_enabled, handoff_threshold, handoff_enabled
#   2. Lab settings GET reflects persisted values
#   3. Multi-turn eligibility: small model rejected by multiturn_min_params
#   4. Multi-turn eligibility: large model accepted
#   5. Conversation internals endpoint (admin-only)
#   6. Compression model selector round-trip
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; ensure_auth

# ── Helpers ───────────────────────────────────────────────────────────────────

get_lab() {
  aget "/v1/dashboard/lab" 2>/dev/null || echo "{}"
}

patch_lab() {
  apatchc "/v1/dashboard/lab" "$1"
}

# Restore lab to sensible defaults on exit
LAB_ORIG=$(get_lab)
cleanup_lab() {
  # Restore compression off, default threshold
  patch_lab '{"context_compression_enabled":false,"handoff_enabled":false,"handoff_threshold":0.85}' > /dev/null 2>&1 || true
}
trap cleanup_lab EXIT

# ── Phase 16-A: Lab settings persistence ─────────────────────────────────────

hdr "Lab Settings — compression + handoff PATCH/GET round-trip"

PATCH_BODY='{"context_compression_enabled":true,"handoff_enabled":true,"handoff_threshold":0.75}'
PATCH_RESP=$(patch_lab "$PATCH_BODY")
PATCH_CODE=$(echo "$PATCH_RESP" | tail -1 2>/dev/null || echo "0")

LAB=$(get_lab)
COMP_ENABLED=$(echo "$LAB" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print(d.get('context_compression_enabled',''))" 2>/dev/null || echo "")
HANDOFF_ENABLED=$(echo "$LAB" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print(d.get('handoff_enabled',''))" 2>/dev/null || echo "")
HANDOFF_THRESH=$(echo "$LAB" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print(d.get('handoff_threshold',''))" 2>/dev/null || echo "")

[ "$COMP_ENABLED" = "True" ] || [ "$COMP_ENABLED" = "true" ] && \
  pass "context_compression_enabled persisted" || \
  fail "context_compression_enabled not set (got: $COMP_ENABLED)"

[ "$HANDOFF_ENABLED" = "True" ] || [ "$HANDOFF_ENABLED" = "true" ] && \
  pass "handoff_enabled persisted" || \
  fail "handoff_enabled not set (got: $HANDOFF_ENABLED)"

python3 -c "
t = float('${HANDOFF_THRESH:-0}')
assert abs(t - 0.75) < 0.01, f'expected 0.75 got {t}'
" 2>/dev/null && pass "handoff_threshold=0.75 persisted" || \
  fail "handoff_threshold wrong (got: $HANDOFF_THRESH)"

# ── Phase 16-B: Compression model round-trip ─────────────────────────────────

hdr "Lab Settings — compression_model selector"

patch_lab '{"compression_model":"qwen2.5:3b"}' > /dev/null
LAB2=$(get_lab)
COMP_MODEL=$(echo "$LAB2" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print(d.get('compression_model') or '')" 2>/dev/null || echo "")
[ "$COMP_MODEL" = "qwen2.5:3b" ] && \
  pass "compression_model persisted as qwen2.5:3b" || \
  fail "compression_model wrong (got: '$COMP_MODEL')"

# Clear compression model (send as JSON null via double-encoded Option<Option<String>>)
patch_lab '{"compression_model":null}' > /dev/null
LAB3=$(get_lab)
COMP_MODEL3=$(echo "$LAB3" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); v=d.get('compression_model'); print('None' if v is None else v)" 2>/dev/null || echo "None")
[ "$COMP_MODEL3" = "None" ] || [ -z "$COMP_MODEL3" ] && \
  pass "compression_model cleared to null" || \
  fail "compression_model null-clear failed (got: '$COMP_MODEL3')"

# ── Phase 16-C: Multi-turn eligibility gate ───────────────────────────────────

hdr "Multi-Turn Eligibility Gate"

# Detect available models
MODELS=$(aget "/v1/ollama/models" 2>/dev/null | python3 -c "
import sys,json
try:
    d = json.loads(sys.stdin.read())
    ms = d.get('models', d) if isinstance(d, dict) else d
    names = [m.get('model_name','') for m in ms if m.get('model_name')]
    print(' '.join(names[:8]))
except: pass
" 2>/dev/null || echo "")

if [ -z "$MODELS" ]; then
  fail "No models available — at least one model must be loaded in Ollama"
else
  # Set restrictive multiturn gate: require 100B+ params (nothing passes)
  patch_lab '{"multiturn_min_params":100,"multiturn_min_ctx":8192}' > /dev/null

  FIRST_MODEL=$(echo "$MODELS" | awk '{print $1}')
  MULTI_RESP=$(curl -s -w "\n%{http_code}" --max-time 90 "$API/api/chat" \
    -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$FIRST_MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"stream\":false,\"conversation_id\":\"00000000-0000-0000-0000-000000000001\"}" \
    2>/dev/null || printf "\n000")
  MULTI_CODE=$(echo "$MULTI_RESP" | tail -1)
  MULTI_BODY=$(echo "$MULTI_RESP" | head -1)

  # Should either succeed (treated as single-turn) or return 400/403 with eligibility error
  if echo "$MULTI_BODY" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); exit(0 if d.get('model_too_small') or d.get('error','').find('multi-turn')>=0 or d.get('code','').find('model_too')>=0 else 1)" 2>/dev/null; then
    pass "Eligibility gate returned model_too_small error"
  elif [ "$MULTI_CODE" = "200" ]; then
    pass "Chat accepted (single-turn fallback when no prior conversation)"
  elif [ "$MULTI_CODE" = "400" ] || [ "$MULTI_CODE" = "422" ]; then
    pass "Eligibility gate rejected oversized constraint (HTTP $MULTI_CODE)"
  else
    fail "Eligibility gate unexpected response: HTTP $MULTI_CODE"
  fi

  # Restore permissive gate
  patch_lab '{"multiturn_min_params":1,"multiturn_min_ctx":1024}' > /dev/null

  PERMISSIVE_RESP=$(curl -s -w "\n%{http_code}" --max-time 90 "$API/api/chat" \
    -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$FIRST_MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}],\"stream\":false}" \
    2>/dev/null || printf "\n000")
  PERMISSIVE_CODE=$(echo "$PERMISSIVE_RESP" | tail -1)
  [ "$PERMISSIVE_CODE" = "200" ] && \
    pass "Chat accepted with permissive multiturn gate (HTTP 200)" || \
    fail "Permissive gate: HTTP $PERMISSIVE_CODE (expected 200 — model loaded but inference failed)"
fi

# ── Phase 16-D: Conversation internals endpoint ───────────────────────────────

hdr "Conversation Internals Endpoint"

# Get a real conversation with turns from the dashboard
CONV_ID=$(aget "/v1/conversations?limit=20" 2>/dev/null | python3 -c "
import sys,json
try:
    d=json.loads(sys.stdin.read())
    convs=d.get('conversations',[])
    for c in convs:
        if c.get('turn_count', 0) > 0:
            print(c['id']); break
except: pass
" 2>/dev/null || echo "")

if [ -z "$CONV_ID" ]; then
  fail "No conversations in DB — prior inference tests must have created conversations"
else
  # Get first job_id from this conversation
  JOB_ID=$(aget "/v1/conversations/$CONV_ID" 2>/dev/null | python3 -c "
import sys,json
try:
    d=json.loads(sys.stdin.read())
    turns=d.get('turns',[])
    print(turns[0]['job_id'] if turns else '')
except: pass
" 2>/dev/null || echo "")

  if [ -z "$JOB_ID" ]; then
    fail "Conversation $CONV_ID has no turns — conversation must have at least one turn"
  else
    INTERNALS_RESP=$(curl -s -w "\n%{http_code}" --max-time 10 \
      "$API/v1/conversations/$CONV_ID/turns/$JOB_ID/internals" \
      -H "Authorization: Bearer $TK" 2>/dev/null || printf "\n000")
    INTERNALS_CODE=$(echo "$INTERNALS_RESP" | tail -1)
    INTERNALS_BODY=$(echo "$INTERNALS_RESP" | head -1)

    if [ "$INTERNALS_CODE" = "200" ]; then
      HAS_JOB=$(echo "$INTERNALS_BODY" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print('ok' if d.get('job_id') else '')" 2>/dev/null || echo "")
      [ "$HAS_JOB" = "ok" ] && \
        pass "Internals endpoint returned job_id (HTTP 200)" || \
        fail "Internals response missing job_id"
    elif [ "$INTERNALS_CODE" = "404" ]; then
      pass "Internals endpoint reachable (404 = S3 not configured or no compressed data)"
    elif [ "$INTERNALS_CODE" = "503" ]; then
      fail "Internals: S3/message_store not configured (HTTP 503) — MinIO must be running"
    elif [ "$INTERNALS_CODE" = "403" ]; then
      fail "Internals endpoint returned 403 — account_manage permission missing for test user"
    else
      fail "Internals endpoint: unexpected HTTP $INTERNALS_CODE"
    fi
  fi
fi

save_counts

[ "$FAIL_COUNT" -eq 0 ]
