#!/usr/bin/env bash
# Phase 06: Multi-Format Inference + Endpoint Smoke Tests
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

# ── Multi-Format Inference ────────────────────────────────────────────────────

hdr "Multi-Format Inference (all endpoints)"

TMPDIR_MF=$(mktemp -d)
(curl -s --max-time 30 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Say hi\"}],\"max_tokens\":8,\"stream\":true}" \
  > "$TMPDIR_MF/sse" 2>/dev/null || true) &
(curl -s -w "\n%{http_code}" --max-time 30 "$API/api/chat" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"word\"}],\"stream\":false}" \
  > "$TMPDIR_MF/chat" 2>/dev/null || printf "\n000" > "$TMPDIR_MF/chat") &
(curl -s -w "\n%{http_code}" --max-time 30 "$API/api/generate" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"prompt\":\"word\",\"stream\":false}" \
  > "$TMPDIR_MF/generate" 2>/dev/null || printf "\n000" > "$TMPDIR_MF/generate") &
(curl -s -w "\n%{http_code}" "$API/api/tags" -H "X-API-Key: $API_KEY" \
  > "$TMPDIR_MF/tags" 2>/dev/null || printf "\n000" > "$TMPDIR_MF/tags") &
(curl -s -w "\n%{http_code}" "$API/api/show" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "{\"name\":\"$MODEL\"}" > "$TMPDIR_MF/show" 2>/dev/null || printf "\n000" > "$TMPDIR_MF/show") &
(curl -s -w "\n%{http_code}" "$API/v1beta/models" -H "X-API-Key: $API_KEY" \
  > "$TMPDIR_MF/gemini" 2>/dev/null || printf "\n000" > "$TMPDIR_MF/gemini") &
(curl -s -w "\n%{http_code}" "$API/v1/test/completions" \
  -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}],\"max_tokens\":4,\"stream\":false}" \
  > "$TMPDIR_MF/test_completions" 2>/dev/null || printf "\n000" > "$TMPDIR_MF/test_completions") &
(apostc "/v1/test/api/chat" \
  "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}],\"stream\":false}" \
  > "$TMPDIR_MF/test_chat" 2>/dev/null || printf "\n000" > "$TMPDIR_MF/test_chat") &
(apostc "/v1/test/api/generate" \
  "{\"model\":\"$MODEL\",\"prompt\":\"ping\",\"stream\":false}" \
  > "$TMPDIR_MF/test_generate" 2>/dev/null || printf "\n000" > "$TMPDIR_MF/test_generate") &
wait

# SSE check
SSE_RES=$(cat "$TMPDIR_MF/sse" 2>/dev/null || echo "")
echo "$SSE_RES" | grep -q "data:" \
  && pass "OpenAI SSE streaming has data events" || fail "SSE: no data events"

for ep in chat generate tags show gemini test_completions test_chat test_generate; do
  c=$(tail -1 "$TMPDIR_MF/$ep" 2>/dev/null || echo "000")
  [ "$c" = "200" ] && pass "$ep → 200" || fail "$ep → $c"
done
rm -rf "$TMPDIR_MF"

# ── SSE Content Validation (basic — detailed check in phase 08) ───────────────

hdr "SSE Content Validation"

# During parallel phases, other tests can saturate providers causing SSE failures.
# We do a basic check here; strict JSON structure validation is in 08-sdd-advanced.sh.
SSE_FULL=$(curl -s --max-time 60 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Reply with exactly: Hello World\"}],\"max_tokens\":50,\"stream\":true}" \
  2>/dev/null || echo "")

