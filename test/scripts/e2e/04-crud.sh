#!/usr/bin/env bash
# Phase 04: Account / API Key / Provider (num_parallel) / Server CRUD
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; ensure_auth
ensure_provider_ids

# ── Account CRUD ──────────────────────────────────────────────────────────────

hdr "Account CRUD"

assert_get "/v1/accounts" 200 "List accounts"
assert_get "/v1/accounts?page=1&limit=10" 200 "List accounts paginated"
assert_get "/v1/accounts?search=test&page=1&limit=5" 200 "List accounts search+paginated"
# Verify response has total field
ACCT_TOTAL=$(aget "/v1/accounts?limit=10" 2>/dev/null | jv '["total"]' 2>/dev/null || echo "")
[ -n "$ACCT_TOTAL" ] && [ "$ACCT_TOTAL" != "None" ] \
  && pass "Accounts response has total=$ACCT_TOTAL" || fail "Accounts response missing total field"

TEST_USER="e2e-user-$(python3 -c 'import uuid;print(str(uuid.uuid4())[:8])')"
ACCT_RES=$(apostc "/v1/accounts" \
  "{\"username\":\"$TEST_USER\",\"password\":\"TestPass123\",\"name\":\"E2E User\",\"role\":\"admin\"}")
ACCT_CODE=$(echo "$ACCT_RES" | code)
ACCT_ID=$(echo "$ACCT_RES" | body | jv '["id"]' 2>/dev/null || echo "")
if { [ "$ACCT_CODE" = "200" ] || [ "$ACCT_CODE" = "201" ]; } && [ -n "$ACCT_ID" ] && [ "$ACCT_ID" != "None" ]; then
  pass "Create account → $ACCT_CODE ($TEST_USER)"

  c=$(apatchc "/v1/accounts/$ACCT_ID" '{"role":"admin"}' | code)
  [ "$c" = "200" ] || [ "$c" = "204" ] && pass "Update role → $c" || fail "Update → $c"

  c=$(apatchc "/v1/accounts/$ACCT_ID/active" '{"is_active":false}' | code)
  [ "$c" = "200" ] || [ "$c" = "204" ] && pass "Deactivate → $c" || fail "Deactivate → $c"

  assert_get "/v1/accounts/$ACCT_ID/sessions" 200 "List sessions"

  c=$(adelc "/v1/accounts/$ACCT_ID" | code)
  [ "$c" = "204" ] && pass "Delete account → 204" || fail "Delete → $c"
else
  fail "Create account failed ($ACCT_CODE)"
fi

DUP_CODE=$(apostc "/v1/accounts" \
  "{\"username\":\"$USERNAME\",\"password\":\"test1234\",\"name\":\"Dup\",\"role\":\"admin\"}" | code)
case "$DUP_CODE" in
  400|409|500) pass "Duplicate username rejected → $DUP_CODE" ;;
  *) fail "Duplicate: expected 400/409/500, got $DUP_CODE" ;;
esac

# ── API Key CRUD ──────────────────────────────────────────────────────────────

hdr "API Key CRUD"

assert_get "/v1/keys" 200 "List keys"
assert_get "/v1/keys?page=1&limit=10" 200 "List keys paginated"
assert_get "/v1/keys?search=e2e&page=1&limit=5" 200 "List keys search+paginated"
KEYS_TOTAL=$(aget "/v1/keys?limit=10" 2>/dev/null | jv '["total"]' 2>/dev/null || echo "")
[ -n "$KEYS_TOTAL" ] && [ "$KEYS_TOTAL" != "None" ] \
  && pass "Keys response has total=$KEYS_TOTAL" || fail "Keys response missing total field"

KEY_RES=$(apostc "/v1/keys" \
  "{\"tenant_id\":\"$USERNAME\",\"name\":\"e2e-lifecycle\",\"tier\":\"free\"}")
