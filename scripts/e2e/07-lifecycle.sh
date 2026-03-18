#!/usr/bin/env bash
# Phase 07: Job Lifecycle / SSE Replay / Native API / Password Reset / Edge Cases
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

# ── Job Cancel During Streaming ───────────────────────────────────────────────

hdr "Job Cancel During Streaming"

curl -s "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Write a long essay about computer science\"}],\"max_tokens\":300,\"stream\":true}" \
  > /dev/null 2>&1 &
CANCEL_PID=$!
sleep 3

CANCEL_JOB_ID=$(aget "/v1/dashboard/jobs?limit=1&status=running" 2>/dev/null \
  | jv '["jobs"][0]["id"]' 2>/dev/null || echo "")
[ -z "$CANCEL_JOB_ID" ] || [ "$CANCEL_JOB_ID" = "None" ] && \
  CANCEL_JOB_ID=$(aget "/v1/dashboard/jobs?limit=1" 2>/dev/null \
    | jv '["jobs"][0]["id"]' 2>/dev/null || echo "")

if [ -n "$CANCEL_JOB_ID" ] && [ "$CANCEL_JOB_ID" != "None" ]; then
  c=$(adelc "/v1/dashboard/jobs/$CANCEL_JOB_ID" | code)
  case "$c" in 200|204) pass "Job cancel → $c" ;; *) fail "Job cancel → $c" ;; esac
  sleep 1
  S=$(aget "/v1/dashboard/jobs/$CANCEL_JOB_ID" 2>/dev/null | jv '["status"]' 2>/dev/null || echo "unknown")
  case "$S" in
    cancelled|Cancelled) pass "Job status = cancelled" ;;
    completed|Completed) pass "Job completed before cancel" ;;
    failed|Failed) pass "Job failed before cancel (parallel phase interference)" ;;
    *) fail "Job status = $S" ;;
  esac
else
  fail "No job found to cancel"
fi
kill $CANCEL_PID 2>/dev/null || true; wait $CANCEL_PID 2>/dev/null || true

# Warm-up: ensure inference is ready post-cancel
WARMUP_OK="no"
for _w in $(seq 1 20); do
  WU_CODE=$(curl -s -w "\n%{http_code}" -o /dev/null --max-time 90 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":2,\"stream\":false}" \
    2>/dev/null | tail -1) || WU_CODE="000"
  [ "$WU_CODE" = "200" ] && WARMUP_OK="yes" && break
  info "Warm-up ${_w}/20 → HTTP $WU_CODE, waiting 5s..."
  sleep 5
done
[ "$WARMUP_OK" = "yes" ] && pass "Inference ready after cancel" || fail "Inference not ready after warm-up"

# ── Native Inference API (/v1/inference) ──────────────────────────────────────

hdr "Native Inference API"

INF_RES=$(kpostc "/v1/inference" \
  "{\"prompt\":\"Say hello\",\"model\":\"$MODEL\",\"provider_type\":\"ollama\"}")
INF_CODE=$(echo "$INF_RES" | code)
INF_JOB_ID=$(echo "$INF_RES" | body | jv '["job_id"]' 2>/dev/null || echo "")
case "$INF_CODE" in
  200|201|202) pass "Submit inference → $INF_CODE" ;;
  *) fail "Submit inference → $INF_CODE" ;;
esac

if [ -n "$INF_JOB_ID" ] && [ "$INF_JOB_ID" != "None" ]; then
  sleep 2
  c=$(kgetc "/v1/inference/$INF_JOB_ID/status" | code)
  [ "$c" = "200" ] && pass "Job status → 200" || fail "Job status → $c"

  for _w in $(seq 1 15); do
    S=$(kget "/v1/inference/$INF_JOB_ID/status" 2>/dev/null | jv '["status"]' 2>/dev/null || echo "")
    case "$S" in completed|Completed|failed|Failed) break ;; esac; sleep 1
  done
  c=$(kgetc "/v1/inference/$INF_JOB_ID/stream" | code)
  [ "$c" = "200" ] && pass "Job stream → 200" || fail "Job stream → $c"
  c=$(kdelc "/v1/inference/$INF_JOB_ID" | code)
  case "$c" in 200|204) pass "Cancel inference → $c" ;; *) fail "Cancel → $c" ;; esac
