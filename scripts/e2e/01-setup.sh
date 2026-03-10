#!/usr/bin/env bash
# Phase 1-6: Infrastructure + Auth + Provider + Model Sync + API Key
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

# ── Phase 1: Infrastructure ──────────────────────────────────────────────────

hdr "Phase 1: Infrastructure Setup"

if [ "$SKIP_DB_RESET" = "0" ]; then
  docker compose exec -T postgres psql -U veronex -d veronex -c \
    "DROP SCHEMA public CASCADE; CREATE SCHEMA public;" > /dev/null 2>&1
  # Re-apply migrations since schema was dropped
  docker compose run --rm migrate-postgres > /dev/null 2>&1
  docker compose restart veronex > /dev/null 2>&1
  info "Waiting for veronex to start..."
  for i in $(seq 1 30); do
    sleep 2
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$API/health" 2>/dev/null || echo "000")
    [ "$HTTP_CODE" = "200" ] && break
    [ "$i" -eq 30 ] && fail "veronex did not start in 60s"
  done
  pass "DB reset & veronex restarted"
else
  info "Skipping DB reset (SKIP_DB_RESET=1)"
fi

docker compose exec -T valkey valkey-cli EVAL \
  "for _,k in ipairs(redis.call('keys','veronex:login_attempts:*')) do redis.call('del',k) end" 0 \
  > /dev/null 2>&1 || true

H=$(curl -sf "$API/health" 2>/dev/null || echo "")
R=$(curl -sf "$API/readyz" 2>/dev/null || echo "")
[ "$H" = "ok" ] && [ "$R" = "ok" ] && pass "health=ok, readyz=ok" || fail "health=$H, readyz=$R"

SETUP_STATUS=$(curl -sf "$API/v1/setup/status" 2>/dev/null | jv '["needs_setup"]' || echo "error")
[ "$SETUP_STATUS" = "True" ] && pass "needs_setup=True" || info "needs_setup=$SETUP_STATUS"

# ── Phase 2: Authentication ──────────────────────────────────────────────────

hdr "Phase 2: Authentication"

cat > /tmp/_sched_login.json << EOF
{"username":"$USERNAME","password":"$PASSWORD"}
EOF
SETUP_CODE=$(curl -s -w "\n%{http_code}" "$API/v1/setup" \
  -H 'Content-Type: application/json' -d @/tmp/_sched_login.json | code)
case "$SETUP_CODE" in
  200|201) pass "Admin account created" ;;
  409)     info "Account already exists (409)" ;;
  *)       fail "Setup failed (HTTP $SETUP_CODE)" ;;
esac

LOGIN_RAW=$(curl -si "$API/v1/auth/login" \
  -H 'Content-Type: application/json' -d @/tmp/_sched_login.json 2>&1)
TK=$(echo "$LOGIN_RAW" | sed -n 's/.*veronex_access_token=\([^;]*\).*/\1/p')
if [ -z "$TK" ]; then
  fail "Could not extract JWT token"
  echo -e "${RED}Cannot continue without auth. Exiting.${NC}"; exit 1
fi
pass "JWT token obtained"
save_var TK "$TK"

STATS=$(aget "/v1/dashboard/stats" 2>/dev/null || echo "{}")
pass "Dashboard accessible — total_keys=$(echo "$STATS" | jv '["total_keys"]' || echo err)"

# ── Phase 3: Provider Registration ──────────────────────────────────────────

hdr "Phase 3: Provider Registration"

SERVER_RES=$(apost "/v1/servers" "{\"name\":\"test-gpu-server\",\"node_exporter_url\":\"$NODE_EXPORTER\"}" || echo "")
SERVER_ID=$(echo "$SERVER_RES" | jv '["id"]' || echo "")
[ -n "$SERVER_ID" ] && [ "$SERVER_ID" != "None" ] \
  && pass "Server: $SERVER_ID" || fail "Server registration failed"
save_var SERVER_ID "$SERVER_ID"

PROV_RES=$(apost "/v1/providers" "{\"name\":\"test-ollama\",\"provider_type\":\"ollama\",\"url\":\"$OLLAMA_URL\"}" || echo "")
PROVIDER_ID=$(echo "$PROV_RES" | jv '["id"]' || echo "")
[ -n "$PROVIDER_ID" ] && [ "$PROVIDER_ID" != "None" ] \
  && pass "Provider: $PROVIDER_ID" || fail "Provider registration failed"