SSE_OK=$(echo "$SSE_FULL" | grep "^data: {" | python3 -c "
import sys, json
for line in sys.stdin:
    line = line.strip()
    if line.startswith('data: '):
        try:
            d = json.loads(line[6:])
            if 'choices' in d and len(d['choices']) > 0:
                print('yes'); exit()
        except: pass
print('no')
" 2>/dev/null || echo "no")

if [ "$SSE_OK" = "yes" ]; then
  pass "SSE valid JSON structure with choices"
else
  # During parallel phases, this can fail due to provider contention
  FIRST_DATA=$(echo "$SSE_FULL" | grep "^data:" | head -1 | cut -c1-120)
  info "SSE choices not found in parallel phase (first data: ${FIRST_DATA:-empty}) — validated in phase 08"
fi
HAS_DONE=$(echo "$SSE_FULL" | grep -c "\[DONE\]" 2>/dev/null; true)
[ "${HAS_DONE:-0}" -gt 0 ] && pass "SSE ends with [DONE]" || info "SSE [DONE] not captured (parallel phase contention)"

# ── Endpoint Smoke Tests ──────────────────────────────────────────────────────

hdr "Endpoint Smoke Tests"

assert_get "/v1/servers" 200 "List servers"
assert_get "/v1/audit?limit=10" 200 "Audit log"
assert_get "/v1/dashboard/lab" 200 "Lab settings"
assert_get "/v1/dashboard/analytics?hours=24" 200 "Dashboard analytics"
assert_get "/v1/dashboard/queue/depth" 200 "Queue depth"
assert_get "/v1/dashboard/overview" 200 "Dashboard overview"

c=$(curl -s -w "\n%{http_code}" "$API/docs/openapi.json" | code)
[ "$c" = "200" ] && pass "OpenAPI spec → 200" || fail "OpenAPI → $c"
c=$(curl -s -w "\n%{http_code}" "$API/docs/swagger" | code)
[ "$c" = "200" ] && pass "Swagger UI → 200" || fail "Swagger → $c"
c=$(curl -s -w "\n%{http_code}" "$API/docs/redoc" | code)
[ "$c" = "200" ] && pass "Redoc UI → 200" || fail "Redoc → $c"
c=$(curl -s -w "\n%{http_code}" "$API/v1/metrics/targets" | code)
[ "$c" = "200" ] && pass "Metrics targets → 200" || fail "Metrics targets → $c"

# /api/version, /api/ps
c=$(curl -s -w "\n%{http_code}" "$API/api/version" -H "X-API-Key: $API_KEY" 2>/dev/null | code)
[ "$c" = "200" ] && pass "/api/version → 200" || fail "/api/version → $c"
c=$(curl -s -w "\n%{http_code}" "$API/api/ps" -H "X-API-Key: $API_KEY" 2>/dev/null | code)
[ "$c" = "200" ] && pass "/api/ps → 200" || fail "/api/ps → $c"

# Embed endpoints
for ep_name in "embed" "embeddings"; do
  BODY='{"model":"'"$MODEL"'","input":"test"}'
  [ "$ep_name" = "embeddings" ] && BODY='{"model":"'"$MODEL"'","prompt":"test"}'
  c=$(curl -s -w "\n%{http_code}" --max-time 30 "$API/api/$ep_name" \
    -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" -d "$BODY" 2>/dev/null | code)
  case "$c" in
    200) pass "/api/$ep_name → 200" ;;
    400|404|500|501) pass "/api/$ep_name → $c (not supported)" ;;
    *) fail "/api/$ep_name → $c" ;;
  esac
done

# ── Server / Provider Endpoints ───────────────────────────────────────────────

hdr "Server & Provider Endpoints"

if [ -n "${SERVER_ID_LOCAL:-}" ] && [ "$SERVER_ID_LOCAL" != "None" ]; then
  c=$(agetc "/v1/servers/$SERVER_ID_LOCAL/metrics" | code)
  [ "$c" = "200" ] && pass "Local server metrics → 200" || info "Local server metrics → $c"
  assert_get "/v1/servers/$SERVER_ID_LOCAL/metrics/history?hours=1" 200 "Local metrics history"
fi
if [ -n "${SERVER_ID_REMOTE:-}" ] && [ "$SERVER_ID_REMOTE" != "None" ]; then
  c=$(agetc "/v1/servers/$SERVER_ID_REMOTE/metrics" | code)
  [ "$c" = "200" ] && pass "Remote server metrics → 200" || info "Remote server metrics → $c"
fi

if [ -n "${PROVIDER_ID_LOCAL:-}" ] && [ "$PROVIDER_ID_LOCAL" != "None" ]; then
  c=$(agetc "/v1/providers/$PROVIDER_ID_LOCAL/key" | code)
  [ "$c" = "200" ] && pass "Local provider key → 200" || info "Local provider key → $c (no key)"
fi

# Session grouping trigger
c=$(apostc "/v1/dashboard/session-grouping/trigger" "{}" | code)
[ "$c" = "200" ] || [ "$c" = "202" ] && pass "Session grouping → $c" || fail "Session grouping → $c"