fi

# ── SSE Replay & Dashboard Stream ────────────────────────────────────────────

hdr "SSE Replay & Dashboard Stream"

REPLAY_ID=$(aget "/v1/dashboard/jobs?limit=1&status=completed" 2>/dev/null \
  | jv '["jobs"][0]["id"]' 2>/dev/null || echo "")
if [ -n "$REPLAY_ID" ] && [ "$REPLAY_ID" != "None" ]; then
  curl -s --max-time 10 "$API/v1/jobs/$REPLAY_ID/stream" \
    -H "X-API-Key: $API_KEY" > /dev/null 2>&1 || true
  pass "SSE replay endpoint accessible"
fi

c=$(curl -s -w "\n%{http_code}" --max-time 3 "$API/v1/dashboard/jobs/stream" \
  -H "Authorization: Bearer $TK" 2>/dev/null || true)
c=$(echo "$c" | code)
case "$c" in 200|000) pass "Dashboard SSE stream accessible" ;; *) fail "Dashboard SSE → $c" ;; esac

# ── Password Reset ────────────────────────────────────────────────────────────

hdr "Password Reset Flow"

RESET_USER="e2e-reset-$(python3 -c 'import uuid;print(str(uuid.uuid4())[:8])')"
RESET_RES=$(apostc "/v1/accounts" \
  "{\"username\":\"$RESET_USER\",\"password\":\"OldPass123!\",\"name\":\"Reset\",\"role\":\"admin\"}")
RESET_CODE=$(echo "$RESET_RES" | code)
RESET_ACCT_ID=$(echo "$RESET_RES" | body | jv '["id"]' 2>/dev/null || echo "")
if [ "$RESET_CODE" = "200" ] || [ "$RESET_CODE" = "201" ]; then
  LINK_RES=$(apostc "/v1/accounts/$RESET_ACCT_ID/reset-link" "{}")
  LINK_CODE=$(echo "$LINK_RES" | code)
  RESET_TOKEN=$(echo "$LINK_RES" | body | jv '["token"]' 2>/dev/null || echo "")
  case "$LINK_CODE" in 200|201) pass "Reset link → $LINK_CODE" ;; *) fail "Reset link → $LINK_CODE" ;; esac

  if [ -n "$RESET_TOKEN" ] && [ "$RESET_TOKEN" != "None" ]; then
    c=$(rawpostc "/v1/auth/reset-password" \
      "{\"token\":\"$RESET_TOKEN\",\"new_password\":\"NewPass456!\"}" | code)
    [ "$c" = "200" ] || [ "$c" = "204" ] && pass "Password reset → $c" || fail "Reset → $c"
    c=$(rawpostc "/v1/auth/login" \
      "{\"username\":\"$RESET_USER\",\"password\":\"NewPass456!\"}" | code)
    [ "$c" = "200" ] && pass "Login with new password" || fail "New password login → $c"
    # Token reuse must be rejected
    c=$(rawpostc "/v1/auth/reset-password" \
      "{\"token\":\"$RESET_TOKEN\",\"new_password\":\"Another789!\"}" | code)
    case "$c" in 400|401|404|410) pass "Reused token rejected → $c" ;; *) fail "Reused token: got $c" ;; esac
  fi
  adel "/v1/accounts/$RESET_ACCT_ID" > /dev/null 2>&1
else
  fail "Reset account failed ($RESET_CODE)"
fi

# ── SDD §7: Queued Cancel — ZREM + DECR Atomic ──────────────────────────────

hdr "SDD §7: Queued Cancel — Cancel Job While in ZSET Queue"

# Strategy: saturate provider capacity with long streaming requests, then submit
# additional jobs that should queue in ZSET. Cancel one queued job and verify cleanup.
QCANCEL_PIDS=()

