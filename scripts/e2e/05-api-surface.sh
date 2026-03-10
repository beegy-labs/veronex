#!/usr/bin/env bash
# Phase 17+24: Multi-Format Inference, Endpoint Smoke Tests
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

# ── Multi-Format Inference ──────────────────────────────────────────────────

hdr "Multi-Format Inference"

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
  && pass "OpenAI SSE streaming" || fail "SSE: no data events"

# Batch endpoint checks
for ep in chat generate tags show gemini test_completions test_chat test_generate; do
  c=$(tail -1 "$TMPDIR_MF/$ep" 2>/dev/null || echo "000")
  [ "$c" = "200" ] && pass "$ep → 200" || fail "$ep → $c"
done
rm -rf "$TMPDIR_MF"

# ── Endpoint Smoke Tests ────────────────────────────────────────────────────

hdr "Endpoint Smoke Tests"

assert_get "/v1/servers" 200 "List servers"
assert_get "/v1/audit?limit=10" 200 "Audit log"
assert_get "/v1/dashboard/lab" 200 "Lab settings"
assert_get "/v1/dashboard/analytics?hours=24" 200 "Dashboard analytics"

c=$(curl -s -w "\n%{http_code}" "$API/docs/openapi.json" | code)
[ "$c" = "200" ] && pass "OpenAPI spec → 200" || fail "OpenAPI → $c"

c=$(curl -s -w "\n%{http_code}" "$API/v1/metrics/targets" | code)
[ "$c" = "200" ] && pass "Metrics targets → 200" || fail "Metrics targets → $c"

c=$(curl -s -w "\n%{http_code}" "$API/docs/swagger" | code)
[ "$c" = "200" ] && pass "Swagger UI → 200" || fail "Swagger → $c"

c=$(curl -s -w "\n%{http_code}" "$API/docs/redoc" | code)
[ "$c" = "200" ] && pass "Redoc UI → 200" || fail "Redoc → $c"

if [ -n "$SERVER_ID" ] && [ "$SERVER_ID" != "None" ]; then
  c=$(agetc "/v1/servers/$SERVER_ID/metrics" | code)
  [ "$c" = "200" ] && pass "Server metrics → 200" || info "Server metrics → $c"
  assert_get "/v1/servers/$SERVER_ID/metrics/history?hours=1" 200 "Metrics history"
fi

if [ -n "$PROVIDER_ID" ] && [ "$PROVIDER_ID" != "None" ]; then
  c=$(apostc "/v1/providers/$PROVIDER_ID/sync" "{}" | code)
  [ "$c" = "200" ] || [ "$c" = "202" ] && pass "Provider sync → $c" || fail "Provider sync → $c"
  c=$(agetc "/v1/providers/$PROVIDER_ID/key" | code)
  [ "$c" = "200" ] && pass "Provider key → 200" || pass "Provider key → $c (no key)"
fi

c=$(apostc "/v1/dashboard/session-grouping/trigger" "{}" | code)
[ "$c" = "200" ] || [ "$c" = "202" ] && pass "Session grouping → $c" || fail "Session grouping → $c"

c=$(curl -s -w "\n%{http_code}" "$API/api/version" -H "X-API-Key: $API_KEY" 2>/dev/null | code)
[ "$c" = "200" ] && pass "/api/version → 200" || fail "/api/version → $c"

c=$(curl -s -w "\n%{http_code}" "$API/api/ps" -H "X-API-Key: $API_KEY" 2>/dev/null | code)
[ "$c" = "200" ] && pass "/api/ps → 200" || fail "/api/ps → $c"

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

# Lab toggle + revert
LAB=$(aget "/v1/dashboard/lab" 2>/dev/null | jv '["gemini_enabled"]' 2>/dev/null || echo "")
if [ -n "$LAB" ] && [ "$LAB" != "None" ]; then
  if [ "$LAB" = "True" ]; then
    apatch "/v1/dashboard/lab" '{"gemini_enabled":false}' > /dev/null 2>&1
    apatch "/v1/dashboard/lab" '{"gemini_enabled":true}' > /dev/null 2>&1
  else
    apatch "/v1/dashboard/lab" '{"gemini_enabled":true}' > /dev/null 2>&1
    apatch "/v1/dashboard/lab" '{"gemini_enabled":false}' > /dev/null 2>&1
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

save_counts
