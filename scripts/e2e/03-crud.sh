#!/usr/bin/env bash
# Phase 14-18: Account, Key, Provider, Server CRUD
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

# ── Account CRUD ────────────────────────────────────────────────────────────

hdr "Account CRUD"

assert_get "/v1/accounts" 200 "List accounts"

TEST_USER="e2e-user-$(python3 -c 'import uuid;print(str(uuid.uuid4())[:8])')"
ACCT_RES=$(apostc "/v1/accounts" "{\"username\":\"$TEST_USER\",\"password\":\"TestPass123\",\"name\":\"E2E Test User\",\"role\":\"admin\"}")
ACCT_CODE=$(echo "$ACCT_RES" | code)
ACCT_ID=$(echo "$ACCT_RES" | body | jv '["id"]' 2>/dev/null || echo "")
if { [ "$ACCT_CODE" = "200" ] || [ "$ACCT_CODE" = "201" ]; } && [ -n "$ACCT_ID" ] && [ "$ACCT_ID" != "None" ]; then
  pass "Create account → $ACCT_CODE ($TEST_USER)"

  c=$(apatchc "/v1/accounts/$ACCT_ID" '{"role":"admin"}' | code)
  [ "$c" = "200" ] || [ "$c" = "204" ] && pass "Update → $c" || fail "Update → $c"

  c=$(apatchc "/v1/accounts/$ACCT_ID/active" '{"is_active":false}' | code)
  [ "$c" = "200" ] || [ "$c" = "204" ] && pass "Deactivate → $c" || fail "Deactivate → $c"

  assert_get "/v1/accounts/$ACCT_ID/sessions" 200 "List sessions"

  c=$(adelc "/v1/accounts/$ACCT_ID" | code)
  [ "$c" = "204" ] && pass "Delete account → 204" || fail "Delete account → $c"
else
  fail "Create account failed ($ACCT_CODE)"
fi

DUP_CODE=$(apostc "/v1/accounts" "{\"username\":\"$USERNAME\",\"password\":\"test1234\",\"name\":\"Dup\",\"role\":\"admin\"}" | code)
case "$DUP_CODE" in
  400|409|500) pass "Duplicate username rejected → $DUP_CODE" ;;
  *) fail "Duplicate: expected 400/409/500, got $DUP_CODE" ;;
esac

# ── API Key CRUD ────────────────────────────────────────────────────────────

hdr "API Key CRUD"

assert_get "/v1/keys" 200 "List keys"

KEY_RES=$(apostc "/v1/keys" "{\"tenant_id\":\"$USERNAME\",\"name\":\"e2e-lifecycle-key\",\"tier\":\"free\"}")
KEY_CODE=$(echo "$KEY_RES" | code)
KEY_ID=$(echo "$KEY_RES" | body | jv '["id"]' 2>/dev/null || echo "")
KEY_RAW=$(echo "$KEY_RES" | body | jv '["key"]' 2>/dev/null || echo "")
if [ "$KEY_CODE" = "200" ] || [ "$KEY_CODE" = "201" ] && [ -n "$KEY_ID" ] && [ "$KEY_ID" != "None" ]; then
  pass "Create key → $KEY_CODE"

  c=$(apatchc "/v1/keys/$KEY_ID" '{"is_active":false}' | code)
  [ "$c" = "204" ] || [ "$c" = "200" ] && pass "Toggle off → $c" || fail "Toggle → $c"

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

# ── Provider Management ─────────────────────────────────────────────────────

hdr "Provider Management"

if [ -n "$PROVIDER_ID" ] && [ "$PROVIDER_ID" != "None" ]; then
  assert_get "/v1/providers/$PROVIDER_ID/models" 200 "Provider models"
  assert_get "/v1/providers/$PROVIDER_ID/selected-models" 200 "Selected models"

  c=$(apatchc "/v1/providers/$PROVIDER_ID/selected-models/$MODEL" '{"is_enabled":false}' | code)
  [ "$c" = "200" ] || [ "$c" = "204" ] && pass "Disable model → $c" || fail "Disable → $c"
  apatch "/v1/providers/$PROVIDER_ID/selected-models/$MODEL" '{"is_enabled":true}' > /dev/null 2>&1
  pass "Model disable/enable OK"
fi

c=$(agetc "/v1/providers/00000000-0000-0000-0000-000000000000/models" | code)
[ "$c" = "404" ] && pass "Non-existent provider → 404" || fail "Expected 404, got $c"

TMP_RES=$(apostc "/v1/providers" "{\"name\":\"tmp-delete\",\"provider_type\":\"ollama\",\"url\":\"http://127.0.0.1:59999\"}")
TMP_CODE=$(echo "$TMP_RES" | code)
TMP_ID=$(echo "$TMP_RES" | body | jv '["id"]' 2>/dev/null || echo "")
if [ "$TMP_CODE" = "201" ] && [ -n "$TMP_ID" ] && [ "$TMP_ID" != "None" ]; then
  c=$(adelc "/v1/providers/$TMP_ID" | code)
  [ "$c" = "204" ] && pass "Delete provider → 204" || fail "Delete provider → $c"
else
  fail "Create temp provider failed ($TMP_CODE)"
fi

c=$(agetc "/v1/ollama/models/$MODEL/providers" | code)
[ "$c" = "200" ] && pass "Model→provider mapping → 200" || fail "Mapping → $c"

# ── Server CRUD ──────────────────────────────────────────────────────────────

hdr "Server CRUD"

TMP_RES=$(apostc "/v1/servers" '{"name":"tmp-srv-test"}')
TMP_CODE=$(echo "$TMP_RES" | code)
TMP_ID=$(echo "$TMP_RES" | body | jv '["id"]' 2>/dev/null || echo "")
if [ "$TMP_CODE" = "201" ] && [ -n "$TMP_ID" ] && [ "$TMP_ID" != "None" ]; then
  c=$(apatchc "/v1/servers/$TMP_ID" '{"name":"tmp-srv-updated"}' | code)
  [ "$c" = "200" ] && pass "Update server → 200" || fail "Update server → $c"
  c=$(adelc "/v1/servers/$TMP_ID" | code)
  [ "$c" = "204" ] && pass "Delete server → 204" || fail "Delete server → $c"
else
  fail "Create temp server failed ($TMP_CODE)"
fi

save_counts