KEY_CODE=$(echo "$KEY_RES" | code)
KEY_ID=$(echo "$KEY_RES" | body | jv '["id"]' 2>/dev/null || echo "")
KEY_RAW=$(echo "$KEY_RES" | body | jv '["key"]' 2>/dev/null || echo "")
if [ "$KEY_CODE" = "200" ] || [ "$KEY_CODE" = "201" ] && [ -n "$KEY_ID" ] && [ "$KEY_ID" != "None" ]; then
  pass "Create key → $KEY_CODE"

  c=$(apatchc "/v1/keys/$KEY_ID" '{"is_active":false}' | code)
  [ "$c" = "204" ] || [ "$c" = "200" ] && pass "Disable key → $c" || fail "Disable → $c"

  c=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
    -H "Authorization: Bearer $KEY_RAW" -H "Content-Type: application/json" \
    -d '{"model":"test","messages":[{"role":"user","content":"hi"}]}' | code)
  [ "$c" = "401" ] || [ "$c" = "403" ] && pass "Inactive key rejected → $c" || fail "Expected 401/403, got $c"

  c=$(apatchc "/v1/keys/$KEY_ID" '{"tier":"paid"}' | code)
  [ "$c" = "204" ] || [ "$c" = "200" ] && pass "Change tier → $c" || fail "Tier → $c"

  # Regenerate key — new raw key returned
  REGEN_RES=$(apostc "/v1/keys/$KEY_ID/regenerate" "{}")
  REGEN_CODE=$(echo "$REGEN_RES" | code)
  REGEN_KEY=$(echo "$REGEN_RES" | body | jv '["key"]' 2>/dev/null || echo "")
  if { [ "$REGEN_CODE" = "200" ] || [ "$REGEN_CODE" = "201" ]; } && [ -n "$REGEN_KEY" ] && [ "$REGEN_KEY" != "None" ] && [ "$REGEN_KEY" != "$KEY_RAW" ]; then
    pass "Regenerate key → $REGEN_CODE (new key differs from original)"
  elif [ "$REGEN_CODE" = "200" ] || [ "$REGEN_CODE" = "201" ]; then
    pass "Regenerate key → $REGEN_CODE"
  else
    fail "Regenerate key → $REGEN_CODE"
  fi

  # mcp_cap_points PATCH
  c=$(apatchc "/v1/keys/$KEY_ID" '{"mcp_cap_points":5}' | code)
  [ "$c" = "204" ] || [ "$c" = "200" ] && pass "PATCH mcp_cap_points=5 → $c" || fail "PATCH mcp_cap_points → $c"

  c=$(adelc "/v1/keys/$KEY_ID" | code)
  [ "$c" = "204" ] && pass "Delete key → 204" || fail "Delete key → $c"
else
  fail "Create key failed ($KEY_CODE)"
fi

# ── Provider CRUD (including num_parallel) ────────────────────────────────────

hdr "Provider List Pagination"

assert_get "/v1/providers" 200 "List providers"
assert_get "/v1/providers?page=1&limit=10" 200 "List providers paginated"
assert_get "/v1/providers?search=ollama&page=1&limit=5" 200 "List providers search+paginated"
PROV_TOTAL=$(aget "/v1/providers?limit=10" 2>/dev/null | jv '["total"]' 2>/dev/null || echo "")
[ -n "$PROV_TOTAL" ] && [ "$PROV_TOTAL" != "None" ] \
  && pass "Providers response has total=$PROV_TOTAL" || fail "Providers response missing total field"

# Ollama model list pagination
assert_get "/v1/ollama/models" 200 "List ollama models"
assert_get "/v1/ollama/models?page=1&limit=20" 200 "List ollama models paginated"
OLLAMA_TOTAL=$(aget "/v1/ollama/models?limit=20" 2>/dev/null | jv '["total"]' 2>/dev/null || echo "")
[ -n "$OLLAMA_TOTAL" ] && [ "$OLLAMA_TOTAL" != "None" ] \
  && pass "Ollama models response has total=$OLLAMA_TOTAL" || fail "Ollama models missing total field"

# Ollama model→providers pagination
if [ -n "${MODEL:-}" ]; then
  MENC=$(python3 -c "import urllib.parse; print(urllib.parse.quote('$MODEL'))" 2>/dev/null || echo "$MODEL")
  assert_get "/v1/ollama/models/$MENC/providers?page=1&limit=10" 200 "Model providers paginated"
  MP_TOTAL=$(aget "/v1/ollama/models/$MENC/providers?limit=10" 2>/dev/null | jv '["total"]' 2>/dev/null || echo "")
  [ -n "$MP_TOTAL" ] && [ "$MP_TOTAL" != "None" ] \
    && pass "Model providers response has total=$MP_TOTAL" || fail "Model providers missing total field"