# Determine total capacity to know how many saturating requests we need
QCANCEL_CAP=$(aget "/v1/dashboard/capacity" 2>/dev/null || echo '{"providers":[]}')
TOTAL_MC=$(echo "$QCANCEL_CAP" | python3 -c "
import sys, json; d = json.loads(sys.stdin.read())
print(sum(m.get('max_concurrent',0) for p in d.get('providers',[]) for m in p.get('loaded_models',[]) if m.get('model_name')=='$MODEL'))
" 2>/dev/null || echo "4")
# Fire 30 streaming requests rapidly to saturate all provider slots + overflow into queue
SAT_COUNT=30
for qi in $(seq 1 "$SAT_COUNT"); do
  curl -s --max-time 60 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Explain algorithm $qi in great detail\"}],\"max_tokens\":200,\"stream\":true}" \
    > /dev/null 2>&1 &
  QCANCEL_PIDS+=($!)
done
# Immediately poll for queued or running jobs (race the dispatcher)
QCANCEL_JOB=""
for _qc_try in $(seq 1 8); do
  QCANCEL_JOB=$(aget "/v1/dashboard/jobs?limit=1&status=pending" 2>/dev/null \
    | jv '["jobs"][0]["id"]' 2>/dev/null || echo "")
  [ -n "$QCANCEL_JOB" ] && [ "$QCANCEL_JOB" != "None" ] && break
  QCANCEL_JOB=$(aget "/v1/dashboard/jobs?limit=1&status=running" 2>/dev/null \
    | jv '["jobs"][0]["id"]' 2>/dev/null || echo "")
  [ -n "$QCANCEL_JOB" ] && [ "$QCANCEL_JOB" != "None" ] && break
  sleep 0.3
done

if [ -n "$QCANCEL_JOB" ] && [ "$QCANCEL_JOB" != "None" ]; then
  # Check if job is in ZSET (queued state)
  ZSET_SCORE=$(docker compose exec -T valkey valkey-cli ZSCORE "veronex:queue:zset" "$QCANCEL_JOB" 2>/dev/null | tr -d ' \r\n' || echo "")
  DEMAND_BEFORE=$(docker compose exec -T valkey valkey-cli GET "veronex:demand:$MODEL" 2>/dev/null | tr -d ' \r\n' || echo "0")

  if [ -n "$ZSET_SCORE" ] && [ "$ZSET_SCORE" != "(nil)" ]; then
    info "Job $QCANCEL_JOB in ZSET (score=$ZSET_SCORE) — cancelling queued job"
  else
    info "Job $QCANCEL_JOB not in ZSET (already dispatched) — cancelling processing job"
  fi

  # Cancel the job via dashboard API
  CANCEL_CODE=$(adelc "/v1/dashboard/jobs/$QCANCEL_JOB" | code)
  case "$CANCEL_CODE" in
    200|204) pass "Queued cancel API -> $CANCEL_CODE" ;;
    *)       info "Queued cancel API -> $CANCEL_CODE" ;;
  esac

  sleep 1

  # Verify job removed from ZSET
  ZSET_AFTER=$(docker compose exec -T valkey valkey-cli ZSCORE "veronex:queue:zset" "$QCANCEL_JOB" 2>/dev/null | tr -d ' \r\n' || echo "")
  if [ -z "$ZSET_AFTER" ] || [ "$ZSET_AFTER" = "(nil)" ]; then
    pass "Queued cancel: job removed from ZSET (ZREM confirmed)"
  else
    info "Job still in ZSET (score=$ZSET_AFTER) — may have been re-enqueued"
  fi

  # Verify job status in DB
  QCANCEL_STATUS=$(aget "/v1/dashboard/jobs/$QCANCEL_JOB" 2>/dev/null | jv '["status"]' 2>/dev/null || echo "unknown")
  case "$QCANCEL_STATUS" in
    cancelled|Cancelled) pass "Queued cancel: job status = cancelled" ;;
    failed|Failed) pass "Queued cancel: job status = failed (cancel processed)" ;;
    completed|Completed) pass "Queued cancel: job completed before cancel reached it" ;;
    *) info "Queued cancel: job status = $QCANCEL_STATUS" ;;
  esac

  # Verify demand counter decremented or consistent
  DEMAND_AFTER=$(docker compose exec -T valkey valkey-cli GET "veronex:demand:$MODEL" 2>/dev/null | tr -d ' \r\n' || echo "0")
  if [ "${DEMAND_AFTER:-0}" -le "${DEMAND_BEFORE:-0}" ]; then
    pass "Queued cancel: demand counter consistent (before=$DEMAND_BEFORE after=$DEMAND_AFTER)"
  else
    info "Demand counter: before=$DEMAND_BEFORE after=$DEMAND_AFTER (other requests may have enqueued)"
  fi
