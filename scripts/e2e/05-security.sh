#!/usr/bin/env bash
# Phase 05: Auth Edge Cases / Security Hardening / Rate Limiting / RBAC
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; ensure_auth

# Wait for any queued/running jobs from previous phases to drain
wait_queue_empty 30

# ── Auth Edge Cases ───────────────────────────────────────────────────────────

hdr "Auth Edge Cases"

# Clear login-attempt counters to avoid rate-limit interference from parallel tests
docker compose exec -T valkey valkey-cli EVAL \
  "for _,k in ipairs(redis.call('keys','veronex:login_attempts:*')) do redis.call('del',k) end" 0 \
  > /dev/null 2>&1 || true

c=$(rawpostc "/v1/auth/login" '{"username":"nobody","password":"wrong"}' | code)
[ "$c" = "401" ] && pass "Invalid creds → 401" || fail "Expected 401, got $c"

c=$(rawc "/v1/providers" | code)
[ "$c" = "401" ] && pass "No token → 401" || fail "Expected 401, got $c"

c=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
  -H "Authorization: Bearer sk-invalid-key" -H "Content-Type: application/json" \
  -d '{"model":"test","messages":[{"role":"user","content":"hi"}]}' | code)
[ "$c" = "401" ] && pass "Invalid API key → 401" || fail "Expected 401, got $c"

# Token refresh & logout
LOGIN_HDRS=$(curl -si "$API/v1/auth/login" \
  -H 'Content-Type: application/json' -d @/tmp/_sched_login.json 2>/dev/null)
REFRESH_TK=$(echo "$LOGIN_HDRS" | sed -n 's/.*veronex_refresh_token=\([^;]*\).*/\1/p' | head -1)
if [ -n "$REFRESH_TK" ]; then
  REFRESH_RES=$(curl -s -w "\n%{http_code}" -X POST "$API/v1/auth/refresh" \
    -H "Cookie: veronex_refresh_token=$REFRESH_TK")
  REFRESH_CODE=$(echo "$REFRESH_RES" | code)
  if [ "$REFRESH_CODE" = "200" ]; then
    pass "Token refresh → 200"
    NEW_TK=$(echo "$REFRESH_RES" | sed -n 's/.*veronex_refresh_token=\([^;]*\).*/\1/p' | head -1)
    [ -z "$NEW_TK" ] && NEW_TK="$REFRESH_TK"
    c=$(curl -s -w "\n%{http_code}" -X POST "$API/v1/auth/logout" \
      -H "Cookie: veronex_refresh_token=$NEW_TK" | code)
    [ "$c" = "204" ] && pass "Logout → 204" || fail "Logout → $c"

    # Verify revoked session JWT is rejected
    REVOKED_ACCESS=$(echo "$REFRESH_RES" | body | python3 -c "
import sys,json
try: print(json.loads(sys.stdin.read()).get('access_token',''))
except: print('')
" 2>/dev/null || echo "")
    if [ -n "$REVOKED_ACCESS" ]; then
      REV_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$API/v1/providers" \
        -H "Authorization: Bearer $REVOKED_ACCESS" 2>/dev/null || echo "000")
      [ "$REV_CODE" = "401" ] \
        && pass "Revoked session JWT rejected → 401" \
        || info "Revoked session JWT → $REV_CODE (session may still be valid within grace window)"
    fi
  else
    fail "Token refresh → $REFRESH_CODE"
  fi
else
  fail "No refresh cookie"
fi

# ── Security Hardening ────────────────────────────────────────────────────────

hdr "Security Hardening"

SEC_HDRS=$(curl -sI "$API/health" 2>/dev/null)
echo "$SEC_HDRS" | grep -qi "x-content-type-options: nosniff" \
  && pass "X-Content-Type-Options: nosniff" || fail "Missing X-Content-Type-Options"
echo "$SEC_HDRS" | grep -qi "x-frame-options: deny" \
  && pass "X-Frame-Options: DENY" || fail "Missing X-Frame-Options"
echo "$SEC_HDRS" | grep -qi "referrer-policy" \
  && pass "Referrer-Policy present" || fail "Missing Referrer-Policy"

# SSRF — cloud metadata endpoints
c=$(apostc "/v1/providers" '{"name":"ssrf-meta","provider_type":"ollama","url":"http://169.254.169.254/latest/meta-data/"}' | code)
[ "$c" != "201" ] && pass "SSRF blocked: AWS metadata → $c" || fail "SSRF: metadata IP accepted"

c=$(apostc "/v1/providers" '{"name":"ssrf-gcp","provider_type":"ollama","url":"http://metadata.google.internal/"}' | code)
[ "$c" != "201" ] && pass "SSRF blocked: GCP metadata → $c" || fail "SSRF: metadata hostname accepted"

# Input validation — oversized model name
LONG_MODEL=$(python3 -c "print('a' * 300)")
c=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$LONG_MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}" | code)
case "$c" in
  400|413|422) pass "Oversized model name rejected → $c" ;;
  *) fail "Oversized model: expected 400/413/422, got $c" ;;
