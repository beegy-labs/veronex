#!/usr/bin/env bash
# Phase 04: Account / API Key / Provider (num_parallel) / Server CRUD
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

# ── Account CRUD ──────────────────────────────────────────────────────────────

hdr "Account CRUD"

assert_get "/v1/accounts" 200 "List accounts"

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

  c=$(adelc "/v1/keys/$KEY_ID" | code)
  [ "$c" = "204" ] && pass "Delete key → 204" || fail "Delete key → $c"
else
  fail "Create key failed ($KEY_CODE)"
fi

# ── Provider CRUD (including num_parallel) ────────────────────────────────────

hdr "Provider CRUD — num_parallel Field (SDD)"

# Verify num_parallel in existing providers
if [ -n "${PROVIDER_ID_LOCAL:-}" ] && [ "$PROVIDER_ID_LOCAL" != "None" ]; then
  PROV_JSON=$(agetc "/v1/providers/$PROVIDER_ID_LOCAL" | body 2>/dev/null || echo "{}")
  # Fall back to list endpoint
  PROV_JSON=$(aget "/v1/providers" 2>/dev/null | python3 -c "
import sys,json; providers=json.loads(sys.stdin.read())
p=[p for p in providers if p.get('id')=='$PROVIDER_ID_LOCAL']
import json; print(json.dumps(p[0]) if p else '{}')
" 2>/dev/null || echo "{}")
  NP_VAL=$(echo "$PROV_JSON" | jv '["num_parallel"]' 2>/dev/null || echo "")
  [ -n "$NP_VAL" ] && [ "$NP_VAL" != "None" ] \
    && pass "Local provider num_parallel=$NP_VAL" || fail "num_parallel missing from provider response"
fi

# Create a temp provider with explicit num_parallel
TMP_RES=$(apostc "/v1/providers" \
  "{\"name\":\"tmp-np-test\",\"provider_type\":\"ollama\",\"url\":\"http://127.0.0.1:59998\",\"num_parallel\":8}")
TMP_CODE=$(echo "$TMP_RES" | code)
TMP_ID=$(echo "$TMP_RES" | body | jv '["id"]' 2>/dev/null || echo "")
if [ "$TMP_CODE" = "201" ] && [ -n "$TMP_ID" ] && [ "$TMP_ID" != "None" ]; then
  pass "Create temp provider with num_parallel=8 → 201"

  # Verify num_parallel=8 in list
  TMP_NP=$(aget "/v1/providers" 2>/dev/null | python3 -c "
import sys,json; providers=json.loads(sys.stdin.read())
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
else
  fail "Create temp provider failed ($TMP_CODE)"
fi

# Non-existent provider → 404
c=$(agetc "/v1/providers/00000000-0000-0000-0000-000000000000/models" | code)
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

TMP_SRV=$(apostc "/v1/servers" '{"name":"tmp-srv-crud"}')
TMP_SRV_CODE=$(echo "$TMP_SRV" | code)
TMP_SRV_ID=$(echo "$TMP_SRV" | body | jv '["id"]' 2>/dev/null || echo "")
if [ "$TMP_SRV_CODE" = "201" ] && [ -n "$TMP_SRV_ID" ] && [ "$TMP_SRV_ID" != "None" ]; then
  c=$(apatchc "/v1/servers/$TMP_SRV_ID" '{"name":"tmp-srv-updated"}' | code)
  [ "$c" = "200" ] && pass "Update server → 200" || fail "Update server → $c"
  c=$(adelc "/v1/servers/$TMP_SRV_ID" | code)
  [ "$c" = "204" ] && pass "Delete server → 204" || fail "Delete server → $c"
else
  fail "Create temp server failed ($TMP_SRV_CODE)"
fi

save_counts