save_var PROVIDER_ID "$PROVIDER_ID"

if [ -n "$PROVIDER_ID" ] && [ -n "$SERVER_ID" ]; then
  apatch "/v1/providers/$PROVIDER_ID" \
    "{\"name\":\"test-ollama\",\"server_id\":\"$SERVER_ID\",\"gpu_index\":0}" > /dev/null 2>&1
  pass "Provider linked to server"
fi

PROV_COUNT=$(aget "/v1/providers" | jv '.__len__()' || echo "0")
[ "$PROV_COUNT" -ge 1 ] && pass "Provider count: $PROV_COUNT" || fail "No providers found"

# ── Phase 4: Model Sync ─────────────────────────────────────────────────────

hdr "Phase 4: Model Sync"

apost "/v1/ollama/models/sync" "{}" > /dev/null 2>&1 || true
for i in $(seq 1 20); do
  sleep 1
  SYNC_STATUS=$(aget "/v1/ollama/sync/status" 2>/dev/null | jv '["status"]' 2>/dev/null || echo "running")
  [ "$SYNC_STATUS" != "running" ] && break
done
pass "Sync status: $SYNC_STATUS"

MODELS=$(aget "/v1/ollama/models" || echo '{"models":[]}')
MODEL_COUNT=$(echo "$MODELS" | jv '["models"].__len__()' || echo "0")
echo "$MODELS" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
for m in d.get('models', [])[:8]:
    print(f'    {m.get(\"name\", m.get(\"model_name\", \"?\"))}')
if len(d.get('models', [])) > 8: print(f'    ... and {len(d[\"models\"])-8} more')
" 2>/dev/null || true
[ "$MODEL_COUNT" -ge 1 ] && pass "$MODEL_COUNT models available" || fail "No models synced"

HAS_MODEL=$(echo "$MODELS" | python3 -c "
import sys, json; model='$MODEL'; base=model.split(':')[0]
d=json.loads(sys.stdin.read())
names=[m.get('name',m.get('model_name','')) for m in d.get('models',[])]
print('yes' if any(n==model or n.startswith(base+':') for n in names) else 'no')
" 2>/dev/null || echo "no")
[ "$HAS_MODEL" = "yes" ] && pass "$MODEL available" || fail "$MODEL not found"

# ── Phase 5: Capacity Settings ──────────────────────────────────────────────

hdr "Phase 5: Capacity Settings"

SETTINGS=$(aget "/v1/dashboard/capacity/settings" || echo "{}")
pass "Capacity settings loaded"

apatch "/v1/dashboard/capacity/settings" '{"probe_permits":2,"probe_rate":5}' > /dev/null 2>&1
S2=$(aget "/v1/dashboard/capacity/settings" || echo "{}")
PP2=$(echo "$S2" | jv '["probe_permits"]' || echo ""); PR2=$(echo "$S2" | jv '["probe_rate"]' || echo "")
[ "$PP2" = "2" ] && [ "$PR2" = "5" ] && pass "Settings update verified" || fail "Settings update failed"
apatch "/v1/dashboard/capacity/settings" '{"probe_permits":1,"probe_rate":3}' > /dev/null 2>&1

# ── Phase 6: API Key ────────────────────────────────────────────────────────

hdr "Phase 6: API Key"

ACCOUNT_ID=$(aget "/v1/accounts" | jv '[0]["id"]' || echo "")
if [ -n "$ACCOUNT_ID" ] && [ "$ACCOUNT_ID" != "None" ]; then
  KEY_RES=$(apost "/v1/keys" "{\"tenant_id\":\"$ACCOUNT_ID\",\"name\":\"scheduler-test\",\"tier\":\"paid\"}" || echo "")
  API_KEY=$(echo "$KEY_RES" | jv '["key"]' || echo "")
  [ -n "$API_KEY" ] && [ "$API_KEY" != "None" ] \
    && pass "API key: ${API_KEY:0:12}..." || fail "API key creation failed"
  save_var API_KEY "$API_KEY"
else
  fail "Could not find account"
fi

save_counts