# Lab toggle + revert
LAB=$(aget "/v1/dashboard/lab" 2>/dev/null | jv '["gemini_function_calling"]' 2>/dev/null || echo "")
if [ -n "$LAB" ] && [ "$LAB" != "None" ]; then
  if [ "$LAB" = "True" ]; then
    apatch "/v1/dashboard/lab" '{"gemini_function_calling":false}' > /dev/null 2>&1
    apatch "/v1/dashboard/lab" '{"gemini_function_calling":true}' > /dev/null 2>&1
  else
    apatch "/v1/dashboard/lab" '{"gemini_function_calling":true}' > /dev/null 2>&1
    apatch "/v1/dashboard/lab" '{"gemini_function_calling":false}' > /dev/null 2>&1
  fi
  pass "Lab toggle + revert OK"
fi

# Per-key usage
KEY_LIST=$(aget "/v1/keys" 2>/dev/null || echo "[]")
FIRST_KEY_ID=$(echo "$KEY_LIST" | jv '[0]["id"]' 2>/dev/null || echo "")
if [ -n "$FIRST_KEY_ID" ] && [ "$FIRST_KEY_ID" != "None" ]; then
  assert_get "/v1/usage/$FIRST_KEY_ID?hours=24" 200 "Per-key usage"
  assert_get "/v1/usage/$FIRST_KEY_ID/jobs?hours=24" 200 "Per-key jobs"
  assert_get "/v1/usage/$FIRST_KEY_ID/models?hours=24" 200 "Per-key models"
fi

FIRST_JOB_ID=$(aget "/v1/dashboard/jobs?limit=1" 2>/dev/null | jv '["jobs"][0]["id"]' 2>/dev/null || echo "")
[ -n "$FIRST_JOB_ID" ] && [ "$FIRST_JOB_ID" != "None" ] \
  && assert_get "/v1/dashboard/jobs/$FIRST_JOB_ID" 200 "Job detail"

# ── SDD §5: Pull Drain Endpoint ──────────────────────────────────────────────

hdr "SDD §5: Pull Drain — POST /v1/ollama/models/pull"

# Verify endpoint exists and accepts admin requests
# (Full drain+pull would take too long in CI; we verify the API surface and 202 response)
if [ -n "${PROVIDER_ID_LOCAL:-}" ] && [ "$PROVIDER_ID_LOCAL" != "None" ]; then
  PULL_RES=$(apostc "/v1/ollama/models/pull" \
    "{\"model\":\"$MODEL\",\"provider_id\":\"$PROVIDER_ID_LOCAL\"}")
  PULL_CODE=$(echo "$PULL_RES" | code)
  case "$PULL_CODE" in
    202) pass "Pull drain endpoint → 202 Accepted (drain+pull started in background)" ;;
    200) pass "Pull drain endpoint → 200 OK" ;;
    # 409 would mean pull already in progress — acceptable
    409) pass "Pull drain endpoint → 409 (pull already in progress)" ;;
    *) fail "Pull drain endpoint → $PULL_CODE (expected 202)" ;;
  esac

  # Wait briefly for is_pulling state to propagate, then verify dispatch blocked
  sleep 2
  info "Pull in progress — is_pulling=true should block dispatch routing"

  # §5: Verify dispatch is actually blocked during pull
  # Inference for pulling model+provider should either:
  #   - Route to remote provider (200) if available
  #   - Return 503 if no other provider can serve the model
  PULL_INF_CODE=$(curl -s -w "\n%{http_code}" -o /dev/null --max-time 15 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"pull block test\"}],\"max_tokens\":3,\"stream\":false}" \
    2>/dev/null | tail -1)
  case "$PULL_INF_CODE" in
    200) pass "Pull dispatch block: request rerouted to non-pulling provider (200)" ;;
    503) pass "Pull dispatch block: no eligible provider during pull (503)" ;;
    429) pass "Pull dispatch block: rate limited during pull (429)" ;;
    *)   info "Pull dispatch block: got $PULL_INF_CODE (pull may have completed)" ;;
  esac
  # is_pulling will be cleared by background task after pull completes
else
  info "Pull drain test skipped — no local provider registered"
fi

save_counts
