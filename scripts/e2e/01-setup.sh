#!/usr/bin/env bash
# Phase 01: Infrastructure + Auth + Dual Provider Registration + API Key
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

# ── Phase 1: Infrastructure ───────────────────────────────────────────────────

hdr "Phase 1: Infrastructure Setup"

if [ "$SKIP_DB_RESET" = "0" ]; then
  docker compose exec -T postgres psql -U veronex -d veronex -c \
    "DROP SCHEMA public CASCADE; CREATE SCHEMA public;" > /dev/null 2>&1
  docker compose run --rm migrate-postgres > /dev/null 2>&1
  # Clear all Valkey keys (ZSET queue, demand counters, caches, etc.)
  docker compose exec -T valkey valkey-cli EVAL \
    "local count=0; for _,k in ipairs(redis.call('keys','veronex:*')) do count=count+redis.call('del',k) end; return count" 0 \
    > /dev/null 2>&1 || true
  docker compose restart veronex > /dev/null 2>&1
  info "Waiting for veronex to start..."
  for i in $(seq 1 30); do
    sleep 2
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$API/health" 2>/dev/null || echo "000")
    [ "$HTTP_CODE" = "200" ] && break
    [ "$i" -eq 30 ] && fail "veronex did not start in 60s"
  done
  pass "DB reset, Valkey cleared, veronex restarted"
else
  info "Skipping DB reset (SKIP_DB_RESET=1)"
fi

# Clear login-attempt counters to avoid lockout
docker compose exec -T valkey valkey-cli EVAL \
  "for _,k in ipairs(redis.call('keys','veronex:login_attempts:*')) do redis.call('del',k) end" 0 \
  > /dev/null 2>&1 || true

H=$(curl -sf "$API/health" 2>/dev/null || echo "")
R=$(curl -sf "$API/readyz" 2>/dev/null || echo "")
[ "$H" = "ok" ] && [ "$R" = "ok" ] && pass "health=ok, readyz=ok" || fail "health=$H readyz=$R"

# ── Phase 2: Authentication ───────────────────────────────────────────────────

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
[ -z "$TK" ] && fail "Could not extract JWT token" && exit 1
pass "JWT token obtained"
save_var TK "$TK"

STATS=$(aget "/v1/dashboard/stats" 2>/dev/null || echo "{}")
pass "Dashboard accessible — total_jobs=$(echo "$STATS" | jv '["total_jobs"]' || echo 0)"

# ── Phase 3: Local Provider Registration (localhost) ─────────────────────────

hdr "Phase 3: Local Provider Registration (localhost:11434)"

# Check local Ollama reachability from host (script runs on host, not inside container)
# OLLAMA_LOCAL may be host.docker.internal which only resolves inside Docker — fall back to localhost
LOCAL_CHECK_URL="${OLLAMA_LOCAL/host.docker.internal/localhost}"
LOCAL_ALIVE=$(curl -sf --max-time 5 "$LOCAL_CHECK_URL/api/version" 2>/dev/null | python3 -c "import sys,json; print('yes')" 2>/dev/null || echo "no")
[ "$LOCAL_ALIVE" = "yes" ] && pass "Local Ollama reachable ($LOCAL_CHECK_URL → registered as $OLLAMA_LOCAL)" \
  || info "Local Ollama not reachable — tests may be limited"

SRV_LOCAL_RES=$(apost "/v1/servers" \
  "{\"name\":\"local-dev\",\"node_exporter_url\":\"$NODE_EXPORTER_LOCAL\"}" || echo "")
SERVER_ID_LOCAL=$(echo "$SRV_LOCAL_RES" | jv '["id"]' || echo "")
[ -n "$SERVER_ID_LOCAL" ] && [ "$SERVER_ID_LOCAL" != "None" ] \
  && pass "Local server registered: $SERVER_ID_LOCAL" || fail "Local server registration failed"
save_var SERVER_ID_LOCAL "$SERVER_ID_LOCAL"

PROV_LOCAL_RES=$(apost "/v1/providers" \
  "{\"name\":\"local-ollama\",\"provider_type\":\"ollama\",\"url\":\"$OLLAMA_LOCAL\",\"num_parallel\":4}" || echo "")
PROVIDER_ID_LOCAL=$(echo "$PROV_LOCAL_RES" | jv '["id"]' || echo "")
[ -n "$PROVIDER_ID_LOCAL" ] && [ "$PROVIDER_ID_LOCAL" != "None" ] \
  && pass "Local provider registered: $PROVIDER_ID_LOCAL" || fail "Local provider registration failed"
save_var PROVIDER_ID_LOCAL "$PROVIDER_ID_LOCAL"

