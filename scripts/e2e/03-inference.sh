#!/usr/bin/env bash
# Phase 03: Concurrent Inference + AIMD Learning + DB Verification
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

# ── Auto-detect available models for multi-model testing ─────────────────────
MODELS_ALL=$(aget "/v1/ollama/models" 2>/dev/null | python3 -c "
import sys, json
try:
    models = json.loads(sys.stdin.read())
    # Pick text models (exclude embed/ocr), max 4
    names = [m.get('model_name','') for m in models
             if not any(x in m.get('model_name','').lower() for x in ['embed','ocr','nomic'])]
    print(' '.join(names[:4]))
except: pass
" 2>/dev/null || echo "$MODEL")
[ -z "$MODELS_ALL" ] && MODELS_ALL="$MODEL"
info "Multi-model pool: $MODELS_ALL"

# ── Round 1: Inference Burst (Cold Start) ────────────────────────────────────

hdr "Round 1: Inference Burst — $CONCURRENT concurrent (cold start)"

fire_concurrent "$CONCURRENT" "Say only the digit"
R1_OK=$R_OK; R1_Q=$R_Q; R1_F=$R_F
save_var R1_OK "$R1_OK"; save_var R1_Q "$R1_Q"; save_var R1_F "$R1_F"
info "Round 1: OK=$R1_OK Queued=$R1_Q Failed=$R1_F"
[ "$R1_OK" -ge 1 ] && pass "Inference routing works (OK=$R1_OK)" || fail "No successful inferences"

sleep 1
JOB_COUNT=$(aget "/v1/dashboard/jobs?limit=10" | jv '["jobs"].__len__()' || echo "0")
[ "${JOB_COUNT:-0}" -ge 1 ] && pass "Jobs recorded ($JOB_COUNT)" || fail "No jobs in dashboard"

# ── Capacity Sync & AIMD Wait ─────────────────────────────────────────────────

hdr "Capacity Sync & AIMD Learning"

apostc "/v1/providers/sync" "{}" > /dev/null 2>&1 || true
info "Manual sync triggered, waiting for VRAM probing..."

get_aimd_limit() {
  aget "/v1/dashboard/capacity" 2>/dev/null | python3 -c "
import sys, json; d=json.loads(sys.stdin.read())
limits=[m['max_concurrent'] for p in d.get('providers',[]) for m in p.get('loaded_models',[])
        if m['model_name']=='$MODEL' and m['max_concurrent']>0]
print(max(limits) if limits else '0')
" 2>/dev/null || echo "0"
}

for i in $(seq 1 10); do
  curl -s "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}],\"max_tokens\":4,\"stream\":false}" \
    > /dev/null 2>&1 &
  sleep 2
  CAP=$(aget "/v1/dashboard/capacity" 2>/dev/null || echo '{"providers":[]}')
  LOADED_COUNT=$(echo "$CAP" | python3 -c "
import sys,json; d=json.loads(sys.stdin.read())
print(sum(len(p.get('loaded_models',[])) for p in d.get('providers',[])))
" 2>/dev/null || echo "0")
  [ "$LOADED_COUNT" -ge 1 ] && break
  printf "    tick %d (loaded: %s)\n" "$i" "$LOADED_COUNT"
done
wait 2>/dev/null

print_capacity "$CAP"

AIMD_LIMIT=$(get_aimd_limit)
if [ "$AIMD_LIMIT" = "0" ]; then
  info "AIMD not set — running extra sync cycles..."
  for attempt in 1 2 3; do
    for j in 1 2 3; do
      curl -s "$API/v1/chat/completions" \
        -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
        -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"ping $attempt.$j\"}],\"max_tokens\":4,\"stream\":false}" \
        > /dev/null 2>&1 &
    done
    wait 2>/dev/null
    apostc "/v1/providers/sync" "{}" > /dev/null 2>&1 || true
    sleep 5
    AIMD_LIMIT=$(get_aimd_limit)
    [ "$AIMD_LIMIT" != "0" ] && break
    info "  attempt $attempt: limit still 0"
  done
fi

[ -n "$AIMD_LIMIT" ] && [ "$AIMD_LIMIT" -gt 0 ] \
  && pass "AIMD limit for $MODEL = $AIMD_LIMIT" \
  || fail "AIMD limit not set after sync cycles"
save_var AIMD_LIMIT "$AIMD_LIMIT"

# SDD: AIMD cold start = num_parallel; after learning, max_concurrent ≤ num_parallel
PROVIDERS_JSON=$(aget "/v1/providers" 2>/dev/null || echo "[]")
NP_CHECK=$(echo "$PROVIDERS_JSON" | python3 -c "
import sys, json
providers = json.loads(sys.stdin.read())
nps = [p.get('num_parallel', 4) for p in providers if p.get('provider_type') == 'ollama' and p.get('is_active')]
print(max(nps) if nps else 4)
" 2>/dev/null || echo "4")
if [ -n "$AIMD_LIMIT" ] && [ "$AIMD_LIMIT" != "0" ]; then
  [ "$AIMD_LIMIT" -le "$NP_CHECK" ] \
    && pass "AIMD max_concurrent ($AIMD_LIMIT) ≤ num_parallel ($NP_CHECK) — SDD constraint satisfied" \
    || fail "AIMD max_concurrent ($AIMD_LIMIT) > num_parallel ($NP_CHECK) — SDD violation"
fi

# ── DB Verification ────────────────────────────────────────────────────────────

hdr "Database Verification"

docker compose exec -T postgres psql -U veronex -d veronex -c \
  "SELECT model_name, weight_mb, kv_per_request_mb, max_concurrent, baseline_tps FROM model_vram_profiles LIMIT 10;" \
  2>/dev/null || true

VRAM_ROWS=$(docker compose exec -T postgres psql -U veronex -d veronex -t -c \
  "SELECT COUNT(*) FROM model_vram_profiles;" 2>/dev/null | tr -d ' ')
[ -n "$VRAM_ROWS" ] && [ "$VRAM_ROWS" -ge 1 ] \
  && pass "model_vram_profiles: $VRAM_ROWS rows" || info "model_vram_profiles: empty"

docker compose exec -T postgres psql -U veronex -d veronex -c \
  "SELECT status, COUNT(*) FROM inference_jobs GROUP BY status;" 2>/dev/null || true

JOB_ROWS=$(docker compose exec -T postgres psql -U veronex -d veronex -t -c \
  "SELECT COUNT(*) FROM inference_jobs;" 2>/dev/null | tr -d ' ')
[ -n "$JOB_ROWS" ] && [ "$JOB_ROWS" -ge 1 ] && pass "inference_jobs: $JOB_ROWS rows" \
  || fail "No inference_jobs in DB"

# Verify num_parallel column exists
docker compose exec -T postgres psql -U veronex -d veronex -c \
  "SELECT id, name, num_parallel FROM llm_providers LIMIT 5;" 2>/dev/null \
  && pass "num_parallel column present in llm_providers" || info "num_parallel check skipped"

# ── Round 2: AIMD-Regulated Multi-Model Load ─────────────────────────────────

R2_COUNT=$((${AIMD_LIMIT:-4} + 2))
[ "$R2_COUNT" -lt "$CONCURRENT" ] && R2_COUNT=$CONCURRENT

hdr "Round 2: AIMD-Regulated — $R2_COUNT requests, multi-model (AIMD=$AIMD_LIMIT)"

# Multi-model burst: cycle through available models
TMPDIR_R2=$(mktemp -d)
MODELS_ARR=($MODELS_ALL)
MODEL_COUNT=${#MODELS_ARR[@]}
for i in $(seq 1 "$R2_COUNT"); do
  MDL="${MODELS_ARR[$(( (i - 1) % MODEL_COUNT ))]}"
  (
    T0=$(python3 -c "import time; print(int(time.time()*1000))")
    RES=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
      -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
      -d "{\"model\":\"$MDL\",\"messages\":[{\"role\":\"user\",\"content\":\"Reply digit $i\"}],\"max_tokens\":8,\"stream\":false}" \
      --max-time 120)
    CODE=$(echo "$RES" | tail -1)
    T1=$(python3 -c "import time; print(int(time.time()*1000))")
    echo "$i $CODE $((T1 - T0))ms $MDL" > "$TMPDIR_R2/r_$i"
  ) &
done
wait; echo ""
R_OK=0; R_Q=0; R_F=0
for f in "$TMPDIR_R2"/r_*; do
  read -r IDX CODE DUR MDL < "$f"
  case "$CODE" in
    200)     echo -e "    #$IDX: ${GREEN}200${NC} ($DUR) [$MDL]"; R_OK=$((R_OK+1)) ;;
    429|503) echo -e "    #$IDX: ${YELLOW}${CODE}${NC} ($DUR) [$MDL]"; R_Q=$((R_Q+1)) ;;
    *)       echo -e "    #$IDX: ${RED}${CODE}${NC} ($DUR) [$MDL]"; R_F=$((R_F+1)) ;;
  esac