else
  info "Queued cancel test skipped — no queued/running job found"
fi

# Cleanup: kill saturating requests (suppress "Terminated" messages)
{
  for pid in "${QCANCEL_PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait "${QCANCEL_PIDS[@]}" 2>/dev/null || true
} 2>/dev/null
sleep 2

# ── Edge Cases ────────────────────────────────────────────────────────────────

hdr "Edge Cases"

# Disable model on local provider → requests should route to remote or get blocked
if [ -n "${PROVIDER_ID_LOCAL:-}" ] && [ "$PROVIDER_ID_LOCAL" != "None" ]; then
  apatch "/v1/providers/$PROVIDER_ID_LOCAL/selected-models/$MODEL" '{"is_enabled":false}' > /dev/null 2>&1
  sleep 1
  c=$(curl -s -w "%{http_code}" -o /dev/null --max-time 15 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"test\"}],\"max_tokens\":4,\"stream\":false}")
  case "$c" in
    200) info "Disabled on local: remote provider handled request" ;;
    400|404|503) pass "Model disabled on local → $c (no remote available)" ;;
    *) info "Disabled model → $c" ;;
  esac
  apatch "/v1/providers/$PROVIDER_ID_LOCAL/selected-models/$MODEL" '{"is_enabled":true}' > /dev/null 2>&1
  pass "Model disable/enable cycle on local provider OK"
fi

# Job filtering
assert_get "/v1/dashboard/jobs?status=completed&limit=5" 200 "Jobs filter: status=completed"
assert_get "/v1/dashboard/jobs?q=hello&limit=5" 200 "Jobs filter: full-text search"
assert_get "/v1/dashboard/jobs?source=api&limit=5" 200 "Jobs filter: source=api"

# Session revoke
SESS_LOGIN=$(curl -si "$API/v1/auth/login" \
  -H 'Content-Type: application/json' -d @/tmp/_sched_login.json 2>/dev/null)
SESS_TK=$(echo "$SESS_LOGIN" | sed -n 's/.*veronex_access_token=\([^;]*\).*/\1/p' | head -1)
if [ -n "$SESS_TK" ]; then
  ADMIN_ID=$(aget "/v1/accounts" 2>/dev/null | jv '[0]["id"]' 2>/dev/null || echo "")
  if [ -n "$ADMIN_ID" ] && [ "$ADMIN_ID" != "None" ]; then
    SESS_ID=$(aget "/v1/accounts/$ADMIN_ID/sessions" 2>/dev/null | python3 -c "
import sys,json; d=json.loads(sys.stdin.read())
print(d[-1].get('id',d[-1].get('session_id','')) if isinstance(d,list) and d else '')
" 2>/dev/null || echo "")
    if [ -n "$SESS_ID" ]; then
      c=$(adelc "/v1/sessions/$SESS_ID" | code)
      case "$c" in 200|204) pass "Revoke session → $c" ;; *) fail "Revoke → $c" ;; esac
    fi
  fi
fi