if [ -n "$PROVIDER_ID_LOCAL" ] && [ "$PROVIDER_ID_LOCAL" != "None" ] && \
   [ -n "$SERVER_ID_LOCAL" ] && [ "$SERVER_ID_LOCAL" != "None" ]; then
  apatch "/v1/providers/$PROVIDER_ID_LOCAL" \
    "{\"name\":\"local-ollama\",\"server_id\":\"$SERVER_ID_LOCAL\",\"gpu_index\":0,\"num_parallel\":4}" > /dev/null 2>&1
  pass "Local provider linked to local server"
fi

# Keep PROVIDER_ID as the local one for backward compat
save_var PROVIDER_ID "$PROVIDER_ID_LOCAL"
save_var SERVER_ID "$SERVER_ID_LOCAL"

# ── Phase 4: Remote Provider Registration ────────────────────────────────────

hdr "Phase 4: Remote Provider Registration ($OLLAMA_REMOTE)"

REMOTE_ALIVE=$(curl -sf --max-time 10 "$OLLAMA_REMOTE/api/version" 2>/dev/null | python3 -c "import sys,json; print('yes')" 2>/dev/null || echo "no")
[ "$REMOTE_ALIVE" = "yes" ] && pass "Remote Ollama reachable ($OLLAMA_REMOTE)" || info "Remote Ollama not reachable — remote tests may be limited"

SRV_REMOTE_RES=$(apost "/v1/servers" \
  "{\"name\":\"k8s-worker-ai-01\",\"node_exporter_url\":\"$NODE_EXPORTER_REMOTE\"}" || echo "")
SERVER_ID_REMOTE=$(echo "$SRV_REMOTE_RES" | jv '["id"]' || echo "")
[ -n "$SERVER_ID_REMOTE" ] && [ "$SERVER_ID_REMOTE" != "None" ] \
  && pass "Remote server registered: $SERVER_ID_REMOTE" || fail "Remote server registration failed"
save_var SERVER_ID_REMOTE "$SERVER_ID_REMOTE"

PROV_REMOTE_RES=$(apost "/v1/providers" \
  "{\"name\":\"remote-ollama\",\"provider_type\":\"ollama\",\"url\":\"$OLLAMA_REMOTE\",\"num_parallel\":4}" || echo "")
PROVIDER_ID_REMOTE=$(echo "$PROV_REMOTE_RES" | jv '["id"]' || echo "")
[ -n "$PROVIDER_ID_REMOTE" ] && [ "$PROVIDER_ID_REMOTE" != "None" ] \
  && pass "Remote provider registered: $PROVIDER_ID_REMOTE" || fail "Remote provider registration failed"
save_var PROVIDER_ID_REMOTE "$PROVIDER_ID_REMOTE"

if [ -n "$PROVIDER_ID_REMOTE" ] && [ "$PROVIDER_ID_REMOTE" != "None" ] && \
   [ -n "$SERVER_ID_REMOTE" ] && [ "$SERVER_ID_REMOTE" != "None" ]; then
  apatch "/v1/providers/$PROVIDER_ID_REMOTE" \
    "{\"name\":\"remote-ollama\",\"server_id\":\"$SERVER_ID_REMOTE\",\"gpu_index\":0,\"num_parallel\":4}" > /dev/null 2>&1
  pass "Remote provider linked to remote server"
fi

# Verify both providers listed
PROV_COUNT=$(aget "/v1/providers" | jv '["total"]' || echo "0")
[ "$PROV_COUNT" -ge 2 ] && pass "Both providers registered (count=$PROV_COUNT)" \
  || fail "Expected ≥2 providers, got $PROV_COUNT"

# ── Phase 5: Model Sync (both providers) ─────────────────────────────────────

hdr "Phase 5: Model Sync"

apost "/v1/ollama/models/sync" "{}" > /dev/null 2>&1 || true
for i in $(seq 1 20); do
  sleep 2
  SYNC_STATUS=$(aget "/v1/ollama/sync/status" 2>/dev/null | jv '["status"]' 2>/dev/null || echo "running")
  [ "$SYNC_STATUS" != "running" ] && break
done
pass "Global model sync: $SYNC_STATUS"

# Sync each provider individually (triggers VramPool + capacity analysis)
if [ -n "$PROVIDER_ID_LOCAL" ] && [ "$PROVIDER_ID_LOCAL" != "None" ]; then
  c=$(apostc "/v1/providers/$PROVIDER_ID_LOCAL/sync" "{}" | code)
  case "$c" in 200|202) pass "Local provider sync triggered → $c" ;; *) info "Local sync → $c" ;; esac
fi
if [ -n "$PROVIDER_ID_REMOTE" ] && [ "$PROVIDER_ID_REMOTE" != "None" ]; then
  c=$(apostc "/v1/providers/$PROVIDER_ID_REMOTE/sync" "{}" | code)
  case "$c" in 200|202) pass "Remote provider sync triggered → $c" ;; *) info "Remote sync → $c" ;; esac