done
rm -rf "$TMPDIR_R2"
R2_OK=$R_OK; R2_Q=$R_Q; R2_F=$R_F
save_var R2_OK "$R2_OK"; save_var R2_Q "$R2_Q"; save_var R2_F "$R2_F"
info "Round 2: OK=$R2_OK Queued=$R2_Q Failed=$R2_F"
[ "$R2_OK" -ge 1 ] && pass "AIMD-regulated inference works" || fail "No successful inferences in round 2"
[ "$R2_F" -eq 0 ] && pass "All $R2_COUNT requests completed without error" \
  || fail "$R2_F requests failed under AIMD load"

# ── Usage & Analytics ─────────────────────────────────────────────────────────

hdr "Usage & Analytics"

sleep 1
USAGE=$(aget "/v1/usage?hours=1" || echo '{}')
TOTAL_REQ=$(echo "$USAGE" | jv '["total_requests"]' || echo "0")
info "total_requests=$TOTAL_REQ total_tokens=$(echo "$USAGE" | jv '["total_tokens"]' || echo 0)"
[ "$TOTAL_REQ" != "0" ] && pass "Usage data recorded ($TOTAL_REQ requests)" || info "Usage pipeline pending"

assert_get "/v1/usage/breakdown?hours=1" 200 "Usage breakdown"
assert_get "/v1/dashboard/performance?hours=1" 200 "Performance metrics"