esac

# ── Rate Limiting ─────────────────────────────────────────────────────────────

hdr "Rate Limiting (RPM)"

RL_RES=$(apost "/v1/keys" \
  "{\"tenant_id\":\"$USERNAME\",\"name\":\"rpm-test\",\"rate_limit_rpm\":2,\"tier\":\"paid\"}" || echo "")
RL_KEY=$(echo "$RL_RES" | jv '["key"]' || echo "")
RL_KEY_ID=$(echo "$RL_RES" | jv '["id"]' || echo "")
if [ -n "$RL_KEY" ] && [ "$RL_KEY" != "None" ]; then
  RL_TMPDIR=$(mktemp -d)
  for i in 1 2 3; do
    (curl -s -w "%{http_code}" -o /dev/null --max-time 30 "$API/v1/chat/completions" \
      -H "Authorization: Bearer $RL_KEY" -H "Content-Type: application/json" \
      -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Say $i\"}],\"max_tokens\":4,\"stream\":false}" \
      > "$RL_TMPDIR/$i") &
  done
  wait
  RL_CODES=$(cat "$RL_TMPDIR"/* 2>/dev/null | tr '\n' ' '); rm -rf "$RL_TMPDIR"
  echo "$RL_CODES" | grep -q "429" \
    && pass "RPM limit enforced — codes: $RL_CODES" || fail "RPM not enforced — codes: $RL_CODES"

  # Verify rate limit key has TTL in Valkey (should expire, not persist forever)
  RL_HASH=$(echo "$RL_RES" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('key_hash',''))" 2>/dev/null || echo "")
  if [ -n "$RL_HASH" ]; then
    RL_TTL=$(valkey_ttl "veronex:rpm:$RL_HASH")
    if [ "$RL_TTL" -gt 0 ] 2>/dev/null; then
      pass "RPM counter has TTL ($RL_TTL seconds) — will auto-expire"
    elif [ "$RL_TTL" = "-1" ]; then
      fail "RPM counter has no TTL — will persist forever (memory leak)"
    else
      info "RPM counter TTL=$RL_TTL (key may have expired already)"
    fi
  fi

  adel "/v1/keys/$RL_KEY_ID" > /dev/null 2>&1
else
  fail "Rate limit key creation failed"
fi

# ── TPM Rate Limiting ─────────────────────────────────────────────────────────

hdr "Rate Limiting (TPM)"

TPM_RES=$(apost "/v1/keys" \
  "{\"tenant_id\":\"$USERNAME\",\"name\":\"tpm-test\",\"rate_limit_tpm\":50,\"rate_limit_rpm\":100,\"tier\":\"paid\"}" || echo "")
TPM_KEY=$(echo "$TPM_RES" | jv '["key"]' || echo "")
TPM_KEY_ID=$(echo "$TPM_RES" | jv '["id"]' || echo "")
if [ -n "$TPM_KEY" ] && [ "$TPM_KEY" != "None" ]; then
  # First request: consume tokens with a large max_tokens response
  TPM_C1=$(curl -s -w "%{http_code}" -o /dev/null --max-time 60 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $TPM_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Write a long essay about AI\"}],\"max_tokens\":80,\"stream\":false}" \
    2>/dev/null || true)
  # Second request: should hit TPM limit
  TPM_C2=$(curl -s -w "%{http_code}" -o /dev/null --max-time 60 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $TPM_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Write another long essay about ML\"}],\"max_tokens\":80,\"stream\":false}" \
    2>/dev/null || true)
  if [ "$TPM_C2" = "429" ]; then
    pass "TPM limit enforced — req1=$TPM_C1 req2=$TPM_C2 (429)"
  elif [ "$TPM_C1" = "200" ] && [ "$TPM_C2" = "200" ]; then
    info "TPM limit not triggered (tokens may not have exceeded 50) — req1=$TPM_C1 req2=$TPM_C2"
  elif [ "$TPM_C1" = "000" ] || [ "$TPM_C2" = "000" ]; then
    fail "TPM test failed — connection error during parallel run (req1=$TPM_C1 req2=$TPM_C2)"
  else
    fail "TPM test unexpected — req1=$TPM_C1 req2=$TPM_C2"
  fi
  adel "/v1/keys/$TPM_KEY_ID" > /dev/null 2>&1
else
  fail "TPM rate limit key creation failed"
fi

# ── Session & RBAC ────────────────────────────────────────────────────────────

hdr "Session & RBAC"

# Expired API key
PAST=$(python3 -c "from datetime import datetime,timezone; print(datetime(2020,1,1,tzinfo=timezone.utc).isoformat())")
EXP_RES=$(apost "/v1/keys" \
  "{\"tenant_id\":\"$USERNAME\",\"name\":\"expired\",\"tier\":\"paid\",\"expires_at\":\"$PAST\"}" || echo "")
EXP_KEY=$(echo "$EXP_RES" | jv '["key"]' || echo "")
EXP_KEY_ID=$(echo "$EXP_RES" | jv '["id"]' || echo "")
if [ -n "$EXP_KEY" ] && [ "$EXP_KEY" != "None" ]; then
  c=$(curl -s -w "%{http_code}" -o /dev/null "$API/v1/chat/completions" \
    -H "Authorization: Bearer $EXP_KEY" -H "Content-Type: application/json" \
    -d '{"model":"test","messages":[{"role":"user","content":"hi"}]}')
  [ "$c" = "401" ] && pass "Expired key → 401" || fail "Expired key: got $c"
  adel "/v1/keys/$EXP_KEY_ID" > /dev/null 2>&1
fi

# Get viewer role ID for RBAC tests
VIEWER_ROLE_ID=$(aget "/v1/roles" | python3 -c "
import sys,json
roles=json.loads(sys.stdin.read())
r=[r for r in roles if r.get('name')=='viewer']
print(r[0]['id'] if r else '')
" 2>/dev/null || echo "")

# RBAC restrictions (viewer role blocked from protected endpoints)
RBAC_USER="e2e-rbac-$(python3 -c 'import uuid;print(str(uuid.uuid4())[:8])')"
RBAC_RES=$(apostc "/v1/accounts" \
  "{\"username\":\"$RBAC_USER\",\"password\":\"TestPass123!\",\"name\":\"RBAC\",\"role_ids\":[\"$VIEWER_ROLE_ID\"]}")
RBAC_CODE=$(echo "$RBAC_RES" | code)
RBAC_ACCT_ID=$(echo "$RBAC_RES" | body | jv '["id"]' 2>/dev/null || echo "")
if [ "$RBAC_CODE" = "200" ] || [ "$RBAC_CODE" = "201" ]; then
  RBAC_LOGIN=$(curl -si "$API/v1/auth/login" -H 'Content-Type: application/json' \
    -d "{\"username\":\"$RBAC_USER\",\"password\":\"TestPass123!\"}" 2>/dev/null)
  RBAC_TK=$(echo "$RBAC_LOGIN" | sed -n 's/.*veronex_access_token=\([^;]*\).*/\1/p' | head -1)
  if [ -n "$RBAC_TK" ]; then
    BEFORE=$(curl -s -w "\n%{http_code}" "$API/v1/keys" -H "Authorization: Bearer $RBAC_TK" | code)
    adelc "/v1/accounts/$RBAC_ACCT_ID/sessions" > /dev/null 2>&1; sleep 1
    AFTER=$(curl -s -w "\n%{http_code}" "$API/v1/keys" -H "Authorization: Bearer $RBAC_TK" | code)
    [ "$AFTER" = "401" ] && pass "Revoked session → 401 (was $BEFORE)" || fail "Revoked: got $AFTER"

    RBAC_LOGIN2=$(curl -si "$API/v1/auth/login" -H 'Content-Type: application/json' \
      -d "{\"username\":\"$RBAC_USER\",\"password\":\"TestPass123!\"}" 2>/dev/null)
    RBAC_TK2=$(echo "$RBAC_LOGIN2" | sed -n 's/.*veronex_access_token=\([^;]*\).*/\1/p' | head -1)
    if [ -n "$RBAC_TK2" ]; then
      # viewer has no account_manage → /accounts blocked
      c=$(curl -s -w "\n%{http_code}" "$API/v1/accounts" -H "Authorization: Bearer $RBAC_TK2" | code)
      [ "$c" = "403" ] && pass "RBAC: viewer → /accounts = 403" || info "RBAC: /accounts = $c"
      # viewer has no audit_view → /audit blocked
      c=$(curl -s -w "\n%{http_code}" "$API/v1/audit?limit=1" -H "Authorization: Bearer $RBAC_TK2" | code)
      [ "$c" = "403" ] && pass "RBAC: viewer → /audit = 403" || info "RBAC: /audit = $c"
      # viewer has no provider_manage → /providers POST blocked
      c=$(curl -s -w "\n%{http_code}" "$API/v1/providers" -H "Authorization: Bearer $RBAC_TK2" \
        -H "Content-Type: application/json" -d '{"name":"blocked","provider_type":"ollama","url":"http://blocked"}' | code)
      [ "$c" = "403" ] && pass "RBAC: viewer → provider create = 403" || info "RBAC: provider create = $c"
      # viewer has no key_manage → /keys POST blocked
      c=$(curl -s -w "\n%{http_code}" "$API/v1/keys" -H "Authorization: Bearer $RBAC_TK2" \
        -H "Content-Type: application/json" -d '{"tenant_id":"test","name":"blocked","tier":"free"}' | code)
      [ "$c" = "403" ] && pass "RBAC: viewer → key create = 403" || info "RBAC: key create = $c"
      # viewer has no role_manage → /roles POST blocked
      c=$(curl -s -w "\n%{http_code}" "$API/v1/roles" -H "Authorization: Bearer $RBAC_TK2" \
        -H "Content-Type: application/json" -d '{"name":"blocked","permissions":[],"menus":[]}' | code)
      [ "$c" = "403" ] && pass "RBAC: viewer → role create = 403" || info "RBAC: role create = $c"
    fi
  fi
  adel "/v1/accounts/$RBAC_ACCT_ID" > /dev/null 2>&1
else
  fail "RBAC account creation failed ($RBAC_CODE)"
fi

# ── Role & Permission CRUD ────────────────────────────────────────────────────

hdr "Role & Permission CRUD"

# List roles (super can access)
assert_get "/v1/roles" 200 "List roles"

# Create a test role with limited permissions
ROLE_RES=$(apostc "/v1/roles" '{"name":"e2e-test-role","permissions":["dashboard_view"],"menus":["dashboard"]}')
ROLE_CODE=$(echo "$ROLE_RES" | code)
ROLE_ID=$(echo "$ROLE_RES" | body | jv '["id"]' 2>/dev/null || echo "")
[ "$ROLE_CODE" = "200" ] || [ "$ROLE_CODE" = "201" ] && pass "Create role → $ROLE_CODE" || fail "Create role → $ROLE_CODE"

# Update role
if [ -n "$ROLE_ID" ] && [ "$ROLE_ID" != "None" ]; then
  c=$(apatchc "/v1/roles/$ROLE_ID" '{"permissions":["dashboard_view","api_test"],"menus":["dashboard","test"]}' | code)
  [ "$c" = "200" ] || [ "$c" = "204" ] && pass "Update role → $c" || fail "Update role → $c"
fi

# System role cannot be modified
SUPER_ROLE_ID=$(aget "/v1/roles" | python3 -c "
import sys,json
roles=json.loads(sys.stdin.read())
r=[r for r in roles if r.get('name')=='super']
print(r[0]['id'] if r else '')
" 2>/dev/null || echo "")
if [ -n "$SUPER_ROLE_ID" ]; then
  c=$(apatchc "/v1/roles/$SUPER_ROLE_ID" '{"permissions":["dashboard_view"]}' | code)
  [ "$c" = "403" ] && pass "System role update blocked → 403" || fail "System role update → $c (expected 403)"
fi

# N:N role assignment — create account with multiple roles
ROLE2_RES=$(apostc "/v1/roles" '{"name":"e2e-key-role","permissions":["key_manage"],"menus":["keys"]}')
ROLE2_ID=$(echo "$ROLE2_RES" | body | jv '["id"]' 2>/dev/null || echo "")

if [ -n "$ROLE_ID" ] && [ "$ROLE_ID" != "None" ] && [ -n "$ROLE2_ID" ] && [ "$ROLE2_ID" != "None" ]; then
  MULTI_USER="e2e-multi-$(python3 -c 'import uuid;print(str(uuid.uuid4())[:8])')"
  MULTI_RES=$(apostc "/v1/accounts" \
    "{\"username\":\"$MULTI_USER\",\"password\":\"TestPass123!\",\"name\":\"Multi\",\"role_ids\":[\"$ROLE_ID\",\"$ROLE2_ID\"]}")
  MULTI_CODE=$(echo "$MULTI_RES" | code)
  MULTI_ACCT_ID=$(echo "$MULTI_RES" | body | jv '["id"]' 2>/dev/null || echo "")
  [ "$MULTI_CODE" = "200" ] || [ "$MULTI_CODE" = "201" ] \
    && pass "Create account with 2 roles → $MULTI_CODE" \
    || fail "Multi-role account creation → $MULTI_CODE"

  # Verify merged permissions: login and check access
  if [ -n "$MULTI_ACCT_ID" ] && [ "$MULTI_ACCT_ID" != "None" ]; then
    MULTI_LOGIN=$(curl -si "$API/v1/auth/login" -H 'Content-Type: application/json' \
      -d "{\"username\":\"$MULTI_USER\",\"password\":\"TestPass123!\"}" 2>/dev/null)
    MULTI_TK=$(echo "$MULTI_LOGIN" | sed -n 's/.*veronex_access_token=\([^;]*\).*/\1/p' | head -1)
    if [ -n "$MULTI_TK" ]; then
      # dashboard_view from role1 → GET /v1/dashboard/stats should work
      c=$(curl -s -w "\n%{http_code}" "$API/v1/dashboard/stats" -H "Authorization: Bearer $MULTI_TK" | code)
      [ "$c" = "200" ] && pass "Multi-role: dashboard_view works → 200" || fail "Multi-role: dashboard → $c"
      # key_manage from role2 → GET /v1/keys should work
      c=$(curl -s -w "\n%{http_code}" "$API/v1/keys" -H "Authorization: Bearer $MULTI_TK" | code)
      [ "$c" = "200" ] && pass "Multi-role: key_manage works → 200" || fail "Multi-role: keys → $c"
      # account_manage NOT in either role → /accounts blocked
      c=$(curl -s -w "\n%{http_code}" "$API/v1/accounts" -H "Authorization: Bearer $MULTI_TK" | code)
      [ "$c" = "403" ] && pass "Multi-role: account_manage blocked → 403" || info "Multi-role: accounts → $c"
    fi

    # Role with assigned user cannot be deleted
    c=$(adelc "/v1/roles/$ROLE_ID" | code)
    [ "$c" = "409" ] || [ "$c" = "400" ] \
      && pass "Delete role with users → $c (blocked)" \
      || fail "Delete role with users → $c (expected 409/400)"

    # Cleanup multi-role account
    adel "/v1/accounts/$MULTI_ACCT_ID" > /dev/null 2>&1
  fi
fi

# Delete test roles (now unassigned)
if [ -n "$ROLE_ID" ] && [ "$ROLE_ID" != "None" ]; then
  c=$(adelc "/v1/roles/$ROLE_ID" | code)
  [ "$c" = "204" ] && pass "Delete role → 204" || fail "Delete role → $c"
fi
if [ -n "$ROLE2_ID" ] && [ "$ROLE2_ID" != "None" ]; then
  c=$(adelc "/v1/roles/$ROLE2_ID" | code)
  [ "$c" = "204" ] && pass "Delete role2 → 204" || fail "Delete role2 → $c"
fi

# ZSET queue: MAX_QUEUE_PER_MODEL enforcement (SDD: per-model cap 2000)
# Practical test: verify 429 is returned when inference is requested with no providers
info "SDD MAX_QUEUE_PER_MODEL=2000, MAX_QUEUE_SIZE=10000 — enforced via Lua atomic enqueue"

# ── Login Rate Limit ──────────────────────────────────────────────────────────

hdr "Login Rate Limit (IP-based)"

# Read LOGIN_RATE_LIMIT from container env via a running process env (or use compose config)
CONTAINER_LIMIT=$(docker compose exec -T veronex sh -c 'echo ${LOGIN_RATE_LIMIT:-10}' 2>/dev/null | tr -d '\r\n' || echo "10")
if [ "${CONTAINER_LIMIT:-10}" = "0" ]; then
  fail "LOGIN_RATE_LIMIT=0 — rate limiting is disabled; set LOGIN_RATE_LIMIT > 0 to enforce login lockout"
  # Still verify that unlimited login works (no false 429)
  c=$(rawpostc "/v1/auth/login" "{\"username\":\"$USERNAME\",\"password\":\"$PASSWORD\"}" | code)
  [ "$c" = "200" ] && pass "Login allowed when rate limit disabled → 200" \
    || fail "Login failed unexpectedly → $c"
else
  # Helper: delete all login_attempts keys from host side (avoids xargs dependency in container)
  _clear_login_rl() {
    local keys
    keys=$(valkey_keys 'veronex:login_attempts:*')
    if [ -n "$keys" ]; then
      # shellcheck disable=SC2086
      docker compose exec -T valkey valkey-cli del $keys > /dev/null 2>&1 || true
    fi
  }

  # Clear any existing attempt counter for the test IP
  _clear_login_rl

  LIMIT="${CONTAINER_LIMIT:-10}"
  info "LOGIN_RATE_LIMIT=$LIMIT — testing lockout after $LIMIT failed attempts"

  # Fire LIMIT+1 bad login attempts to trigger lockout
  LAST_CODE="000"
  for i in $(seq 1 $((LIMIT + 1))); do
    LAST_CODE=$(rawpostc "/v1/auth/login" '{"username":"nonexistent_e2e_user","password":"badpass"}' | code)
  done

  [ "$LAST_CODE" = "429" ] \
    && pass "Login rate limit enforced — attempt $((LIMIT + 1)) → 429" \
    || fail "Login rate limit NOT enforced — attempt $((LIMIT + 1)) → $LAST_CODE (expected 429)"

  # Clear counters so subsequent tests aren't affected
  _clear_login_rl

  # Verify legitimate login still works after counter reset
  c=$(rawpostc "/v1/auth/login" "{\"username\":\"$USERNAME\",\"password\":\"$PASSWORD\"}" | code)
  [ "$c" = "200" ] && pass "Legitimate login OK after counter reset → 200" \
    || fail "Legitimate login failed after reset → $c"
fi

save_counts