fi

# Verify MODEL available
MODELS=$(aget "/v1/ollama/models" || echo '{"models":[]}')
MODEL_COUNT=$(echo "$MODELS" | jv '["models"].__len__()' || echo "0")
[ "$MODEL_COUNT" -ge 1 ] && pass "$MODEL_COUNT models synced" || fail "No models synced"

HAS_MODEL=$(echo "$MODELS" | python3 -c "
import sys, json; model='$MODEL'; base=model.split(':')[0]
d=json.loads(sys.stdin.read())
names=[m.get('name',m.get('model_name','')) for m in d.get('models',[])]
print('yes' if any(n==model or n.startswith(base+':') for n in names) else 'no')
" 2>/dev/null || echo "no")
[ "$HAS_MODEL" = "yes" ] && pass "$MODEL available" || fail "$MODEL not found in synced models"

# ── Phase 6: Capacity Settings ────────────────────────────────────────────────

hdr "Phase 6: Capacity Settings"

SETTINGS=$(aget "/v1/dashboard/capacity/settings" || echo "{}")
pass "Capacity settings loaded"

apatch "/v1/dashboard/capacity/settings" '{"probe_permits":2,"probe_rate":5}' > /dev/null 2>&1
S2=$(aget "/v1/dashboard/capacity/settings" || echo "{}")
PP2=$(echo "$S2" | jv '["probe_permits"]' || echo "")
PR2=$(echo "$S2" | jv '["probe_rate"]' || echo "")
[ "$PP2" = "2" ] && [ "$PR2" = "5" ] && pass "Capacity settings updated (permits=2 rate=5)" \
  || fail "Capacity settings update failed (pp=$PP2 pr=$PR2)"
apatch "/v1/dashboard/capacity/settings" '{"probe_permits":1,"probe_rate":3,"analyzer_model":"'"$MODEL"'"}' > /dev/null 2>&1

S3=$(aget "/v1/dashboard/capacity/settings" || echo "{}")
AM=$(echo "$S3" | jv '["analyzer_model"]' || echo "")
[ "$AM" = "$MODEL" ] && pass "Analyzer model set to $MODEL" \
  || fail "Analyzer model not set (got=$AM)"

# ── Phase 7: API Key ──────────────────────────────────────────────────────────

hdr "Phase 7: API Key"

ACCOUNT_ID=$(aget "/v1/accounts" | jv '["accounts"][0]["id"]' || echo "")
if [ -n "$ACCOUNT_ID" ] && [ "$ACCOUNT_ID" != "None" ]; then
  KEY_RES=$(apost "/v1/keys" \
    "{\"tenant_id\":\"$ACCOUNT_ID\",\"name\":\"e2e-paid\",\"tier\":\"paid\"}" || echo "")
  API_KEY=$(echo "$KEY_RES" | jv '["key"]' || echo "")
  [ -n "$API_KEY" ] && [ "$API_KEY" != "None" ] \
    && pass "Paid API key created: ${API_KEY:0:12}..." || fail "API key creation failed"
  API_KEY_ID_PAID=$(echo "$KEY_RES" | jv '["id"]' || echo "")
  save_var API_KEY "$API_KEY"
  save_var API_KEY_PAID "$API_KEY"
  save_var API_KEY_ID_PAID "$API_KEY_ID_PAID"

  # Also create a standard key for tier-priority tests
  STD_KEY_RES=$(apost "/v1/keys" \
    "{\"tenant_id\":\"$ACCOUNT_ID\",\"name\":\"e2e-standard\",\"tier\":\"free\"}" || echo "")
  STD_KEY=$(echo "$STD_KEY_RES" | jv '["key"]' || echo "")
  [ -n "$STD_KEY" ] && [ "$STD_KEY" != "None" ] \
    && pass "Standard API key created: ${STD_KEY:0:12}..." || info "Standard key not created"
  save_var STD_KEY "$STD_KEY"
  save_var API_KEY_STANDARD "$STD_KEY"
else
  fail "Could not find account for API key creation"
fi

# ── Phase 8: Wait for providers to come online ────────────────────────────────

hdr "Phase 8: Provider Online Wait"

for attempt in $(seq 1 12); do
  STATUS=$(aget "/v1/providers" 2>/dev/null | jv '["providers"][0]["status"]' 2>/dev/null || echo "")
  if [ "$STATUS" = "online" ]; then
    pass "Provider online (attempt $attempt)"
    break
  fi
  [ "$attempt" -eq 12 ] && info "Provider still offline after 60s — inference tests may fail"
  sleep 5
done

save_counts
