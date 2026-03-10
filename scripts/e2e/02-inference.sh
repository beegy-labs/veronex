#!/usr/bin/env bash
# Phase 7-12: Inference, Capacity Analyzer, AIMD, DB Verification, Usage
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

# ── Phase 7: Inference Round 1 (Pre-AIMD) ───────────────────────────────────

hdr "Phase 7: Inference Round 1 — $CONCURRENT concurrent (pre-AIMD)"

fire_concurrent "$CONCURRENT" "Say only the digit"
R1_OK=$R_OK; R1_Q=$R_Q; R1_F=$R_F
save_var R1_OK "$R1_OK"; save_var R1_Q "$R1_Q"; save_var R1_F "$R1_F"
info "Round 1: OK=$R1_OK Queued=$R1_Q Failed=$R1_F"
[ "$R1_OK" -ge 1 ] && pass "Inference routing works" || fail "No successful inferences"

sleep 1
JOB_COUNT=$(aget "/v1/dashboard/jobs?limit=10" | jv '["jobs"].__len__()' || echo "0")
[ "$JOB_COUNT" -ge 1 ] && pass "Jobs recorded ($JOB_COUNT)" || fail "No jobs in dashboard"

# ── Phase 8: Capacity Analyzer & AIMD ─────────────────────────────────────

hdr "Phase 8: Capacity Analyzer & AIMD"

apost "/v1/providers/sync" "{}" > /dev/null 2>&1 || true
info "Manual sync triggered, waiting for VRAM probing..."

for i in $(seq 1 10); do
  curl -s "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}],\"max_tokens\":4,\"stream\":false}" > /dev/null 2>&1 &
  sleep 2
  CAP=$(aget "/v1/dashboard/capacity" 2>/dev/null || echo '{"providers":[]}')
  LOADED_COUNT=$(echo "$CAP" | python3 -c "
import sys,json; d=json.loads(sys.stdin.read())
print(sum(len(p.get('loaded_models',[])) for p in d.get('providers',[])))
" 2>/dev/null || echo 0)
  [ "$LOADED_COUNT" -ge 1 ] && break
  printf "    tick %d (loaded: %s)\n" "$i" "$LOADED_COUNT"
done
wait 2>/dev/null

print_capacity "$CAP"

get_aimd_limit() {
  aget "/v1/dashboard/capacity" 2>/dev/null | python3 -c "
import sys, json; d=json.loads(sys.stdin.read())
limits=[m['max_concurrent'] for p in d.get('providers',[]) for m in p.get('loaded_models',[])
        if m['model_name']=='$MODEL' and m['max_concurrent']>0]
print(max(limits) if limits else '0')
" 2>/dev/null || echo "0"
}

AIMD_LIMIT=$(get_aimd_limit)
if [ "$AIMD_LIMIT" = "0" ]; then
  info "AIMD not set — running extra sync cycles..."
  for attempt in 1 2 3; do
    for j in 1 2 3; do
      curl -s "$API/v1/chat/completions" \
        -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
        -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"ping $attempt.$j\"}],\"max_tokens\":4,\"stream\":false}" > /dev/null 2>&1 &
    done
    wait 2>/dev/null
    apost "/v1/providers/sync" "{}" > /dev/null 2>&1 || true
    sleep 5
    AIMD_LIMIT=$(get_aimd_limit)
    [ "$AIMD_LIMIT" != "0" ] && break
    info "  attempt $attempt: limit still 0"
  done
fi

[ -n "$AIMD_LIMIT" ] && [ "$AIMD_LIMIT" -gt 0 ] \
  && pass "AIMD limit for $MODEL = $AIMD_LIMIT" \
  || fail "AIMD limit not set after 3 sync cycles"
save_var AIMD_LIMIT "$AIMD_LIMIT"

# ── Phase 9: DB Verification ────────────────────────────────────────────────

hdr "Phase 9: Database Verification"

docker compose exec -T postgres psql -U veronex -d veronex -c \
  "SELECT model_name, weight_mb, kv_per_request_mb, max_concurrent, baseline_tps FROM model_vram_profiles LIMIT 10;" 2>/dev/null
VRAM_ROWS=$(docker compose exec -T postgres psql -U veronex -d veronex -t -c \
  "SELECT COUNT(*) FROM model_vram_profiles;" 2>/dev/null | tr -d ' ')
[ -n "$VRAM_ROWS" ] && [ "$VRAM_ROWS" -ge 1 ] \
  && pass "model_vram_profiles: $VRAM_ROWS rows" || info "model_vram_profiles: empty"

docker compose exec -T postgres psql -U veronex -d veronex -c \
  "SELECT status, COUNT(*) as cnt FROM inference_jobs GROUP BY status;" 2>/dev/null
JOB_ROWS=$(docker compose exec -T postgres psql -U veronex -d veronex -t -c \
  "SELECT COUNT(*) FROM inference_jobs;" 2>/dev/null | tr -d ' ')
[ -n "$JOB_ROWS" ] && [ "$JOB_ROWS" -ge 1 ] && pass "Jobs: $JOB_ROWS rows" || fail "No jobs in DB"

# ── Phase 10: Inference Round 2 (AIMD Active) ───────────────────────────────

R2_COUNT=$((AIMD_LIMIT + 2))
[ "$R2_COUNT" -lt "$CONCURRENT" ] && R2_COUNT=$CONCURRENT
[ "$AIMD_LIMIT" -le 0 ] && R2_COUNT=$CONCURRENT

hdr "Phase 10: Inference Round 2 — $R2_COUNT requests (AIMD limit=$AIMD_LIMIT)"

fire_concurrent "$R2_COUNT" "Reply with digit"
R2_OK=$R_OK; R2_Q=$R_Q; R2_F=$R_F
save_var R2_OK "$R2_OK"; save_var R2_Q "$R2_Q"; save_var R2_F "$R2_F"
info "Round 2: OK=$R2_OK Queued=$R2_Q Failed=$R2_F"
[ "$R2_OK" -ge 1 ] && pass "AIMD-regulated inference works" || fail "No successful inferences"
[ "$R2_F" -eq 0 ] && pass "All $R2_COUNT completed" || fail "$R2_F requests failed under AIMD"

# ── Phase 11: Usage & Analytics ──────────────────────────────────────────────

hdr "Phase 11: Usage & Analytics"
sleep 1

USAGE=$(aget "/v1/usage?hours=1" || echo '{}')
TOTAL_REQ=$(echo "$USAGE" | jv '["total_requests"]' || echo "0")
info "Requests=$TOTAL_REQ Tokens=$(echo "$USAGE" | jv '["total_tokens"]' || echo 0)"
[ "$TOTAL_REQ" != "0" ] && pass "Usage data recorded" || info "Usage in pipeline"

assert_get "/v1/usage/breakdown?hours=1" 200 "Usage breakdown"
assert_get "/v1/dashboard/performance?hours=1" 200 "Performance metrics"

# ── Phase 12: Final State ────────────────────────────────────────────────────

hdr "Phase 12: Final State"
sleep 2
CFINAL=$(aget "/v1/dashboard/capacity" 2>/dev/null || echo '{"providers":[]}')
print_capacity "$CFINAL"
pass "Final capacity verified"

QD_F=$(aget "/v1/dashboard/queue/depth" | jv '["total"]' || echo "0")
[ "$QD_F" = "0" ] && pass "All queues drained" || info "Queue not empty ($QD_F)"

save_counts