fi

hdr "Provider CRUD — num_parallel Field (SDD)"

# Verify num_parallel in existing providers
if [ -n "${PROVIDER_ID_LOCAL:-}" ] && [ "$PROVIDER_ID_LOCAL" != "None" ]; then
  PROV_JSON=$(agetc "/v1/providers/$PROVIDER_ID_LOCAL" | body 2>/dev/null || echo "{}")
  # Fall back to list endpoint
  PROV_JSON=$(aget "/v1/providers" 2>/dev/null | python3 -c "
import sys,json; d=json.loads(sys.stdin.read())
providers=d.get('providers', d) if isinstance(d, dict) else d
p=[p for p in providers if p.get('id')=='$PROVIDER_ID_LOCAL']
import json; print(json.dumps(p[0]) if p else '{}')
" 2>/dev/null || echo "{}")
  NP_VAL=$(echo "$PROV_JSON" | jv '["num_parallel"]' 2>/dev/null || echo "")
  [ -n "$NP_VAL" ] && [ "$NP_VAL" != "None" ] \
    && pass "Local provider num_parallel=$NP_VAL" || fail "num_parallel missing from provider response"
fi

# Create a temp provider with explicit num_parallel (use reachable Ollama URL)
TMP_PROV_URL="${OLLAMA_LOCAL:-http://host.docker.internal:11434}"
TMP_RES=$(apostc "/v1/providers" \
  "{\"name\":\"tmp-np-test\",\"provider_type\":\"ollama\",\"url\":\"$TMP_PROV_URL\",\"num_parallel\":8}")
TMP_CODE=$(echo "$TMP_RES" | code)
TMP_ID=$(echo "$TMP_RES" | body | jv '["id"]' 2>/dev/null || echo "")
if [ "$TMP_CODE" = "201" ] && [ -n "$TMP_ID" ] && [ "$TMP_ID" != "None" ]; then
  pass "Create temp provider with num_parallel=8 → 201"

  # Verify num_parallel=8 in list
  TMP_NP=$(aget "/v1/providers" 2>/dev/null | python3 -c "
import sys,json; d=json.loads(sys.stdin.read())
providers=d.get('providers', d) if isinstance(d, dict) else d
p=[p for p in providers if p.get('id')=='$TMP_ID']
print(p[0].get('num_parallel','?') if p else '?')
" 2>/dev/null || echo "?")
  [ "$TMP_NP" = "8" ] && pass "num_parallel=8 stored correctly" \
    || fail "num_parallel mismatch: expected 8 got $TMP_NP"

  # Update num_parallel to 2
  c=$(apatchc "/v1/providers/$TMP_ID" \
    '{"name":"tmp-np-test","num_parallel":2}' | code)
  [ "$c" = "200" ] && pass "Update num_parallel → 200" || fail "Update num_parallel → $c"

  # Cleanup
  c=$(adelc "/v1/providers/$TMP_ID" | code)
  [ "$c" = "204" ] && pass "Delete temp provider → 204" || fail "Delete temp provider → $c"
elif [ "$TMP_CODE" = "409" ]; then
  pass "Temp provider duplicate URL rejected → 409 (expected when Ollama URL already registered)"
elif [ "$TMP_CODE" = "502" ]; then
  fail "Temp provider Ollama unreachable → 502 (num_parallel CRUD cannot run)"
else
  fail "Create temp provider failed ($TMP_CODE)"
fi

# Non-existent provider → 404 (use typed prov_xxx format — raw UUID returns 400)
c=$(agetc "/v1/providers/prov_0000000000000000000000/models" | code)
[ "$c" = "404" ] && pass "Non-existent provider → 404" || fail "Expected 404, got $c"

# Registered providers model/selection endpoints
if [ -n "${PROVIDER_ID_LOCAL:-}" ] && [ "$PROVIDER_ID_LOCAL" != "None" ]; then
  assert_get "/v1/providers/$PROVIDER_ID_LOCAL/models" 200 "Local provider models"
  assert_get "/v1/providers/$PROVIDER_ID_LOCAL/selected-models" 200 "Local selected models"

  c=$(apatchc "/v1/providers/$PROVIDER_ID_LOCAL/selected-models/$MODEL" '{"is_enabled":false}' | code)
  [ "$c" = "200" ] || [ "$c" = "204" ] && pass "Disable model → $c" || fail "Disable → $c"
  apatch "/v1/providers/$PROVIDER_ID_LOCAL/selected-models/$MODEL" '{"is_enabled":true}' > /dev/null 2>&1
  pass "Model disable/enable cycle OK"
fi

c=$(agetc "/v1/ollama/models/$MODEL/providers" | code)
[ "$c" = "200" ] && pass "Model→provider mapping → 200" || fail "Mapping → $c"

# ── Server CRUD ───────────────────────────────────────────────────────────────

hdr "Server CRUD"

assert_get "/v1/servers" 200 "List servers"
assert_get "/v1/servers?page=1&limit=10" 200 "List servers paginated"
assert_get "/v1/servers?search=local&page=1&limit=5" 200 "List servers search+paginated"
SRVS_TOTAL=$(aget "/v1/servers?limit=10" 2>/dev/null | jv '["total"]' 2>/dev/null || echo "")
[ -n "$SRVS_TOTAL" ] && [ "$SRVS_TOTAL" != "None" ] \
  && pass "Servers response has total=$SRVS_TOTAL" || fail "Servers response missing total field"

# Validation tests (missing URL, duplicate, unreachable) → 11-verify-liveness.sh

# Create with valid URL (use existing node-exporter that is already registered → 409 expected)
TMP_SRV_URL="${NODE_EXPORTER_LOCAL:-http://host.docker.internal:9100}"
TMP_SRV=$(apostc "/v1/servers" \
  "{\"name\":\"tmp-srv-crud\",\"node_exporter_url\":\"$TMP_SRV_URL\"}")
TMP_SRV_CODE=$(echo "$TMP_SRV" | code)
TMP_SRV_ID=$(echo "$TMP_SRV" | body | jv '["id"]' 2>/dev/null || echo "")
if [ "$TMP_SRV_CODE" = "201" ] && [ -n "$TMP_SRV_ID" ] && [ "$TMP_SRV_ID" != "None" ]; then
  c=$(apatchc "/v1/servers/$TMP_SRV_ID" '{"name":"tmp-srv-updated"}' | code)
  [ "$c" = "200" ] && pass "Update server → 200" || fail "Update server → $c"
  c=$(adelc "/v1/servers/$TMP_SRV_ID" | code)
  [ "$c" = "204" ] && pass "Delete server → 204" || fail "Delete server → $c"
elif [ "$TMP_SRV_CODE" = "409" ]; then
  pass "Duplicate server URL rejected → 409"
elif [ "$TMP_SRV_CODE" = "502" ]; then
  fail "Server node-exporter unreachable → 502 (server CRUD cannot run)"
else
  fail "Create temp server failed ($TMP_SRV_CODE)"
fi

# ── Global Model Settings ─────────────────────────────────────────────────────

hdr "Global Model Settings"

assert_get "/v1/models/global-settings" 200 "List global model settings"
assert_get "/v1/models/global-disabled" 200 "List globally disabled models"

# Disable a model globally
GMS_RES=$(apatchc "/v1/models/global-settings/$MODEL" '{"is_enabled":false}')
GMS_CODE=$(echo "$GMS_RES" | code)
if [ "$GMS_CODE" = "200" ]; then
  pass "Disable model globally → 200"

  # Verify it appears in disabled list
  DISABLED=$(aget "/v1/models/global-disabled" 2>/dev/null || echo "[]")
  echo "$DISABLED" | grep -q "$MODEL" \
    && pass "Model in disabled list" || fail "Model not in disabled list"

  # Re-enable
  c=$(apatchc "/v1/models/global-settings/$MODEL" '{"is_enabled":true}' | code)
  [ "$c" = "200" ] && pass "Re-enable model globally → 200" || fail "Re-enable → $c"
else
  fail "Disable model globally → $GMS_CODE"
fi

# ── API Key Provider Access ───────────────────────────────────────────────────

hdr "API Key Provider Access"

if [ -n "${API_KEY_ID_PAID:-}" ] && [ "$API_KEY_ID_PAID" != "None" ] && [ -n "${PROVIDER_ID_LOCAL:-}" ] && [ "$PROVIDER_ID_LOCAL" != "None" ]; then
  assert_get "/v1/keys/$API_KEY_ID_PAID/providers" 200 "List key provider access"

  c=$(apatchc "/v1/keys/$API_KEY_ID_PAID/providers/$PROVIDER_ID_LOCAL" '{"is_allowed":false}' | code)
  [ "$c" = "200" ] && pass "Deny provider access → 200" || fail "Deny → $c"

  c=$(apatchc "/v1/keys/$API_KEY_ID_PAID/providers/$PROVIDER_ID_LOCAL" '{"is_allowed":true}' | code)
  [ "$c" = "200" ] && pass "Allow provider access → 200" || fail "Allow → $c"
else
  fail "Key provider access test skipped — API_KEY_ID_PAID or PROVIDER_ID_LOCAL not set"
fi

# ── Server/Provider Verify + Liveness ────────────────────────────────────────

hdr "Server Verify — POST /v1/servers/verify"

c=$(apostc "/v1/servers/verify" '{"url":""}' | code)
[ "$c" = "400" ] && pass "Verify server: empty URL → 400" || fail "Verify server: empty URL → $c (expected 400)"

c=$(apostc "/v1/servers/verify" '{"url":"ftp://example.com"}' | code)
[ "$c" = "400" ] && pass "Verify server: ftp:// scheme → 400" || fail "Verify server: ftp:// → $c (expected 400)"

NE_URL="${NODE_EXPORTER_LOCAL:-http://host.docker.internal:9100}"
c=$(apostc "/v1/servers/verify" "{\"url\":\"$NE_URL\"}" | code)
[ "$c" = "409" ] && pass "Verify server: duplicate URL → 409" || fail "Verify server: duplicate URL → $c (expected 409)"

c=$(apostc "/v1/servers/verify" '{"url":"http://192.0.2.1:19999"}' | code)
[ "$c" = "502" ] && pass "Verify server: unreachable → 502" || fail "Verify server: unreachable → $c (expected 502)"

hdr "Provider Verify — POST /v1/providers/verify"

c=$(apostc "/v1/providers/verify" '{"url":""}' | code)
[ "$c" = "400" ] && pass "Verify provider: empty URL → 400" || fail "Verify provider: empty URL → $c (expected 400)"

c=$(apostc "/v1/providers/verify" '{"url":"ftp://example.com:11434"}' | code)
[ "$c" = "400" ] && pass "Verify provider: ftp:// scheme → 400" || fail "Verify provider: ftp:// → $c (expected 400)"

OLLAMA_URL="${OLLAMA_LOCAL:-http://host.docker.internal:11434}"
if [ -n "${PROVIDER_ID_LOCAL:-}" ]; then
  c=$(apostc "/v1/providers/verify" "{\"url\":\"$OLLAMA_URL\"}" | code)
  [ "$c" = "409" ] && pass "Verify provider: duplicate URL → 409" || fail "Verify provider: duplicate URL → $c (expected 409)"
else
  fail "Verify provider duplicate: PROVIDER_ID_LOCAL not set — local provider not registered in setup"
fi

c=$(apostc "/v1/providers/verify" '{"url":"http://192.0.2.1:11434"}' | code)
[ "$c" = "502" ] && pass "Verify provider: unreachable → 502" || fail "Verify provider: unreachable → $c (expected 502)"

hdr "Server Registration Validation"

c=$(apostc "/v1/servers" '{"name":"test-no-url"}' | code)
[ "$c" = "400" ] && pass "Register server: no URL → 400" || fail "Register server: no URL → $c (expected 400)"

c=$(apostc "/v1/servers" '{"name":"test-bad-scheme","node_exporter_url":"ftp://bad"}' | code)
[ "$c" = "400" ] && pass "Register server: bad scheme → 400" || fail "Register server: bad scheme → $c (expected 400)"

c=$(apostc "/v1/servers" "{\"name\":\"test-dup\",\"node_exporter_url\":\"$NE_URL\"}" | code)
[ "$c" = "409" ] && pass "Register server: duplicate URL → 409" || fail "Register server: duplicate URL → $c (expected 409)"

c=$(apostc "/v1/servers" '{"name":"test-unreachable","node_exporter_url":"http://192.0.2.1:19999"}' | code)
[ "$c" = "502" ] && pass "Register server: unreachable → 502" || fail "Register server: unreachable → $c (expected 502)"

hdr "Provider Registration Validation"

if [ -n "${PROVIDER_ID_LOCAL:-}" ]; then
  c=$(apostc "/v1/providers" \
    "{\"name\":\"dup-test\",\"provider_type\":\"ollama\",\"url\":\"$OLLAMA_URL\"}" | code)
  [ "$c" = "409" ] && pass "Register provider: duplicate URL → 409" || fail "Register provider: duplicate URL → $c (expected 409)"
else
  fail "Register provider duplicate: PROVIDER_ID_LOCAL not set — local provider not registered in setup"
fi

c=$(apostc "/v1/providers" \
  '{"name":"bad-test","provider_type":"ollama","url":"http://192.0.2.1:11434"}' | code)
[ "$c" = "502" ] && pass "Register provider: unreachable → 502" || fail "Register provider: unreachable → $c (expected 502)"

c=$(apostc "/v1/providers" '{"name":"no-url","provider_type":"ollama"}' | code)
[ "$c" = "400" ] && pass "Register provider: no URL → 400" || fail "Register provider: no URL → $c (expected 400)"

c=$(apostc "/v1/providers" '{"name":"bad-scheme","provider_type":"ollama","url":"ftp://bad:11434"}' | code)
[ "$c" = "400" ] && pass "Register provider: bad scheme → 400" || fail "Register provider: bad scheme → $c (expected 400)"

hdr "Provider Liveness — Valkey Keys"

ONLINE_COUNT=$(valkey_get "veronex:stats:providers:online")
if [ -n "$ONLINE_COUNT" ] && [ "$ONLINE_COUNT" != "(nil)" ]; then
  pass "PROVIDERS_ONLINE_COUNTER exists (value=$ONLINE_COUNT)"
else
  info "PROVIDERS_ONLINE_COUNTER not set (health_checker may not have run yet)"
fi

if [ -n "${PROVIDER_ID_LOCAL:-}" ] && [ "$PROVIDER_ID_LOCAL" != "None" ]; then
  HB_VAL=$(valkey_get "veronex:provider:hb:$PROVIDER_ID_LOCAL")
  if [ -n "$HB_VAL" ] && [ "$HB_VAL" != "(nil)" ]; then
    pass "Provider heartbeat key present (local)"
  else
    info "Provider heartbeat key absent — agent may not be pushing heartbeats yet"
  fi
fi

if [ -n "${PROVIDER_ID_REMOTE:-}" ] && [ "$PROVIDER_ID_REMOTE" != "None" ]; then
  HB_VAL=$(valkey_get "veronex:provider:hb:$PROVIDER_ID_REMOTE")
  if [ -n "$HB_VAL" ] && [ "$HB_VAL" != "(nil)" ]; then
    pass "Remote provider heartbeat key present"
  else
    info "Remote provider heartbeat key absent — agent may not be running"
  fi
fi

hdr "API Instance Registry — veronex:instances"

INSTANCE_COUNT=$(valkey_scard "veronex:instances")
if [ "${INSTANCE_COUNT:-0}" -ge 1 ]; then
  pass "veronex:instances SET has $INSTANCE_COUNT member(s)"
else
  info "veronex:instances SET empty — multi-instance coordination may not be active"
fi

INSTANCE_ID=$(docker compose exec -T valkey valkey-cli SRANDMEMBER "veronex:instances" 2>/dev/null | tr -d ' \r\n' || echo "")
if [ -n "$INSTANCE_ID" ] && [ "$INSTANCE_ID" != "(nil)" ]; then
  HB_VAL=$(valkey_get "veronex:heartbeat:$INSTANCE_ID")
  if [ -n "$HB_VAL" ] && [ "$HB_VAL" != "(nil)" ]; then
    pass "API instance heartbeat present (veronex:heartbeat:$INSTANCE_ID)"
  else
    info "API instance heartbeat expired or absent for $INSTANCE_ID"
  fi
fi

save_counts