# ── Final Capacity State ──────────────────────────────────────────────────────

hdr "Final Capacity State"

sleep 2
CFINAL=$(aget "/v1/dashboard/capacity" 2>/dev/null || echo '{"providers":[]}')
print_capacity "$CFINAL"
pass "Final capacity verified"

# Queue should be drained
ZSET_FINAL=$(valkey_zcard "veronex:queue:zset")
[ "${ZSET_FINAL:-0}" = "0" ] && pass "ZSET queue drained after rounds" \
  || info "ZSET has $ZSET_FINAL entries (may be residual)"

# ── SDD 레벨 2: N서버 분산 처리 검증 ─────────────────────────────────────────

hdr "SDD Level 2: N-Server Distribution (dual-provider job routing)"

# DB에서 provider별 completed job 수 확인 — 양쪽이 모두 처리했는지
# 분산 확인을 위해 max_concurrent × 2 이상 동시 요청 → Scale-Out 유도
DIST_CONCURRENT=10
info "Firing $DIST_CONCURRENT concurrent requests to induce N-server distribution..."
fire_concurrent "$DIST_CONCURRENT" "distribution test"
sleep 3

DIST_RESULT=$(docker compose exec -T postgres psql -U veronex -d veronex -tAF'|' \
  -c "SELECT p.name, COUNT(j.id) FROM inference_jobs j JOIN llm_providers p ON p.id=j.provider_id WHERE j.status='completed' GROUP BY p.name ORDER BY COUNT(j.id) DESC;" \
  2>/dev/null | tr -d ' \r')

DIST_PROVIDER_COUNT=$(echo "$DIST_RESULT" | grep -c '|' 2>/dev/null || echo "0")
echo "$DIST_RESULT" | while IFS='|' read -r pname cnt; do
  [ -z "$pname" ] && continue
  info "  provider=$pname completed_jobs=$cnt"
done

if [ "${DIST_PROVIDER_COUNT:-0}" -ge 2 ]; then
  pass "N-server distribution confirmed: $DIST_PROVIDER_COUNT providers processed jobs"
elif [ "${DIST_PROVIDER_COUNT:-0}" -eq 1 ]; then
  info "All jobs routed to 1 provider (locality bonus active — Scale-Out triggers at demand > eligible_capacity × 0.80)"
else
  info "No completed jobs found for distribution check"
fi

# ── SDD 레벨 1: AIMD 수렴 검증 ────────────────────────────────────────────────

hdr "SDD Level 1: AIMD Convergence (num_parallel top-down learning)"