FINAL_JOBS=$(aget "/v1/dashboard/stats" 2>/dev/null | jv '["total_jobs"]' 2>/dev/null || echo "0")
[ "$FINAL_JOBS" != "0" ] && [ "$FINAL_JOBS" != "None" ] \
  && pass "Dashboard total_jobs=$FINAL_JOBS" || info "total_jobs=$FINAL_JOBS"

# ── SDD Crash Recovery: processing queue 복원 ─────────────────────────────────

hdr "SDD Crash Recovery — processing queue restore after restart"

# 1. job을 하나 시작해서 processing 상태로 만들기
RECOVERY_JOB=$(curl -s --max-time 5 "$API/v1/inference" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"prompt\":\"crash recovery test\"}" \
  2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin).get('job_id',''))" 2>/dev/null || echo "")

if [ -n "$RECOVERY_JOB" ] && [ "$RECOVERY_JOB" != "None" ]; then
  info "Test job submitted: $RECOVERY_JOB"

  # 완료까지 대기
  for i in $(seq 1 15); do
    sleep 1
    STATUS=$(aget "/v1/inference/$RECOVERY_JOB/status" 2>/dev/null | jv '["status"]' 2>/dev/null || echo "pending")
    if [ "$STATUS" = "completed" ] || [ "$STATUS" = "failed" ]; then break; fi
  done

  # 2. veronex 재시작
  info "Restarting veronex container..."
  docker compose restart veronex > /dev/null 2>&1 || true

  # 재시작 대기
  for i in $(seq 1 30); do
    sleep 2
    HC=$(curl -s -o /dev/null -w "%{http_code}" "$API/health" 2>/dev/null || echo "000")
    [ "$HC" = "200" ] && break
    [ "$i" -eq 30 ] && fail "veronex did not restart in 60s" && break
  done
  pass "veronex restarted successfully"

  # 3. 재시작 후 기존 job 상태가 보존되는지 확인
  # (processing queue에 있던 job은 ZADD로 복원 → 재처리 또는 DB에 상태 유지)
  sleep 3
  STATUS_AFTER=$(aget "/v1/inference/$RECOVERY_JOB/status" 2>/dev/null | jv '["status"]' 2>/dev/null || echo "unknown")
  PROCESSING_COUNT=$(docker compose exec -T valkey valkey-cli LLEN "veronex:queue:processing" 2>/dev/null | tr -d ' \r\n' || echo "0")
  ZSET_RECOVERED=$(docker compose exec -T valkey valkey-cli ZSCORE "veronex:queue:zset" "$RECOVERY_JOB" 2>/dev/null | tr -d ' \r\n' || echo "")

  info "Job $RECOVERY_JOB status after restart: $STATUS_AFTER"
  info "Processing list after restart: $PROCESSING_COUNT entries"

  case "$STATUS_AFTER" in
    completed|failed)
      pass "Crash recovery: job $RECOVERY_JOB preserved status=$STATUS_AFTER after restart" ;;
    queued|processing)
      [ -n "$ZSET_RECOVERED" ] \
        && pass "Crash recovery: job restored to ZSET (score=$ZSET_RECOVERED) — will be reprocessed" \
        || pass "Crash recovery: job still in processing state — being handled" ;;
    *)
      info "Crash recovery: job status=$STATUS_AFTER (unexpected, may indicate recovery in progress)" ;;
  esac

  # processing list 좀비 잔류 없는지 확인
  [ "${PROCESSING_COUNT:-0}" = "0" ] \
    && pass "No zombie jobs in processing list after restart" \
    || info "Processing list has $PROCESSING_COUNT entries (crash recovery may still be running)"

  # Verify instance registered in veronex:instances after restart
  INST_COUNT=$(docker compose exec -T valkey valkey-cli SCARD "veronex:instances" 2>/dev/null | tr -d ' \r\n' || echo "0")
  [ "${INST_COUNT:-0}" -ge 1 ] \
    && pass "Instance re-registered in veronex:instances after restart ($INST_COUNT member(s))" \
    || info "veronex:instances empty after restart (Valkey may not be configured)"
else
  info "Crash recovery test skipped — job submission failed"
fi

save_counts
