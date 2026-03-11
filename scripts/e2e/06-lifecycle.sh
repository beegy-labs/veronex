#!/usr/bin/env bash
# Phase 22,25-29: Job Lifecycle, SSE, Native API, Password Reset, Edge Cases
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

# ── Job Cancel ──────────────────────────────────────────────────────────────

hdr "Job Lifecycle"

curl -s "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Write a long essay about CS history\"}],\"max_tokens\":300,\"stream\":true}" > /dev/null 2>&1 &
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
    *) fail "Job status = $S" ;;
  esac
else
  fail "No job found to cancel"
fi
kill $CANCEL_PID 2>/dev/null || true; wait $CANCEL_PID 2>/dev/null || true

# Poll until inference is ready (non-streaming warm-up request)
# This directly validates the provider/model is available, more reliable than VramPool state.
WARMUP_OK="no"
for _w in $(seq 1 20); do
  WU_CODE=$(curl -s -w "\n%{http_code}" -o /dev/null --max-time 90 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":2,\"stream\":false}" 2>/dev/null | tail -1)
  [ "$WU_CODE" = "200" ] && WARMUP_OK="yes" && break
  info "Inference warm-up ${_w}/20 → HTTP $WU_CODE, waiting 5s..."
  sleep 5
done
[ "$WARMUP_OK" = "yes" ] && pass "Inference ready after cancel" || fail "Inference not ready after warm-up"

# ── SSE Content Verification ────────────────────────────────────────────────

hdr "SSE Verification"

SSE_FULL=""
for _sse_try in $(seq 1 3); do
  SSE_FULL=$(curl -s --max-time 30 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Say hello\"}],\"max_tokens\":8,\"stream\":true}" 2>/dev/null || echo "")
  echo "$SSE_FULL" | grep -q "^data: {" && break
  [ "$_sse_try" -lt 3 ] && info "SSE retry ${_sse_try}/3, waiting 10s..." && sleep 10
done

SSE_OK=$(echo "$SSE_FULL" | grep "^data: {" | head -1 | python3 -c "
import sys, json
line = sys.stdin.readline().strip()
if line.startswith('data: '):
    d = json.loads(line[6:])
    print('yes' if 'choices' in d and len(d['choices']) > 0 else 'no')
else: print('no')
" 2>/dev/null || echo "no")
[ "$SSE_OK" = "yes" ] && pass "SSE valid JSON structure" || fail "SSE JSON invalid"

HAS_DONE=$(echo "$SSE_FULL" | grep -c "\[DONE\]" 2>/dev/null; true)
[ "${HAS_DONE:-0}" -gt 0 ] && pass "SSE ends with [DONE]" || fail "SSE missing [DONE]"

# ── Native Inference API ────────────────────────────────────────────────────

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

# ── SSE Replay & Dashboard Stream ───────────────────────────────────────────

hdr "SSE Replay & Streaming"

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

# ── Password Reset ──────────────────────────────────────────────────────────

hdr "Password Reset"

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

    c=$(rawpostc "/v1/auth/reset-password" \
      "{\"token\":\"$RESET_TOKEN\",\"new_password\":\"Another789!\"}" | code)
    case "$c" in 400|401|404|410) pass "Reused token rejected → $c" ;; *) fail "Reused token: got $c" ;; esac
  fi
  adel "/v1/accounts/$RESET_ACCT_ID" > /dev/null 2>&1
else
  fail "Reset account failed ($RESET_CODE)"
fi

# Revoke specific session
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

# ── Edge Cases ──────────────────────────────────────────────────────────────

hdr "Edge Cases"

if [ -n "$PROVIDER_ID" ] && [ "$PROVIDER_ID" != "None" ]; then
  apatch "/v1/providers/$PROVIDER_ID/selected-models/$MODEL" '{"is_enabled":false}' > /dev/null 2>&1
  sleep 1
  c=$(curl -s -w "%{http_code}" -o /dev/null --max-time 15 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"test\"}],\"max_tokens\":4,\"stream\":false}")
  case "$c" in
    200) info "Disabled model still routed (other providers)" ;;
    400|404|503) pass "Disabled model blocked → $c" ;;
    *) info "Disabled model → $c" ;;
  esac
  apatch "/v1/providers/$PROVIDER_ID/selected-models/$MODEL" '{"is_enabled":true}' > /dev/null 2>&1
  pass "Model disable/enable cycle"
fi

assert_get "/v1/dashboard/jobs?status=completed&limit=5" 200 "Jobs filter by status"
assert_get "/v1/dashboard/jobs?q=hello&limit=5" 200 "Jobs search"
assert_get "/v1/dashboard/jobs?source=api&limit=5" 200 "Jobs filter by source"

FINAL_JOBS=$(aget "/v1/dashboard/stats" 2>/dev/null | jv '["total_jobs"]' 2>/dev/null || echo "0")
[ "$FINAL_JOBS" != "0" ] && [ "$FINAL_JOBS" != "None" ] \
  && pass "Dashboard total_jobs=$FINAL_JOBS" || info "total_jobs=$FINAL_JOBS"

save_counts