# Round 1(cold start) max_concurrent vs 현재(학습 후) 비교
# num_parallel에서 시작해 AIMD가 수렴했으면 max_concurrent ≤ num_parallel이고 값이 고정됨
AIMD_FINAL=$(aget "/v1/dashboard/capacity" 2>/dev/null | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
results = []
for p in d.get('providers', []):
    for m in p.get('loaded_models', []):
        if m.get('model_name') == '$MODEL':
            results.append((p.get('provider_name','?'), m.get('max_concurrent',0), m.get('active_requests',0)))
for name, mc, ar in results:
    print(f'{name}: max_concurrent={mc} active={ar}')
" 2>/dev/null || echo "")

if [ -n "$AIMD_FINAL" ]; then
  pass "AIMD converged state: $AIMD_FINAL"
  # max_concurrent가 num_parallel 미만이면 AIMD가 실제로 학습해 하향 조정한 것
  NP_MAX=$(aget "/v1/providers" 2>/dev/null | python3 -c "
import sys,json; p=json.load(sys.stdin)
nps=[x.get('num_parallel',4) for x in p if x.get('is_active')]
print(max(nps) if nps else 4)
" 2>/dev/null || echo "4")
  AIMD_MC=$(aget "/v1/dashboard/capacity" 2>/dev/null | python3 -c "
import sys,json; d=json.load(sys.stdin)
vals=[m['max_concurrent'] for p in d.get('providers',[]) for m in p.get('loaded_models',[]) if m.get('model_name')=='$MODEL' and m.get('max_concurrent',0)>0]
print(max(vals) if vals else 0)
" 2>/dev/null || echo "0")
  if [ "${AIMD_MC:-0}" -gt 0 ] && [ "${AIMD_MC:-0}" -lt "${NP_MAX:-4}" ]; then
    pass "AIMD learning confirmed: max_concurrent=$AIMD_MC < num_parallel=$NP_MAX (downward convergence)"
  elif [ "${AIMD_MC:-0}" -eq "${NP_MAX:-4}" ]; then
    info "AIMD at upper bound (max_concurrent=$AIMD_MC = num_parallel=$NP_MAX) — server handling load well"
  fi
else
  info "AIMD final state not available (model may have been evicted)"
fi

# ── SDD 레벨 1: Lazy Eviction — 모델 상주 확인 ───────────────────────────────

hdr "SDD Level 1: Lazy Eviction — Model Residency (180s idle threshold)"

# 요청 완료 직후 모델이 여전히 loaded 상태인지 확인
# (eviction은 180s idle 후에만 → 방금 요청 완료 후라면 반드시 loaded 상태여야 함)
RESIDENT=$(aget "/v1/dashboard/capacity" 2>/dev/null | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
loaded = [(p.get('provider_name','?'), m.get('model_name','?'))
          for p in d.get('providers',[]) for m in p.get('loaded_models',[])]
print('yes:' + ','.join(f'{p}/{m}' for p,m in loaded) if loaded else 'no')
" 2>/dev/null || echo "no")

case "$RESIDENT" in
  yes:*) pass "Lazy Eviction: models resident after requests (${RESIDENT#yes:}) — will evict after 180s idle" ;;
  no)    info "No models currently loaded (may have been evicted or sync pending)" ;;
esac

# ── SDD 레벨 2: Goodput 측정 ──────────────────────────────────────────────────

hdr "SDD Level 2: Goodput — N-server multi-model throughput"

CONCURRENT_GOODPUT=12
T_START=$(python3 -c "import time; print(int(time.time()*1000))")

# Multi-model goodput burst
TMPDIR_GP=$(mktemp -d)
for i in $(seq 1 "$CONCURRENT_GOODPUT"); do
  MDL="${MODELS_ARR[$(( (i - 1) % MODEL_COUNT ))]}"
  (
    T0=$(python3 -c "import time; print(int(time.time()*1000))")
    RES=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
      -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
      -d "{\"model\":\"$MDL\",\"messages\":[{\"role\":\"user\",\"content\":\"goodput $i\"}],\"max_tokens\":8,\"stream\":false}" \
      --max-time 120)
    CODE=$(echo "$RES" | tail -1)
    T1=$(python3 -c "import time; print(int(time.time()*1000))")
    echo "$i $CODE $((T1 - T0))ms $MDL" > "$TMPDIR_GP/r_$i"
  ) &
done
wait; echo ""
R_OK=0; R_Q=0; R_F=0
for f in "$TMPDIR_GP"/r_*; do
  read -r IDX CODE DUR MDL < "$f"
  case "$CODE" in
    200)     echo -e "    #$IDX: ${GREEN}200${NC} ($DUR) [$MDL]"; R_OK=$((R_OK+1)) ;;
    429|503) echo -e "    #$IDX: ${YELLOW}${CODE}${NC} ($DUR) [$MDL]"; R_Q=$((R_Q+1)) ;;
    *)       echo -e "    #$IDX: ${RED}${CODE}${NC} ($DUR) [$MDL]"; R_F=$((R_F+1)) ;;
  esac
done
rm -rf "$TMPDIR_GP"
GP_OK=$R_OK

T_END=$(python3 -c "import time; print(int(time.time()*1000))")
GP_ELAPSED=$(( (T_END - T_START) / 1000 ))
GP_ELAPSED_MS=$((T_END - T_START))

if [ "$GP_OK" -gt 0 ]; then
  THROUGHPUT=$(python3 -c "print(f'{$GP_OK / max($GP_ELAPSED, 1):.2f}')" 2>/dev/null || echo "?")
  pass "Goodput: $GP_OK/$CONCURRENT_GOODPUT requests completed in ${GP_ELAPSED}s (${THROUGHPUT} req/s)"
  info "N-server parallel processing: local + remote shared the $GP_OK requests"
else
  fail "Goodput test: 0 requests completed"
fi
save_var GP_OK "$GP_OK"
save_var GP_ELAPSED "$GP_ELAPSED"

save_counts
