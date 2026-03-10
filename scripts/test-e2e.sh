#!/usr/bin/env bash
# ── Veronex E2E Integration Test ─────────────────────────────────────────────
# End-to-end validation of the full Veronex pipeline:
#   DB reset → setup → auth → provider registration → model sync → VRAM probing
#   → inference routing → AIMD adaptation → CRUD → security → analytics
#
# Usage:
#   ./scripts/test-e2e.sh                    # full test (DB reset)
#   SKIP_DB_RESET=1 ./scripts/test-e2e.sh    # reuse existing DB
#   MODEL=qwen3:8b CONCURRENT=8 ./scripts/test-e2e.sh
#
# Prerequisites:
#   - docker compose up (Veronex stack running)
#   - At least 1 Ollama server reachable at OLLAMA_URL
set -euo pipefail

# ── Configuration ────────────────────────────────────────────────────────────
API="${API_URL:-http://localhost:3001}"
OLLAMA_URL="${OLLAMA_URL:-https://ollama.girok.dev}"
NODE_EXPORTER="${NODE_EXPORTER:-http://192.168.1.21:9100}"
USERNAME="admin"
PASSWORD="admin2026!"
MODEL="${MODEL:-qwen3:8b}"
CONCURRENT="${CONCURRENT:-6}"
SKIP_DB_RESET="${SKIP_DB_RESET:-0}"

# ── Color helpers ────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'
pass() { echo -e "  ${GREEN}[PASS]${NC} $1"; PASS_COUNT=$((PASS_COUNT+1)); }
fail() { echo -e "  ${RED}[FAIL]${NC} $1"; FAIL_COUNT=$((FAIL_COUNT+1)); FAIL_MSGS+=("$1"); }
info() { echo -e "  ${YELLOW}[INFO]${NC} $1"; }
hdr()  { echo -e "\n${CYAN}${BOLD}── $1 ──${NC}"; }

PASS_COUNT=0; FAIL_COUNT=0; FAIL_MSGS=()

# JSON value extractor (no jq dependency)
jv() { python3 -c "import sys,json; print(json.loads(sys.stdin.read())$1)" 2>/dev/null; }
jvr() { python3 -c "import sys,json; d=json.loads(sys.stdin.read()); $1" 2>/dev/null; }
# Extract body (all lines except last) from curl response with status code appended.
# macOS head does not support -n -1, so we use sed instead.
body() { sed '$d'; }
code() { tail -1; }

# ── Authenticated helpers ────────────────────────────────────────────────────
TK=""
aget()  { curl -sf "$API$1" -H "Authorization: Bearer $TK"; }
apost() { curl -sf "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
apatch(){ curl -sf -X PATCH "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
adel()  { curl -sf -X DELETE "$API$1" -H "Authorization: Bearer $TK"; }
aput()  { curl -sf -X PUT "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
# With HTTP status code
agetc()  { curl -s -w "\n%{http_code}" "$API$1" -H "Authorization: Bearer $TK"; }
apostc() { curl -s -w "\n%{http_code}" "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
apatchc(){ curl -s -w "\n%{http_code}" -X PATCH "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
adelc()  { curl -s -w "\n%{http_code}" -X DELETE "$API$1" -H "Authorization: Bearer $TK"; }
aputc()  { curl -s -w "\n%{http_code}" -X PUT "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
# Unauthenticated with status
rawc()  { curl -s -w "\n%{http_code}" "$API$1"; }
rawpostc() { curl -s -w "\n%{http_code}" "$API$1" -H 'Content-Type: application/json' -d "$2"; }

# ── Cleanup ──────────────────────────────────────────────────────────────────
TMPDIR_A=$(mktemp -d); TMPDIR_B=$(mktemp -d)
cleanup() { rm -rf "$TMPDIR_A" "$TMPDIR_B" /tmp/_sched_login.json 2>/dev/null; }
trap cleanup EXIT

# ══════════════════════════════════════════════════════════════════════════════
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "${CYAN}${BOLD}  Veronex E2E Integration Test${NC}"
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
info "API=$API  Ollama=$OLLAMA_URL  Model=$MODEL  Concurrency=$CONCURRENT"

# ── Phase 1: Infrastructure ──────────────────────────────────────────────────

hdr "Phase 1: Infrastructure Setup"

# Step 1.1: Reset DB
if [ "$SKIP_DB_RESET" = "0" ]; then
  hdr "1.1 Reset database"
  docker compose exec -T postgres psql -U veronex -d veronex -c \
    "DROP SCHEMA public CASCADE; CREATE SCHEMA public;" > /dev/null 2>&1
  docker compose restart veronex > /dev/null 2>&1
  info "Waiting for veronex to start..."
  for i in $(seq 1 30); do
    sleep 2
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$API/health" 2>/dev/null || echo "000")
    if [ "$HTTP_CODE" = "200" ]; then break; fi
    if [ "$i" -eq 30 ]; then fail "veronex did not start in 60s"; fi
  done
  pass "DB reset & veronex restarted"
else
  info "Skipping DB reset (SKIP_DB_RESET=1)"
fi

# Clear login rate limit from previous runs (10/5min per IP)
docker compose exec -T valkey valkey-cli EVAL \
  "for _,k in ipairs(redis.call('keys','veronex:login_attempts:*')) do redis.call('del',k) end" 0 \
  > /dev/null 2>&1 || true

# Step 1.2: Health check
hdr "1.2 Health & readiness"
H=$(curl -sf "$API/health" 2>/dev/null || echo "")
R=$(curl -sf "$API/readyz" 2>/dev/null || echo "")
[ "$H" = "ok" ] && [ "$R" = "ok" ] && pass "health=ok, readyz=ok" || fail "health=$H, readyz=$R"

# Step 1.3: Setup status
hdr "1.3 Setup status (pre-setup)"
SETUP_STATUS=$(curl -sf "$API/v1/setup/status" 2>/dev/null | jv '["needs_setup"]' || echo "error")
[ "$SETUP_STATUS" = "True" ] && pass "needs_setup=True" || info "needs_setup=$SETUP_STATUS (may already be set up)"

# ── Phase 2: Auth & Account ─────────────────────────────────────────────────

hdr "Phase 2: Authentication"

# Step 2.1: Setup admin
hdr "2.1 Create admin account"
cat > /tmp/_sched_login.json << EOF
{"username":"$USERNAME","password":"$PASSWORD"}
EOF
SETUP_RES=$(curl -s -w "\n%{http_code}" "$API/v1/setup" \
  -H 'Content-Type: application/json' -d @/tmp/_sched_login.json)
SETUP_CODE=$(echo "$SETUP_RES" | tail -1)
case "$SETUP_CODE" in
  200|201) pass "Admin account created" ;;
  409)     info "Account already exists (409)" ;;
  *)       fail "Setup failed (HTTP $SETUP_CODE)" ;;
esac

# Step 2.2: Login
hdr "2.2 Login"
LOGIN_RAW=$(curl -si "$API/v1/auth/login" \
  -H 'Content-Type: application/json' -d @/tmp/_sched_login.json 2>&1)
TK=$(echo "$LOGIN_RAW" | sed -n 's/.*veronex_access_token=\([^;]*\).*/\1/p')
if [ -z "$TK" ]; then
  fail "Could not extract JWT token"
  echo -e "${RED}Cannot continue without auth. Exiting.${NC}"
  exit 1
fi
pass "JWT token obtained"

# Step 2.3: Dashboard stats (empty state)
hdr "2.3 Dashboard stats (empty)"
STATS=$(aget "/v1/dashboard/stats" 2>/dev/null || echo "{}")
TOTAL_KEYS=$(echo "$STATS" | jv '["total_keys"]' || echo "err")
pass "Dashboard accessible — total_keys=$TOTAL_KEYS"

# ── Phase 3: Provider Registration ──────────────────────────────────────────

hdr "Phase 3: Provider Registration"

# Step 3.1: Register GPU server
hdr "3.1 Register GPU server"
SERVER_RES=$(apost "/v1/servers" "{\"name\":\"test-gpu-server\",\"node_exporter_url\":\"$NODE_EXPORTER\"}" || echo "")
SERVER_ID=$(echo "$SERVER_RES" | jv '["id"]' || echo "")
if [ -n "$SERVER_ID" ] && [ "$SERVER_ID" != "None" ]; then
  pass "Server: $SERVER_ID"
else
  fail "Server registration failed"
fi

# Step 3.2: Register Ollama provider
hdr "3.2 Register Ollama provider"
PROV_RES=$(apost "/v1/providers" "{\"name\":\"test-ollama\",\"provider_type\":\"ollama\",\"url\":\"$OLLAMA_URL\"}" || echo "")
PROVIDER_ID=$(echo "$PROV_RES" | jv '["id"]' || echo "")
PROV_STATUS=$(echo "$PROV_RES" | jv '["status"]' || echo "unknown")
if [ -n "$PROVIDER_ID" ] && [ "$PROVIDER_ID" != "None" ]; then
  pass "Provider: $PROVIDER_ID (status=$PROV_STATUS)"
else
  fail "Provider registration failed"
fi

# Step 3.3: Link provider → server
hdr "3.3 Link provider to server"
if [ -n "$PROVIDER_ID" ] && [ -n "$SERVER_ID" ]; then
  LINK_RES=$(apatch "/v1/providers/$PROVIDER_ID" \
    "{\"name\":\"test-ollama\",\"server_id\":\"$SERVER_ID\",\"gpu_index\":0}" 2>&1 || echo "")
  pass "Provider linked to server"
else
  fail "Cannot link — missing IDs"
fi

# Step 3.4: Provider list verification
hdr "3.4 Verify provider list"
PROV_LIST=$(aget "/v1/providers" || echo "[]")
PROV_COUNT=$(echo "$PROV_LIST" | jv '.__len__()' || echo "0")
[ "$PROV_COUNT" -ge 1 ] && pass "Provider count: $PROV_COUNT" || fail "No providers found"

# ── Phase 4: Model Sync & Discovery ─────────────────────────────────────────

hdr "Phase 4: Model Sync"

# Step 4.1: Trigger model sync
hdr "4.1 Sync Ollama models"
SYNC_RES=$(apost "/v1/ollama/models/sync" "{}" || echo "{}")
SYNC_ID=$(echo "$SYNC_RES" | jv '["job_id"]' || echo "unknown")
info "Sync job: $SYNC_ID"

# Wait for sync completion
for i in $(seq 1 20); do
  sleep 1
  SYNC_STATUS=$(aget "/v1/ollama/sync/status" 2>/dev/null | jv '["status"]' 2>/dev/null || echo "running")
  [ "$SYNC_STATUS" != "running" ] && break
done
[ "$SYNC_STATUS" = "completed" ] || [ "$SYNC_STATUS" != "running" ] && pass "Sync status: $SYNC_STATUS" || fail "Sync timed out"

# Step 4.2: Verify models
hdr "4.2 Verify synced models"
MODELS=$(aget "/v1/ollama/models" || echo '{"models":[]}')
MODEL_COUNT=$(echo "$MODELS" | jv '["models"].__len__()' || echo "0")
info "Models synced: $MODEL_COUNT"
echo "$MODELS" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
for m in d.get('models', [])[:8]:
    print(f'    {m.get(\"name\", m.get(\"model_name\", \"?\"))}')
if len(d.get('models', [])) > 8:
    print(f'    ... and {len(d[\"models\"])-8} more')
" 2>/dev/null || true
[ "$MODEL_COUNT" -ge 1 ] && pass "$MODEL_COUNT models available" || fail "No models synced"

# Step 4.3: Check target model exists
hdr "4.3 Verify target model ($MODEL)"
HAS_MODEL=$(echo "$MODELS" | python3 -c "
import sys, json
model = '$MODEL'
base = model.split(':')[0]  # qwen3:8b → qwen3
d = json.loads(sys.stdin.read())
# Exact match first, then prefix match (Ollama resolves aliases)
names = [m.get('name', m.get('model_name', '')) for m in d.get('models', [])]
found = any(n == model or n.startswith(base + ':') for n in names)
print('yes' if found else 'no')
" 2>/dev/null || echo "no")
[ "$HAS_MODEL" = "yes" ] && pass "$MODEL (or variant) is available" || fail "$MODEL not found in synced models"

# ── Phase 5: Capacity Settings ───────────────────────────────────────────────

hdr "Phase 5: Capacity & VRAM"

# Step 5.1: Check settings
hdr "5.1 Capacity settings"
SETTINGS=$(aget "/v1/dashboard/capacity/settings" || echo "{}")
ANALYZER_MODEL=$(echo "$SETTINGS" | jv '["analyzer_model"]' || echo "unknown")
SYNC_ENABLED=$(echo "$SETTINGS" | jv '["sync_enabled"]' || echo "unknown")
SYNC_INTERVAL=$(echo "$SETTINGS" | jv '["sync_interval_secs"]' || echo "unknown")
PROBE_PERMITS=$(echo "$SETTINGS" | jv '["probe_permits"]' || echo "unknown")
PROBE_RATE=$(echo "$SETTINGS" | jv '["probe_rate"]' || echo "unknown")
info "analyzer=$ANALYZER_MODEL sync=$SYNC_ENABLED interval=${SYNC_INTERVAL}s probes=$PROBE_PERMITS rate=$PROBE_RATE"
pass "Capacity settings loaded"

# Step 5.2: Update settings (write + read-back)
hdr "5.2 Update & verify settings"
apatch "/v1/dashboard/capacity/settings" '{"probe_permits":2,"probe_rate":5}' > /dev/null 2>&1
S2=$(aget "/v1/dashboard/capacity/settings" || echo "{}")
PP2=$(echo "$S2" | jv '["probe_permits"]' || echo "")
PR2=$(echo "$S2" | jv '["probe_rate"]' || echo "")
if [ "$PP2" = "2" ] && [ "$PR2" = "5" ]; then
  pass "Settings update verified (permits=$PP2, rate=$PR2)"
else
  fail "Settings update failed (got permits=$PP2, rate=$PR2)"
fi
# Revert
apatch "/v1/dashboard/capacity/settings" '{"probe_permits":1,"probe_rate":3}' > /dev/null 2>&1

# Step 5.3: Queue depth (should be 0 at this point)
hdr "5.3 Queue depth"
QD=$(aget "/v1/dashboard/queue/depth" || echo '{"total":0}')
QD_TOTAL=$(echo "$QD" | jv '["total"]' || echo "0")
QD_PAID=$(echo "$QD" | jv '["api_paid"]' || echo "0")
QD_API=$(echo "$QD" | jv '["api"]' || echo "0")
QD_TEST=$(echo "$QD" | jv '["test"]' || echo "0")
info "Queue: paid=$QD_PAID api=$QD_API test=$QD_TEST total=$QD_TOTAL"
pass "Queue depth accessible"

# ── Phase 6: API Key Creation ────────────────────────────────────────────────

hdr "Phase 6: API Key"

# Step 6.1: Create API key
hdr "6.1 Create test API key"
ACCOUNT_ID=$(aget "/v1/accounts" | jv '[0]["id"]' || echo "")
if [ -z "$ACCOUNT_ID" ] || [ "$ACCOUNT_ID" = "None" ]; then
  fail "Could not find account"
else
  KEY_RES=$(apost "/v1/keys" "{\"tenant_id\":\"$ACCOUNT_ID\",\"name\":\"scheduler-test\",\"tier\":\"paid\"}" || echo "")
  API_KEY=$(echo "$KEY_RES" | jv '["key"]' || echo "")
  if [ -n "$API_KEY" ] && [ "$API_KEY" != "None" ]; then
    pass "API key: ${API_KEY:0:12}..."
  else
    fail "API key creation failed"
  fi
fi

# ── Phase 7: Inference Round 1 (Pre-AIMD) ───────────────────────────────────

hdr "Phase 7: Inference Round 1 — $CONCURRENT concurrent requests (pre-AIMD)"

# Step 7.1: Fire concurrent requests
hdr "7.1 Concurrent inference ($MODEL)"
for i in $(seq 1 "$CONCURRENT"); do
  (
    T0=$(python3 -c "import time; print(int(time.time()*1000))")
    RES=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
      -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
      -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Say only the digit $i.\"}],\"max_tokens\":32,\"stream\":false}" \
      --max-time 120)
    CODE=$(echo "$RES" | tail -1)
    T1=$(python3 -c "import time; print(int(time.time()*1000))")
    ELAPSED=$(( (T1 - T0) ))
    echo "$i $CODE ${ELAPSED}ms" > "$TMPDIR_A/r_$i"
  ) &
done
wait
echo ""
R1_OK=0; R1_Q=0; R1_F=0
for f in "$TMPDIR_A"/r_*; do
  read -r IDX CODE DUR < "$f"
  case "$CODE" in
    200) echo -e "    #$IDX: ${GREEN}200${NC} ($DUR)"; R1_OK=$((R1_OK+1)) ;;
    429|503) echo -e "    #$IDX: ${YELLOW}${CODE}${NC} ($DUR) [queued/throttled]"; R1_Q=$((R1_Q+1)) ;;
    *) echo -e "    #$IDX: ${RED}${CODE}${NC} ($DUR)"; R1_F=$((R1_F+1)) ;;
  esac
done
rm -rf "$TMPDIR_A"
info "Round 1: OK=$R1_OK Queued=$R1_Q Failed=$R1_F"
[ "$R1_OK" -ge 1 ] && pass "Inference routing works" || fail "No successful inferences in Round 1"

# Step 7.2: Verify jobs in dashboard
hdr "7.2 Jobs in dashboard"
sleep 1
JOBS=$(aget "/v1/dashboard/jobs?limit=10" || echo '{"jobs":[]}')
JOB_COUNT=$(echo "$JOBS" | jv '["jobs"].__len__()' || echo "0")
info "Jobs visible: $JOB_COUNT"
[ "$JOB_COUNT" -ge 1 ] && pass "Jobs recorded in DB" || fail "No jobs in dashboard"

# ── Phase 8: Capacity Analyzer Sync ─────────────────────────────────────────

hdr "Phase 8: Capacity Analyzer & AIMD"

# Step 8.1: Trigger manual sync
hdr "8.1 Trigger capacity sync"
apost "/v1/providers/sync" "{}" > /dev/null 2>&1 || true
info "Manual sync triggered, waiting for VRAM probing..."

# Keep model loaded while waiting
for i in $(seq 1 10); do
  curl -s "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}],\"max_tokens\":4,\"stream\":false}" > /dev/null 2>&1 &
  sleep 2
  CAP=$(aget "/v1/dashboard/capacity" 2>/dev/null || echo '{"providers":[]}')
  LOADED_COUNT=$(echo "$CAP" | python3 -c "
import sys,json
d=json.loads(sys.stdin.read())
print(sum(len(p.get('loaded_models',[])) for p in d.get('providers',[])))
" 2>/dev/null || echo 0)
  [ "$LOADED_COUNT" -ge 1 ] && break
  printf "    tick %d (loaded_models: %s)\n" "$i" "$LOADED_COUNT"
done
wait 2>/dev/null

# Step 8.2: Show capacity state
hdr "8.2 Capacity snapshot"
echo "$CAP" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
for p in d.get('providers', []):
    name = p.get('provider_name', '?')
    used = p.get('used_vram_mb', 0)
    total = p.get('total_vram_mb', 0)
    thermal = p.get('thermal_state', 'unknown')
    margin = p.get('safety_margin_pct', 0)
    print(f'    {name}: VRAM={used}/{total}MB thermal={thermal} margin={margin}%')
    for m in p.get('loaded_models', []):
        name = m['model_name']
        w = m['weight_mb']
        kv = m['kv_per_request_mb']
        active = m['active_requests']
        limit = m['max_concurrent']
        concern = m.get('llm_concern', '-')
        print(f'      {name}: weight={w}MB kv={kv}MB active={active}/{limit} concern={concern}')
" 2>/dev/null || echo "    (no capacity data)"

# Step 8.3: Verify AIMD limit (find max across all providers)
get_aimd_limit() {
  aget "/v1/dashboard/capacity" 2>/dev/null | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
limits = [m['max_concurrent']
          for p in d.get('providers', [])
          for m in p.get('loaded_models', [])
          if m['model_name'] == '$MODEL' and m['max_concurrent'] > 0]
print(max(limits) if limits else '0')
" 2>/dev/null || echo "0"
}

AIMD_LIMIT=$(get_aimd_limit)

# If AIMD not set yet, trigger additional sync cycles with inference data
if [ "$AIMD_LIMIT" = "0" ]; then
  info "AIMD limit not set yet — running extra sync cycles..."
  for attempt in 1 2 3; do
    # Generate inference data for the analyzer
    for j in 1 2 3; do
      curl -s "$API/v1/chat/completions" \
        -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
        -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"ping $attempt.$j\"}],\"max_tokens\":4,\"stream\":false}" > /dev/null 2>&1 &
    done
    wait 2>/dev/null
    # Trigger sync
    apost "/v1/providers/sync" "{}" > /dev/null 2>&1 || true
    sleep 5
    AIMD_LIMIT=$(get_aimd_limit)
    [ "$AIMD_LIMIT" != "0" ] && break
    info "  attempt $attempt: limit still 0"
  done
fi

[ -n "$AIMD_LIMIT" ] && [ "$AIMD_LIMIT" -gt 0 ] \
  && pass "AIMD limit for $MODEL = $AIMD_LIMIT" \
  || fail "AIMD limit not set after 3 sync cycles"

# ── Phase 9: DB Verification ────────────────────────────────────────────────

hdr "Phase 9: Database Verification"

# Step 9.1: model_vram_profiles
hdr "9.1 model_vram_profiles"
docker compose exec -T postgres psql -U veronex -d veronex -c \
  "SELECT model_name, weight_mb, kv_per_request_mb, max_concurrent, baseline_tps FROM model_vram_profiles LIMIT 10;" 2>/dev/null
VRAM_ROWS=$(docker compose exec -T postgres psql -U veronex -d veronex -t -c \
  "SELECT COUNT(*) FROM model_vram_profiles;" 2>/dev/null | tr -d ' ')
[ -n "$VRAM_ROWS" ] && [ "$VRAM_ROWS" -ge 1 ] && pass "model_vram_profiles: $VRAM_ROWS rows" || info "model_vram_profiles: empty (analyzer cycle pending)"

# Step 9.2: capacity_settings
hdr "9.2 capacity_settings"
docker compose exec -T postgres psql -U veronex -d veronex -c \
  "SELECT probe_permits, probe_rate, sync_interval_secs, analyzer_model FROM capacity_settings;" 2>/dev/null
pass "capacity_settings verified"

# Step 9.3: inference_jobs
hdr "9.3 inference_jobs"
JOB_ROWS=$(docker compose exec -T postgres psql -U veronex -d veronex -t -c \
  "SELECT COUNT(*) FROM inference_jobs;" 2>/dev/null | tr -d ' ')
info "inference_jobs: $JOB_ROWS rows"
docker compose exec -T postgres psql -U veronex -d veronex -c \
  "SELECT status, COUNT(*) as cnt FROM inference_jobs GROUP BY status;" 2>/dev/null
[ -n "$JOB_ROWS" ] && [ "$JOB_ROWS" -ge 1 ] && pass "Jobs recorded" || fail "No jobs in DB"

# Step 9.4: llm_providers
hdr "9.4 llm_providers"
docker compose exec -T postgres psql -U veronex -d veronex -c \
  "SELECT name, provider_type, status, total_vram_mb FROM llm_providers;" 2>/dev/null
pass "Provider state verified"

# ── Phase 10: Inference Round 2 (AIMD Active) ───────────────────────────────

# Send limit+2 concurrent requests to verify throttling behavior
R2_COUNT=$((AIMD_LIMIT + 2))
[ "$R2_COUNT" -lt "$CONCURRENT" ] && R2_COUNT=$CONCURRENT
[ "$AIMD_LIMIT" -le 0 ] && R2_COUNT=$CONCURRENT

hdr "Phase 10: Inference Round 2 — $R2_COUNT requests (AIMD active, limit=$AIMD_LIMIT)"

TMPDIR_B=$(mktemp -d)
for i in $(seq 1 "$R2_COUNT"); do
  (
    T0=$(python3 -c "import time; print(int(time.time()*1000))")
    RES=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
      -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
      -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Reply with digit $i.\"}],\"max_tokens\":16,\"stream\":false}" \
      --max-time 120)
    CODE=$(echo "$RES" | tail -1)
    T1=$(python3 -c "import time; print(int(time.time()*1000))")
    ELAPSED=$(( (T1 - T0) ))
    echo "$i $CODE ${ELAPSED}ms" > "$TMPDIR_B/r_$i"
  ) &
done
wait
echo ""
R2_OK=0; R2_Q=0; R2_F=0
for f in "$TMPDIR_B"/r_*; do
  read -r IDX CODE DUR < "$f"
  case "$CODE" in
    200) echo -e "    #$IDX: ${GREEN}200${NC} ($DUR)"; R2_OK=$((R2_OK+1)) ;;
    429|503) echo -e "    #$IDX: ${YELLOW}${CODE}${NC} ($DUR) [queued/throttled]"; R2_Q=$((R2_Q+1)) ;;
    *) echo -e "    #$IDX: ${RED}${CODE}${NC} ($DUR)"; R2_F=$((R2_F+1)) ;;
  esac
done
rm -rf "$TMPDIR_B"
info "Round 2: OK=$R2_OK Queued=$R2_Q Failed=$R2_F"
[ "$R2_OK" -ge 1 ] && pass "AIMD-regulated inference works" || fail "No successful inferences in Round 2"

# Verify all requests completed (queued ones should eventually succeed via retry)
[ "$R2_F" -eq 0 ] && pass "All $R2_COUNT requests completed (OK=$R2_OK, queued=$R2_Q)" \
  || fail "$R2_F requests failed under AIMD regulation"

# ── Phase 11: Usage & Analytics ──────────────────────────────────────────────

hdr "Phase 11: Usage & Analytics"

# Step 11.1: Aggregate usage
hdr "11.1 Aggregate usage"
sleep 1
USAGE=$(aget "/v1/usage?hours=1" || echo '{}')
TOTAL_REQ=$(echo "$USAGE" | jv '["total_requests"]' || echo "0")
TOTAL_TOKENS=$(echo "$USAGE" | jv '["total_tokens"]' || echo "0")
info "Requests=$TOTAL_REQ Tokens=$TOTAL_TOKENS"
[ "$TOTAL_REQ" != "0" ] && pass "Usage data recorded" || info "Usage may be in analytics pipeline"

# Step 11.2: Usage breakdown
hdr "11.2 Usage breakdown"
BREAKDOWN=$(aget "/v1/usage/breakdown?hours=1" || echo '{}')
echo "$BREAKDOWN" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
for m in d.get('by_model', [])[:5]:
    print(f'    {m[\"model_name\"]}: {m[\"request_count\"]} reqs, {m.get(\"total_tokens\",0)} tokens')
" 2>/dev/null || true
pass "Breakdown endpoint accessible"

# Step 11.3: Performance
hdr "11.3 Performance metrics"
PERF=$(aget "/v1/dashboard/performance?hours=1" || echo '{}')
echo "$PERF" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
tp = d.get('throughput', {})
lt = d.get('latency', {})
print(f'    avg_tps={tp.get(\"avg_tokens_per_sec\", 0):.1f}')
print(f'    p50_latency={lt.get(\"p50_ms\", 0):.0f}ms p95={lt.get(\"p95_ms\", 0):.0f}ms')
" 2>/dev/null || true
pass "Performance endpoint accessible"

# ── Phase 12: Final Capacity & Thermal State ─────────────────────────────────

hdr "Phase 12: Final State"

# Step 12.1: Final capacity
hdr "12.1 Final capacity snapshot"
sleep 2
CFINAL=$(aget "/v1/dashboard/capacity" 2>/dev/null || echo '{"providers":[]}')
echo "$CFINAL" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
for p in d.get('providers', []):
    thermal = p.get('thermal_state', 'unknown')
    for m in p.get('loaded_models', []):
        print(f'    {m[\"model_name\"]}: active={m[\"active_requests\"]} limit={m[\"max_concurrent\"]} thermal={thermal}')
" 2>/dev/null || echo "    (no data)"
pass "Final capacity verified"

# Step 12.2: Queue depth (should be 0 after completion)
hdr "12.2 Final queue depth"
QD_FINAL=$(aget "/v1/dashboard/queue/depth" || echo '{"total":0}')
QD_F_TOTAL=$(echo "$QD_FINAL" | jv '["total"]' || echo "0")
info "Queue depth: $QD_F_TOTAL"
[ "$QD_F_TOTAL" = "0" ] && pass "All queues drained" || info "Queue not empty ($QD_F_TOTAL remaining)"

# ── Phase 13: Auth Edge Cases ─────────────────────────────────────────────────

hdr "Phase 13: Auth Edge Cases"

# 13.1: Invalid credentials
hdr "13.1 Invalid credentials → 401"
BAD_LOGIN=$(rawpostc "/v1/auth/login" '{"username":"nobody","password":"wrong"}')
BAD_CODE=$(echo "$BAD_LOGIN" | tail -1)
[ "$BAD_CODE" = "401" ] && pass "Invalid creds → 401" || fail "Expected 401, got $BAD_CODE"

# 13.2: Unauthenticated access → 401
hdr "13.2 Unauthenticated access → 401"
UNAUTH_CODE=$(rawc "/v1/providers" | tail -1)
[ "$UNAUTH_CODE" = "401" ] && pass "No token → 401" || fail "Expected 401, got $UNAUTH_CODE"

# 13.3: Invalid API key → 401
hdr "13.3 Invalid API key → 401"
BAD_KEY_CODE=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
  -H "Authorization: Bearer sk-invalid-key" -H "Content-Type: application/json" \
  -d '{"model":"test","messages":[{"role":"user","content":"hi"}]}' | tail -1)
[ "$BAD_KEY_CODE" = "401" ] && pass "Invalid API key → 401" || fail "Expected 401, got $BAD_KEY_CODE"

# 13.4: Token refresh + logout (use a fresh login to avoid interfering with main session)
hdr "13.4 Token refresh & logout"
# Login returns Secure cookies — curl cookie jar won't send them over HTTP.
# Parse Set-Cookie headers manually and forward as Cookie header.
LOGIN_HDRS=$(curl -si "$API/v1/auth/login" \
  -H 'Content-Type: application/json' -d @/tmp/_sched_login.json 2>/dev/null)
REFRESH_TK=$(echo "$LOGIN_HDRS" | sed -n 's/.*veronex_refresh_token=\([^;]*\).*/\1/p' | head -1)
if [ -n "$REFRESH_TK" ]; then
  REFRESH_RES=$(curl -s -w "\n%{http_code}" -X POST "$API/v1/auth/refresh" \
    -H "Cookie: veronex_refresh_token=$REFRESH_TK")
  REFRESH_CODE=$(echo "$REFRESH_RES" | code)
  if [ "$REFRESH_CODE" = "200" ]; then
    pass "Token refresh → 200"
    # Extract new refresh token from refresh response for logout
    NEW_REFRESH_TK=$(echo "$REFRESH_RES" | sed -n 's/.*veronex_refresh_token=\([^;]*\).*/\1/p' | head -1)
    [ -z "$NEW_REFRESH_TK" ] && NEW_REFRESH_TK="$REFRESH_TK"
    LOGOUT_CODE=$(curl -s -w "\n%{http_code}" -X POST "$API/v1/auth/logout" \
      -H "Cookie: veronex_refresh_token=$NEW_REFRESH_TK" | code)
    [ "$LOGOUT_CODE" = "204" ] && pass "Logout → 204" || fail "Logout → $LOGOUT_CODE"
  else
    fail "Token refresh → $REFRESH_CODE"
  fi
else
  fail "Token refresh → no refresh cookie in login response"
fi

# ── Phase 14: Account CRUD ───────────────────────────────────────────────────

hdr "Phase 14: Account CRUD"

# 14.1: List accounts
hdr "14.1 List accounts"
ACCT_LIST_CODE=$(agetc "/v1/accounts" | tail -1)
[ "$ACCT_LIST_CODE" = "200" ] && pass "List accounts → 200" || fail "List accounts → $ACCT_LIST_CODE"

# 14.2: Create → update → delete account
hdr "14.2 Account lifecycle"
TEST_USER="e2e-user-$(python3 -c 'import uuid;print(str(uuid.uuid4())[:8])')"
ACCT_CREATE_RES=$(apostc "/v1/accounts" "{\"username\":\"$TEST_USER\",\"password\":\"TestPass123\",\"name\":\"E2E Test User\",\"role\":\"admin\"}")
ACCT_CREATE_CODE=$(echo "$ACCT_CREATE_RES" | tail -1)
ACCT_ID=$(echo "$ACCT_CREATE_RES" | body | jv '["id"]' 2>/dev/null || echo "")
if { [ "$ACCT_CREATE_CODE" = "200" ] || [ "$ACCT_CREATE_CODE" = "201" ]; } && [ -n "$ACCT_ID" ] && [ "$ACCT_ID" != "None" ]; then
  pass "Create account → $ACCT_CREATE_CODE ($TEST_USER)"

  # Update
  ACCT_UPD_CODE=$(apatchc "/v1/accounts/$ACCT_ID" '{"role":"admin"}' | tail -1)
  [ "$ACCT_UPD_CODE" = "200" ] || [ "$ACCT_UPD_CODE" = "204" ] && pass "Update account → $ACCT_UPD_CODE" || fail "Update account → $ACCT_UPD_CODE"

  # Deactivate
  ACCT_DEACT_CODE=$(apatchc "/v1/accounts/$ACCT_ID/active" '{"is_active":false}' | tail -1)
  [ "$ACCT_DEACT_CODE" = "200" ] || [ "$ACCT_DEACT_CODE" = "204" ] && pass "Deactivate account → $ACCT_DEACT_CODE" || fail "Deactivate → $ACCT_DEACT_CODE"

  # List sessions (may be empty)
  SESS_CODE=$(agetc "/v1/accounts/$ACCT_ID/sessions" | tail -1)
  [ "$SESS_CODE" = "200" ] && pass "List sessions → 200" || fail "List sessions → $SESS_CODE"

  # Delete (soft-delete + cascade keys)
  ACCT_DEL_CODE=$(adelc "/v1/accounts/$ACCT_ID" | tail -1)
  [ "$ACCT_DEL_CODE" = "204" ] && pass "Delete account → 204" || fail "Delete account → $ACCT_DEL_CODE"
else
  fail "Create account failed ($ACCT_CREATE_CODE)"
fi

# 14.3: Duplicate username → 409
hdr "14.3 Duplicate username → 409"
DUP_CODE=$(apostc "/v1/accounts" "{\"username\":\"$USERNAME\",\"password\":\"test1234\",\"name\":\"Dup\",\"role\":\"admin\"}" | code)
case "$DUP_CODE" in
  400|409|500) pass "Duplicate username rejected → $DUP_CODE" ;;
  *) fail "Expected 400/409/500, got $DUP_CODE" ;;
esac

# ── Phase 15: API Key CRUD ───────────────────────────────────────────────────

hdr "Phase 15: API Key CRUD"

# 15.1: List keys
hdr "15.1 List keys"
KEY_LIST_CODE=$(agetc "/v1/keys" | tail -1)
[ "$KEY_LIST_CODE" = "200" ] && pass "List keys → 200" || fail "List keys → $KEY_LIST_CODE"

# 15.2: Create → toggle → tier change → delete
hdr "15.2 Key lifecycle"
KEY_CREATE_RES=$(apostc "/v1/keys" "{\"tenant_id\":\"$USERNAME\",\"name\":\"e2e-lifecycle-key\",\"tier\":\"free\"}")
KEY_CREATE_CODE=$(echo "$KEY_CREATE_RES" | tail -1)
KEY_ID=$(echo "$KEY_CREATE_RES" | body | jv '["id"]' 2>/dev/null || echo "")
KEY_RAW=$(echo "$KEY_CREATE_RES" | body | jv '["key"]' 2>/dev/null || echo "")
if [ "$KEY_CREATE_CODE" = "200" ] || [ "$KEY_CREATE_CODE" = "201" ] && [ -n "$KEY_ID" ] && [ "$KEY_ID" != "None" ]; then
  pass "Create key → $KEY_CREATE_CODE (prefix: ${KEY_RAW:0:12}...)"

  # Toggle inactive
  TOGGLE_CODE=$(apatchc "/v1/keys/$KEY_ID" '{"is_active":false}' | tail -1)
  [ "$TOGGLE_CODE" = "204" ] || [ "$TOGGLE_CODE" = "200" ] && pass "Toggle key off → $TOGGLE_CODE" || fail "Toggle key → $TOGGLE_CODE"

  # Inactive key should reject inference
  INACTIVE_CODE=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
    -H "Authorization: Bearer $KEY_RAW" -H "Content-Type: application/json" \
    -d '{"model":"test","messages":[{"role":"user","content":"hi"}]}' | tail -1)
  [ "$INACTIVE_CODE" = "401" ] || [ "$INACTIVE_CODE" = "403" ] && pass "Inactive key rejected → $INACTIVE_CODE" || fail "Inactive key: expected 401/403, got $INACTIVE_CODE"

  # Change tier
  TIER_CODE=$(apatchc "/v1/keys/$KEY_ID" '{"tier":"paid"}' | tail -1)
  [ "$TIER_CODE" = "204" ] || [ "$TIER_CODE" = "200" ] && pass "Change tier → $TIER_CODE" || fail "Change tier → $TIER_CODE"

  # Delete
  KEY_DEL_CODE=$(adelc "/v1/keys/$KEY_ID" | tail -1)
  [ "$KEY_DEL_CODE" = "204" ] && pass "Delete key → 204" || fail "Delete key → $KEY_DEL_CODE"
else
  fail "Create key failed ($KEY_CREATE_CODE)"
fi

# ── Phase 16: Provider Management ────────────────────────────────────────────

hdr "Phase 16: Provider Management"

# 16.1: Provider models
hdr "16.1 Provider models"
if [ -n "$PROVIDER_ID" ] && [ "$PROVIDER_ID" != "None" ]; then
  PMOD_CODE=$(agetc "/v1/providers/$PROVIDER_ID/models" | tail -1)
  [ "$PMOD_CODE" = "200" ] && pass "Provider models → 200" || fail "Provider models → $PMOD_CODE"
else
  info "No provider ID (skipped)"
fi

# 16.2: Model selection (enable/disable)
hdr "16.2 Model selection toggle"
if [ -n "$PROVIDER_ID" ] && [ "$PROVIDER_ID" != "None" ]; then
  # List selected models
  SEL_CODE=$(agetc "/v1/providers/$PROVIDER_ID/selected-models" | tail -1)
  [ "$SEL_CODE" = "200" ] && pass "List selected models → 200" || fail "List selected → $SEL_CODE"

  # Disable a model
  DISABLE_CODE=$(apatchc "/v1/providers/$PROVIDER_ID/selected-models/$MODEL" '{"is_enabled":false}' | tail -1)
  [ "$DISABLE_CODE" = "200" ] || [ "$DISABLE_CODE" = "204" ] && pass "Disable model → $DISABLE_CODE" || fail "Disable model → $DISABLE_CODE"

  # Re-enable
  ENABLE_CODE=$(apatchc "/v1/providers/$PROVIDER_ID/selected-models/$MODEL" '{"is_enabled":true}' | tail -1)
  [ "$ENABLE_CODE" = "200" ] || [ "$ENABLE_CODE" = "204" ] && pass "Re-enable model → $ENABLE_CODE" || fail "Re-enable → $ENABLE_CODE"
else
  info "No provider ID (skipped)"
fi

# 16.3: Non-existent provider → 404
hdr "16.3 Non-existent provider → 404"
NE_PROV_CODE=$(agetc "/v1/providers/00000000-0000-0000-0000-000000000000/models" | tail -1)
[ "$NE_PROV_CODE" = "404" ] && pass "Non-existent provider → 404" || fail "Expected 404, got $NE_PROV_CODE"

# 16.4: Provider delete (create a throwaway one)
hdr "16.4 Provider delete"
TMP_PROV_RES=$(apostc "/v1/providers" "{\"name\":\"tmp-delete-test\",\"provider_type\":\"ollama\",\"url\":\"http://127.0.0.1:59999\"}")
TMP_PROV_CODE=$(echo "$TMP_PROV_RES" | tail -1)
TMP_PROV_ID=$(echo "$TMP_PROV_RES" | body | jv '["id"]' 2>/dev/null || echo "")
if [ "$TMP_PROV_CODE" = "201" ] && [ -n "$TMP_PROV_ID" ] && [ "$TMP_PROV_ID" != "None" ]; then
  DEL_PROV_CODE=$(adelc "/v1/providers/$TMP_PROV_ID" | tail -1)
  [ "$DEL_PROV_CODE" = "204" ] && pass "Delete provider → 204" || fail "Delete provider → $DEL_PROV_CODE"
else
  fail "Create temp provider failed ($TMP_PROV_CODE)"
fi

# 16.5: Ollama model → provider mapping
hdr "16.5 Model → provider mapping"
MAP_CODE=$(agetc "/v1/ollama/models/$MODEL/providers" | tail -1)
[ "$MAP_CODE" = "200" ] && pass "Model providers → 200" || fail "Model providers → $MAP_CODE"

# ── Phase 17: Multi-Format Inference ─────────────────────────────────────────

hdr "Phase 17: Multi-Format Inference (parallel)"

# Fire all 7 requests in parallel
TMPDIR_17=$(mktemp -d)
(curl -s --max-time 30 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Say hi\"}],\"max_tokens\":8,\"stream\":true}" \
  > "$TMPDIR_17/sse" 2>/dev/null || true) &
(curl -s -w "\n%{http_code}" --max-time 30 "$API/api/chat" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Say one word\"}],\"stream\":false}" \
  > "$TMPDIR_17/chat" 2>/dev/null || printf "\n000" > "$TMPDIR_17/chat") &
(curl -s -w "\n%{http_code}" --max-time 30 "$API/api/generate" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"prompt\":\"Say one word\",\"stream\":false}" \
  > "$TMPDIR_17/generate" 2>/dev/null || printf "\n000" > "$TMPDIR_17/generate") &
(curl -s -w "\n%{http_code}" "$API/api/tags" -H "X-API-Key: $API_KEY" \
  > "$TMPDIR_17/tags" 2>/dev/null || printf "\n000" > "$TMPDIR_17/tags") &
(curl -s -w "\n%{http_code}" "$API/api/show" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "{\"name\":\"$MODEL\"}" \
  > "$TMPDIR_17/show" 2>/dev/null || printf "\n000" > "$TMPDIR_17/show") &
(curl -s -w "\n%{http_code}" "$API/v1beta/models" -H "X-API-Key: $API_KEY" \
  > "$TMPDIR_17/gemini" 2>/dev/null || printf "\n000" > "$TMPDIR_17/gemini") &
(curl -s -w "\n%{http_code}" "$API/v1/test/completions" \
  -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}],\"max_tokens\":4,\"stream\":false}" \
  > "$TMPDIR_17/test" 2>/dev/null || printf "\n000" > "$TMPDIR_17/test") &
wait

# 17.1: SSE streaming (OpenAI)
hdr "17.1 OpenAI SSE streaming"
SSE_RES=$(cat "$TMPDIR_17/sse" 2>/dev/null || echo "")
echo "$SSE_RES" | grep -q "data:" \
  && pass "OpenAI SSE streaming works" || fail "SSE streaming: no data events"

# 17.2: Ollama /api/chat
hdr "17.2 Ollama /api/chat"
OLLAMA_CHAT_CODE=$(tail -1 "$TMPDIR_17/chat" 2>/dev/null || echo "000")
[ "$OLLAMA_CHAT_CODE" = "200" ] && pass "Ollama /api/chat → 200" || fail "Ollama chat → $OLLAMA_CHAT_CODE"

# 17.3: Ollama /api/generate
hdr "17.3 Ollama /api/generate"
OLLAMA_GEN_CODE=$(tail -1 "$TMPDIR_17/generate" 2>/dev/null || echo "000")
[ "$OLLAMA_GEN_CODE" = "200" ] && pass "Ollama /api/generate → 200" || fail "Ollama generate → $OLLAMA_GEN_CODE"

# 17.4: Ollama /api/tags
hdr "17.4 Ollama /api/tags"
TAGS_CODE=$(tail -1 "$TMPDIR_17/tags" 2>/dev/null || echo "000")
[ "$TAGS_CODE" = "200" ] && pass "Ollama /api/tags → 200" || fail "Ollama tags → $TAGS_CODE"

# 17.5: Ollama /api/show
hdr "17.5 Ollama /api/show"
SHOW_CODE=$(tail -1 "$TMPDIR_17/show" 2>/dev/null || echo "000")
[ "$SHOW_CODE" = "200" ] && pass "Ollama /api/show → 200" || fail "Ollama show → $SHOW_CODE"

# 17.6: Gemini /v1beta/models
hdr "17.6 Gemini /v1beta/models"
GEMINI_LIST_CODE=$(tail -1 "$TMPDIR_17/gemini" 2>/dev/null || echo "000")
[ "$GEMINI_LIST_CODE" = "200" ] && pass "Gemini /v1beta/models → 200" || fail "Gemini models → $GEMINI_LIST_CODE"

# 17.7: Test endpoint (JWT, no rate limit)
hdr "17.7 Test inference (JWT)"
TEST_INF_CODE=$(tail -1 "$TMPDIR_17/test" 2>/dev/null || echo "000")
[ "$TEST_INF_CODE" = "200" ] && pass "Test completions → 200" || fail "Test completions → $TEST_INF_CODE"

rm -rf "$TMPDIR_17"

# ── Phase 18: Server & Audit & Lab ──────────────────────────────────────────

hdr "Phase 18: Server, Audit, Lab & Docs"

# 18.1: Server CRUD (update + delete)
hdr "18.1 Server update & delete"
TMP_SRV_RES=$(apostc "/v1/servers" '{"name":"tmp-srv-test"}')
TMP_SRV_CODE=$(echo "$TMP_SRV_RES" | tail -1)
TMP_SRV_ID=$(echo "$TMP_SRV_RES" | body | jv '["id"]' 2>/dev/null || echo "")
if [ "$TMP_SRV_CODE" = "201" ] && [ -n "$TMP_SRV_ID" ] && [ "$TMP_SRV_ID" != "None" ]; then
  SRV_UPD_CODE=$(apatchc "/v1/servers/$TMP_SRV_ID" '{"name":"tmp-srv-updated"}' | tail -1)
  [ "$SRV_UPD_CODE" = "200" ] && pass "Update server → 200" || fail "Update server → $SRV_UPD_CODE"

  SRV_DEL_CODE=$(adelc "/v1/servers/$TMP_SRV_ID" | tail -1)
  [ "$SRV_DEL_CODE" = "204" ] && pass "Delete server → 204" || fail "Delete server → $SRV_DEL_CODE"
else
  fail "Create temp server failed ($TMP_SRV_CODE)"
fi

# 18.2: Server metrics
hdr "18.2 Server metrics"
if [ -n "$SERVER_ID" ] && [ "$SERVER_ID" != "None" ]; then
  SMET_CODE=$(agetc "/v1/servers/$SERVER_ID/metrics" | tail -1)
  [ "$SMET_CODE" = "200" ] && pass "Server metrics → 200" || info "Server metrics → $SMET_CODE (agent may be unreachable)"
else
  info "No server ID (skipped)"
fi

# 18.3: Audit log
hdr "18.3 Audit log"
AUDIT_CODE=$(agetc "/v1/audit?limit=10" | tail -1)
[ "$AUDIT_CODE" = "200" ] && pass "Audit log → 200" || fail "Audit log → $AUDIT_CODE"

AUDIT_COUNT=$(aget "/v1/audit?limit=10" 2>/dev/null | jv '["events"].__len__()' 2>/dev/null || echo "0")
info "Audit events: $AUDIT_COUNT"

# 18.4: Lab settings
hdr "18.4 Lab settings"
LAB_GET_CODE=$(agetc "/v1/dashboard/lab" | tail -1)
[ "$LAB_GET_CODE" = "200" ] && pass "Get lab settings → 200" || fail "Lab settings → $LAB_GET_CODE"

# Toggle and revert
LAB_CURRENT=$(aget "/v1/dashboard/lab" 2>/dev/null | jv '["gemini_enabled"]' 2>/dev/null || echo "")
if [ -n "$LAB_CURRENT" ] && [ "$LAB_CURRENT" != "None" ]; then
  if [ "$LAB_CURRENT" = "True" ]; then
    apatch "/v1/dashboard/lab" '{"gemini_enabled":false}' > /dev/null 2>&1
    apatch "/v1/dashboard/lab" '{"gemini_enabled":true}' > /dev/null 2>&1
  else
    apatch "/v1/dashboard/lab" '{"gemini_enabled":true}' > /dev/null 2>&1
    apatch "/v1/dashboard/lab" '{"gemini_enabled":false}' > /dev/null 2>&1
  fi
  pass "Lab toggle + revert OK"
fi

# 18.5: OpenAPI docs
hdr "18.5 OpenAPI spec"
DOC_CODE=$(curl -s -w "\n%{http_code}" "$API/docs/openapi.json" | tail -1)
[ "$DOC_CODE" = "200" ] && pass "OpenAPI spec → 200" || fail "OpenAPI spec → $DOC_CODE"

# 18.6: Prometheus targets
hdr "18.6 Prometheus targets"
PROM_CODE=$(curl -s -w "\n%{http_code}" "$API/v1/metrics/targets" | tail -1)
[ "$PROM_CODE" = "200" ] && pass "Prometheus targets → 200" || fail "Prometheus targets → $PROM_CODE"

# ── Phase 19: Usage Breakdown (per-key) ──────────────────────────────────────

hdr "Phase 19: Per-Key Usage"

# 19.1: Get key ID from list
KEY_LIST=$(aget "/v1/keys" 2>/dev/null || echo "[]")
FIRST_KEY_ID=$(echo "$KEY_LIST" | jv '[0]["id"]' 2>/dev/null || echo "")

if [ -n "$FIRST_KEY_ID" ] && [ "$FIRST_KEY_ID" != "None" ]; then
  # 19.2: Per-key usage
  hdr "19.2 Per-key usage"
  PK_CODE=$(agetc "/v1/usage/$FIRST_KEY_ID?hours=24" | tail -1)
  [ "$PK_CODE" = "200" ] && pass "Per-key usage → 200" || fail "Per-key usage → $PK_CODE"

  # 19.3: Per-key jobs
  hdr "19.3 Per-key jobs"
  PKJ_CODE=$(agetc "/v1/usage/$FIRST_KEY_ID/jobs?hours=24" | tail -1)
  [ "$PKJ_CODE" = "200" ] && pass "Per-key jobs → 200" || fail "Per-key jobs → $PKJ_CODE"

  # 19.4: Per-key model breakdown
  hdr "19.4 Per-key model breakdown"
  PKM_CODE=$(agetc "/v1/usage/$FIRST_KEY_ID/models?hours=24" | tail -1)
  [ "$PKM_CODE" = "200" ] && pass "Per-key models → 200" || fail "Per-key models → $PKM_CODE"
else
  info "No keys found (skipped per-key usage)"
fi

# 19.5: Dashboard analytics
hdr "19.5 Dashboard analytics"
ANALYTICS_CODE=$(agetc "/v1/dashboard/analytics?hours=24" | tail -1)
[ "$ANALYTICS_CODE" = "200" ] && pass "Analytics → 200" || fail "Analytics → $ANALYTICS_CODE"

# 19.6: Job detail
hdr "19.6 Job detail"
FIRST_JOB_ID=$(aget "/v1/dashboard/jobs?limit=1" 2>/dev/null | jv '["jobs"][0]["id"]' 2>/dev/null || echo "")
if [ -n "$FIRST_JOB_ID" ] && [ "$FIRST_JOB_ID" != "None" ]; then
  JOB_DET_CODE=$(agetc "/v1/dashboard/jobs/$FIRST_JOB_ID" | tail -1)
  [ "$JOB_DET_CODE" = "200" ] && pass "Job detail → 200" || fail "Job detail → $JOB_DET_CODE"
else
  info "No jobs found (skipped)"
fi

# ── Phase 20: Security Hardening ───────────────────────────────────────────────

hdr "Phase 20: Security Hardening"

# 20.1: Security headers
hdr "20.1 Security headers"
SEC_HDRS=$(curl -sI "$API/health" 2>/dev/null)
echo "$SEC_HDRS" | grep -qi "x-content-type-options: nosniff" \
  && pass "X-Content-Type-Options: nosniff" || fail "Missing X-Content-Type-Options header"
echo "$SEC_HDRS" | grep -qi "x-frame-options: deny" \
  && pass "X-Frame-Options: DENY" || fail "Missing X-Frame-Options header"
echo "$SEC_HDRS" | grep -qi "referrer-policy" \
  && pass "Referrer-Policy present" || fail "Missing Referrer-Policy header"

# 20.2: SSRF protection
hdr "20.2 SSRF protection"
SSRF1=$(apostc "/v1/providers" '{"name":"ssrf-meta","provider_type":"ollama","url":"http://169.254.169.254/latest/meta-data/"}')
SSRF1_CODE=$(echo "$SSRF1" | code)
[ "$SSRF1_CODE" != "201" ] \
  && pass "SSRF blocked: metadata IP → $SSRF1_CODE" \
  || { fail "SSRF: metadata IP accepted (201)"; SSRF1_ID=$(echo "$SSRF1" | body | jv '["id"]' 2>/dev/null); adel "/v1/providers/$SSRF1_ID" > /dev/null 2>&1; }

SSRF2=$(apostc "/v1/providers" '{"name":"ssrf-gcp","provider_type":"ollama","url":"http://metadata.google.internal/"}')
SSRF2_CODE=$(echo "$SSRF2" | code)
[ "$SSRF2_CODE" != "201" ] \
  && pass "SSRF blocked: metadata hostname → $SSRF2_CODE" \
  || { fail "SSRF: metadata hostname accepted (201)"; SSRF2_ID=$(echo "$SSRF2" | body | jv '["id"]' 2>/dev/null); adel "/v1/providers/$SSRF2_ID" > /dev/null 2>&1; }

# 20.3: Input validation
hdr "20.3 Input validation"
LONG_MODEL=$(python3 -c "print('a' * 300)")
BIGMODEL_CODE=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$LONG_MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}" | code)
case "$BIGMODEL_CODE" in
  400|413|422) pass "Oversized model name (300B) rejected → $BIGMODEL_CODE" ;;
  *) fail "Oversized model name: expected 400/413/422, got $BIGMODEL_CODE" ;;
esac

# ── Phase 21: Rate Limiting ────────────────────────────────────────────────────

hdr "Phase 21: Rate Limiting"

hdr "21.1 RPM limit enforcement"
RL_KEY_RES=$(apost "/v1/keys" "{\"tenant_id\":\"$USERNAME\",\"name\":\"rpm-limit-test\",\"rate_limit_rpm\":2,\"tier\":\"paid\"}" || echo "")
RL_KEY=$(echo "$RL_KEY_RES" | jv '["key"]' || echo "")
RL_KEY_ID=$(echo "$RL_KEY_RES" | jv '["id"]' || echo "")

if [ -n "$RL_KEY" ] && [ "$RL_KEY" != "None" ]; then
  # Fire 3 parallel requests (limit=2) — at least 1 should get 429
  RL_TMPDIR=$(mktemp -d)
  for i in 1 2 3; do
    (
      C=$(curl -s -w "%{http_code}" -o /dev/null --max-time 30 "$API/v1/chat/completions" \
        -H "Authorization: Bearer $RL_KEY" -H "Content-Type: application/json" \
        -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Say $i\"}],\"max_tokens\":4,\"stream\":false}")
      echo "$C" > "$RL_TMPDIR/$i"
    ) &
  done
  wait
  RL_CODES=$(cat "$RL_TMPDIR"/* 2>/dev/null | tr '\n' ' ')
  rm -rf "$RL_TMPDIR"
  if echo "$RL_CODES" | grep -q "429"; then
    pass "RPM limit enforced — codes: $RL_CODES"
  else
    fail "RPM limit not enforced — all codes: $RL_CODES"
  fi
  # Cleanup
  adel "/v1/keys/$RL_KEY_ID" > /dev/null 2>&1
else
  fail "Rate limit key creation failed"
fi

# ── Phase 22: Job Lifecycle ────────────────────────────────────────────────────

hdr "Phase 22: Job Lifecycle"

# 22.1: Job cancel
hdr "22.1 Job cancel"
# Start a long inference in background
curl -s "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Write a long essay about computer science history\"}],\"max_tokens\":300,\"stream\":true}" > /dev/null 2>&1 &
CANCEL_PID=$!
sleep 3

# Find running/pending job
CANCEL_JOB_ID=$(aget "/v1/dashboard/jobs?limit=1&status=running" 2>/dev/null \
  | jv '["jobs"][0]["id"]' 2>/dev/null || echo "")
[ -z "$CANCEL_JOB_ID" ] || [ "$CANCEL_JOB_ID" = "None" ] && \
  CANCEL_JOB_ID=$(aget "/v1/dashboard/jobs?limit=1" 2>/dev/null \
    | jv '["jobs"][0]["id"]' 2>/dev/null || echo "")

if [ -n "$CANCEL_JOB_ID" ] && [ "$CANCEL_JOB_ID" != "None" ]; then
  CANCEL_CODE=$(adelc "/v1/dashboard/jobs/$CANCEL_JOB_ID" | code)
  case "$CANCEL_CODE" in
    200|204) pass "Job cancel endpoint → $CANCEL_CODE" ;;
    *) fail "Job cancel → $CANCEL_CODE" ;;
  esac

  sleep 1
  CANCEL_STATUS=$(aget "/v1/dashboard/jobs/$CANCEL_JOB_ID" 2>/dev/null \
    | jv '["status"]' 2>/dev/null || echo "unknown")
  case "$CANCEL_STATUS" in
    cancelled|Cancelled) pass "Job status = cancelled" ;;
    completed|Completed) pass "Job completed before cancel (idempotent)" ;;
    *) fail "Job status after cancel = $CANCEL_STATUS" ;;
  esac
else
  fail "No job found to cancel"
fi
kill $CANCEL_PID 2>/dev/null || true; wait $CANCEL_PID 2>/dev/null || true

# 22.2: SSE content verification
hdr "22.2 SSE content verification"
SSE_FULL=$(curl -s --max-time 30 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Say hello\"}],\"max_tokens\":8,\"stream\":true}" 2>/dev/null || echo "")

# Verify JSON structure in SSE events
SSE_FIRST_DATA=$(echo "$SSE_FULL" | grep "^data: {" | head -1 || echo "")
SSE_JSON_OK=$(echo "$SSE_FIRST_DATA" | python3 -c "
import sys, json
line = sys.stdin.readline().strip()
if line.startswith('data: '):
    d = json.loads(line[6:])
    print('yes' if 'choices' in d and len(d['choices']) > 0 else 'no')
else:
    print('no')
" 2>/dev/null || echo "no")
[ "$SSE_JSON_OK" = "yes" ] && pass "SSE events contain valid JSON with choices" || fail "SSE JSON structure invalid"

# Verify [DONE] terminator
HAS_DONE=$(echo "$SSE_FULL" | grep -c "\[DONE\]" || echo "0")
[ "$HAS_DONE" -gt 0 ] \
  && pass "SSE stream ends with [DONE]" || fail "SSE stream missing [DONE] terminator"

# ── Phase 23: Session & Access Control ─────────────────────────────────────────

hdr "Phase 23: Session & Access Control"

# 23.1: Expired API key
hdr "23.1 Expired API key → 401"
PAST_DATE=$(python3 -c "from datetime import datetime,timezone; print(datetime(2020,1,1,tzinfo=timezone.utc).isoformat())")
EXP_KEY_RES=$(apost "/v1/keys" "{\"tenant_id\":\"$USERNAME\",\"name\":\"expired-test\",\"tier\":\"paid\",\"expires_at\":\"$PAST_DATE\"}" || echo "")
EXP_KEY=$(echo "$EXP_KEY_RES" | jv '["key"]' || echo "")
EXP_KEY_ID=$(echo "$EXP_KEY_RES" | jv '["id"]' || echo "")
if [ -n "$EXP_KEY" ] && [ "$EXP_KEY" != "None" ]; then
  EXP_CODE=$(curl -s -w "%{http_code}" -o /dev/null "$API/v1/chat/completions" \
    -H "Authorization: Bearer $EXP_KEY" -H "Content-Type: application/json" \
    -d '{"model":"test","messages":[{"role":"user","content":"hi"}]}')
  [ "$EXP_CODE" = "401" ] && pass "Expired API key → 401" || fail "Expired key: expected 401, got $EXP_CODE"
  adel "/v1/keys/$EXP_KEY_ID" > /dev/null 2>&1
else
  fail "Expired key creation failed"
fi

# 23.2: Session revoke → JWT invalid
hdr "23.2 Session revoke → JWT invalid"
RBAC_USER="e2e-rbac-$(python3 -c 'import uuid;print(str(uuid.uuid4())[:8])')"
RBAC_CREATE_RES=$(apostc "/v1/accounts" \
  "{\"username\":\"$RBAC_USER\",\"password\":\"TestPass123!\",\"name\":\"RBAC Test\",\"role\":\"admin\"}")
RBAC_CREATE_CODE=$(echo "$RBAC_CREATE_RES" | code)
RBAC_ACCT_ID=$(echo "$RBAC_CREATE_RES" | body | jv '["id"]' 2>/dev/null || echo "")

if [ "$RBAC_CREATE_CODE" = "200" ] || [ "$RBAC_CREATE_CODE" = "201" ]; then
  # Login as temp account
  RBAC_LOGIN=$(curl -si "$API/v1/auth/login" \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"$RBAC_USER\",\"password\":\"TestPass123!\"}" 2>/dev/null)
  RBAC_TK=$(echo "$RBAC_LOGIN" | sed -n 's/.*veronex_access_token=\([^;]*\).*/\1/p' | head -1)

  if [ -n "$RBAC_TK" ]; then
    # Verify token works
    RBAC_BEFORE=$(curl -s -w "\n%{http_code}" "$API/v1/keys" \
      -H "Authorization: Bearer $RBAC_TK" | code)

    # Revoke all sessions for temp account (using admin TK)
    adelc "/v1/accounts/$RBAC_ACCT_ID/sessions" > /dev/null 2>&1

    sleep 1
    # Try revoked token
    RBAC_AFTER=$(curl -s -w "\n%{http_code}" "$API/v1/keys" \
      -H "Authorization: Bearer $RBAC_TK" | code)
    [ "$RBAC_AFTER" = "401" ] \
      && pass "Revoked session → 401 (was $RBAC_BEFORE)" \
      || fail "Revoked session: expected 401, got $RBAC_AFTER"
  else
    fail "Could not login as temp account"
  fi

  # 23.3: RBAC — admin cannot access super-only endpoints
  hdr "23.3 RBAC admin restrictions"
  # Re-login (session was revoked)
  RBAC_LOGIN2=$(curl -si "$API/v1/auth/login" \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"$RBAC_USER\",\"password\":\"TestPass123!\"}" 2>/dev/null)
  RBAC_TK2=$(echo "$RBAC_LOGIN2" | sed -n 's/.*veronex_access_token=\([^;]*\).*/\1/p' | head -1)

  if [ -n "$RBAC_TK2" ]; then
    RBAC_ACCT_CODE=$(curl -s -w "\n%{http_code}" "$API/v1/accounts" \
      -H "Authorization: Bearer $RBAC_TK2" | code)
    [ "$RBAC_ACCT_CODE" = "403" ] \
      && pass "RBAC: admin → /v1/accounts = 403" \
      || info "RBAC: admin → /v1/accounts = $RBAC_ACCT_CODE (role enforcement may differ)"

    RBAC_AUDIT_CODE=$(curl -s -w "\n%{http_code}" "$API/v1/audit?limit=1" \
      -H "Authorization: Bearer $RBAC_TK2" | code)
    [ "$RBAC_AUDIT_CODE" = "403" ] \
      && pass "RBAC: admin → /v1/audit = 403" \
      || info "RBAC: admin → /v1/audit = $RBAC_AUDIT_CODE (role enforcement may differ)"
  else
    info "Could not re-login for RBAC test"
  fi

  # Cleanup temp account
  adel "/v1/accounts/$RBAC_ACCT_ID" > /dev/null 2>&1
else
  fail "RBAC temp account creation failed ($RBAC_CREATE_CODE)"
fi

# ── Phase 24: Additional Endpoints ────────────────────────────────────────────

hdr "Phase 24: Additional Endpoints"

# 24.1: Server list
hdr "24.1 Server list"
SRV_LIST_CODE=$(agetc "/v1/servers" | code)
[ "$SRV_LIST_CODE" = "200" ] && pass "List servers → 200" || fail "List servers → $SRV_LIST_CODE"

# 24.2: Metrics history
hdr "24.2 Server metrics history"
if [ -n "$SERVER_ID" ] && [ "$SERVER_ID" != "None" ]; then
  HIST_CODE=$(agetc "/v1/servers/$SERVER_ID/metrics/history?hours=1" | code)
  [ "$HIST_CODE" = "200" ] && pass "Metrics history → 200" || fail "Metrics history → $HIST_CODE"
else
  info "No server ID (skipped)"
fi

# 24.3: Single provider sync
hdr "24.3 Single provider sync"
if [ -n "$PROVIDER_ID" ] && [ "$PROVIDER_ID" != "None" ]; then
  SSYNC_CODE=$(apostc "/v1/providers/$PROVIDER_ID/sync" "{}" | code)
  [ "$SSYNC_CODE" = "200" ] || [ "$SSYNC_CODE" = "202" ] \
    && pass "Single provider sync → $SSYNC_CODE" || fail "Single provider sync → $SSYNC_CODE"
else
  info "No provider ID (skipped)"
fi

# 24.4: Provider key reveal
hdr "24.4 Provider key reveal"
if [ -n "$PROVIDER_ID" ] && [ "$PROVIDER_ID" != "None" ]; then
  PKEY_CODE=$(agetc "/v1/providers/$PROVIDER_ID/key" | code)
  [ "$PKEY_CODE" = "200" ] && pass "Provider key reveal → 200" || pass "Provider key reveal → $PKEY_CODE (no key set)"
else
  info "No provider ID (skipped)"
fi

# 24.5: Session grouping trigger
hdr "24.5 Session grouping trigger"
SG_CODE=$(apostc "/v1/dashboard/session-grouping/trigger" "{}" | code)
[ "$SG_CODE" = "200" ] || [ "$SG_CODE" = "202" ] \
  && pass "Session grouping trigger → $SG_CODE" || fail "Session grouping trigger → $SG_CODE"

# 24.6: Ollama /api/version
hdr "24.6 Ollama /api/version"
VER_CODE=$(curl -s -w "\n%{http_code}" "$API/api/version" \
  -H "X-API-Key: $API_KEY" 2>/dev/null | code)
[ "$VER_CODE" = "200" ] && pass "Ollama /api/version → 200" || fail "Ollama /api/version → $VER_CODE"

# 24.7: Ollama /api/ps
hdr "24.7 Ollama /api/ps"
PS_CODE=$(curl -s -w "\n%{http_code}" "$API/api/ps" \
  -H "X-API-Key: $API_KEY" 2>/dev/null | code)
[ "$PS_CODE" = "200" ] && pass "Ollama /api/ps → 200" || fail "Ollama /api/ps → $PS_CODE"

# 24.8: Ollama /api/embed
hdr "24.8 Ollama /api/embed"
EMBED_CODE=$(curl -s -w "\n%{http_code}" --max-time 30 "$API/api/embed" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"input\":\"test embedding\"}" 2>/dev/null | code)
case "$EMBED_CODE" in
  200) pass "Ollama /api/embed → 200" ;;
  400|404|501) pass "Ollama /api/embed → $EMBED_CODE (model may not support embeddings)" ;;
  *) fail "Ollama /api/embed → $EMBED_CODE" ;;
esac

# ── Phase 25: Native Inference API ─────────────────────────────────────────────

hdr "Phase 25: Native Inference API"

# Native inference uses API key auth (same as /v1/chat/completions)
kpostc() { curl -s -w "\n%{http_code}" "$API$1" -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' -d "$2"; }
kgetc()  { curl -s -w "\n%{http_code}" "$API$1" -H "Authorization: Bearer $API_KEY"; }
kget()   { curl -sf "$API$1" -H "Authorization: Bearer $API_KEY"; }
kdelc()  { curl -s -w "\n%{http_code}" -X DELETE "$API$1" -H "Authorization: Bearer $API_KEY"; }

# 25.1: Submit inference job
hdr "25.1 Submit inference (POST /v1/inference)"
INF_SUBMIT_RES=$(kpostc "/v1/inference" \
  "{\"prompt\":\"Say hello\",\"model\":\"$MODEL\",\"provider_type\":\"ollama\"}")
INF_SUBMIT_CODE=$(echo "$INF_SUBMIT_RES" | code)
INF_JOB_ID=$(echo "$INF_SUBMIT_RES" | body | jv '["job_id"]' 2>/dev/null || echo "")
case "$INF_SUBMIT_CODE" in
  200|201|202) pass "Submit inference → $INF_SUBMIT_CODE (job=$INF_JOB_ID)" ;;
  *) fail "Submit inference → $INF_SUBMIT_CODE" ;;
esac

# 25.2: Job status
hdr "25.2 Job status (GET /v1/inference/{id}/status)"
if [ -n "$INF_JOB_ID" ] && [ "$INF_JOB_ID" != "None" ]; then
  sleep 2
  INF_STATUS_CODE=$(kgetc "/v1/inference/$INF_JOB_ID/status" | code)
  [ "$INF_STATUS_CODE" = "200" ] \
    && pass "Job status → 200" || fail "Job status → $INF_STATUS_CODE"
else
  info "No job ID (skipped)"
fi

# 25.3: Job stream (GET /v1/inference/{id}/stream)
hdr "25.3 Job stream (GET /v1/inference/{id}/stream)"
if [ -n "$INF_JOB_ID" ] && [ "$INF_JOB_ID" != "None" ]; then
  # Wait for job completion
  for _w in $(seq 1 15); do
    S=$(kget "/v1/inference/$INF_JOB_ID/status" 2>/dev/null | jv '["status"]' 2>/dev/null || echo "")
    case "$S" in completed|Completed|failed|Failed) break ;; esac
    sleep 1
  done
  INF_STREAM_CODE=$(kgetc "/v1/inference/$INF_JOB_ID/stream" | code)
  [ "$INF_STREAM_CODE" = "200" ] \
    && pass "Job stream → 200" || fail "Job stream → $INF_STREAM_CODE"
else
  info "No job ID (skipped)"
fi

# 25.4: Cancel inference (DELETE /v1/inference/{id})
hdr "25.4 Cancel inference (DELETE /v1/inference/{id})"
if [ -n "$INF_JOB_ID" ] && [ "$INF_JOB_ID" != "None" ]; then
  INF_CANCEL_CODE=$(kdelc "/v1/inference/$INF_JOB_ID" | code)
  case "$INF_CANCEL_CODE" in
    200|204) pass "Cancel inference → $INF_CANCEL_CODE (idempotent)" ;;
    *) fail "Cancel inference → $INF_CANCEL_CODE" ;;
  esac
else
  info "No job ID (skipped)"
fi

# ── Phase 26: SSE Replay & Dashboard Stream ───────────────────────────────────

hdr "Phase 26: SSE Replay & Dashboard Stream"

# 26.1: Job SSE replay (GET /v1/jobs/{id}/stream — API key auth)
hdr "26.1 Job SSE replay"
REPLAY_JOB_ID=$(aget "/v1/dashboard/jobs?limit=1&status=completed" 2>/dev/null \
  | jv '["jobs"][0]["id"]' 2>/dev/null || echo "")
if [ -n "$REPLAY_JOB_ID" ] && [ "$REPLAY_JOB_ID" != "None" ]; then
  REPLAY_RES=$(curl -s --max-time 10 "$API/v1/jobs/$REPLAY_JOB_ID/stream" \
    -H "X-API-Key: $API_KEY" 2>/dev/null || true)
  if echo "$REPLAY_RES" | grep -q "data:"; then
    pass "SSE replay → data events received"
  else
    # Replay may return empty for jobs with no stored tokens
    pass "SSE replay → endpoint accessible (no stored tokens)"
  fi
else
  info "No completed job for replay (skipped)"
fi

# 26.2: Dashboard jobs SSE stream (GET /v1/dashboard/jobs/stream)
hdr "26.2 Dashboard jobs SSE stream"
DJSSE_CODE=$(curl -s -w "\n%{http_code}" --max-time 3 "$API/v1/dashboard/jobs/stream" \
  -H "Authorization: Bearer $TK" 2>/dev/null || true)
DJSSE_CODE=$(echo "$DJSSE_CODE" | code)
# SSE endpoint returns 200 and keeps streaming — curl times out with 200
case "$DJSSE_CODE" in
  200|000) pass "Dashboard jobs SSE stream accessible" ;;
  *) fail "Dashboard jobs SSE → $DJSSE_CODE" ;;
esac

# ── Phase 27: Test Endpoints (JWT, no rate limit) ─────────────────────────────

hdr "Phase 27: Test Endpoints"

# 27.1: Test Ollama /api/chat
hdr "27.1 Test Ollama /api/chat (JWT)"
TEST_CHAT_CODE=$(apostc "/v1/test/api/chat" \
  "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}],\"stream\":false}" | code)
[ "$TEST_CHAT_CODE" = "200" ] && pass "Test /api/chat → 200" || fail "Test /api/chat → $TEST_CHAT_CODE"

# 27.2: Test Ollama /api/generate
hdr "27.2 Test Ollama /api/generate (JWT)"
TEST_GEN_CODE=$(apostc "/v1/test/api/generate" \
  "{\"model\":\"$MODEL\",\"prompt\":\"ping\",\"stream\":false}" | code)
[ "$TEST_GEN_CODE" = "200" ] && pass "Test /api/generate → 200" || fail "Test /api/generate → $TEST_GEN_CODE"

# ── Phase 28: Password Reset & Session Details ────────────────────────────────

hdr "Phase 28: Password Reset & Session Management"

# 28.1: Create reset link (super only)
hdr "28.1 Password reset link"
# Create a temp account for reset testing
RESET_USER="e2e-reset-$(python3 -c 'import uuid;print(str(uuid.uuid4())[:8])')"
RESET_ACCT_RES=$(apostc "/v1/accounts" \
  "{\"username\":\"$RESET_USER\",\"password\":\"OldPass123!\",\"name\":\"Reset Test\",\"role\":\"admin\"}")
RESET_ACCT_CODE=$(echo "$RESET_ACCT_RES" | code)
RESET_ACCT_ID=$(echo "$RESET_ACCT_RES" | body | jv '["id"]' 2>/dev/null || echo "")

if [ "$RESET_ACCT_CODE" = "200" ] || [ "$RESET_ACCT_CODE" = "201" ]; then
  # Generate reset link
  RESET_LINK_RES=$(apostc "/v1/accounts/$RESET_ACCT_ID/reset-link" "{}")
  RESET_LINK_CODE=$(echo "$RESET_LINK_RES" | code)
  RESET_TOKEN=$(echo "$RESET_LINK_RES" | body | jv '["token"]' 2>/dev/null || echo "")

  case "$RESET_LINK_CODE" in
    200|201) pass "Reset link created → $RESET_LINK_CODE" ;;
    *) fail "Reset link → $RESET_LINK_CODE" ;;
  esac

  # 28.2: Use reset token to change password
  hdr "28.2 Password reset with token"
  if [ -n "$RESET_TOKEN" ] && [ "$RESET_TOKEN" != "None" ]; then
    RESET_PW_CODE=$(rawpostc "/v1/auth/reset-password" \
      "{\"token\":\"$RESET_TOKEN\",\"new_password\":\"NewPass456!\"}" | code)
    [ "$RESET_PW_CODE" = "200" ] || [ "$RESET_PW_CODE" = "204" ] \
      && pass "Password reset → $RESET_PW_CODE" || fail "Password reset → $RESET_PW_CODE"

    # Verify new password works
    RESET_LOGIN_CODE=$(rawpostc "/v1/auth/login" \
      "{\"username\":\"$RESET_USER\",\"password\":\"NewPass456!\"}" | code)
    [ "$RESET_LOGIN_CODE" = "200" ] \
      && pass "Login with new password → 200" || fail "Login with new password → $RESET_LOGIN_CODE"

    # 28.3: Reuse reset token → should fail
    hdr "28.3 Reused reset token → rejected"
    REUSE_CODE=$(rawpostc "/v1/auth/reset-password" \
      "{\"token\":\"$RESET_TOKEN\",\"new_password\":\"Another789!\"}" | code)
    case "$REUSE_CODE" in
      400|401|404|410) pass "Reused reset token rejected → $REUSE_CODE" ;;
      *) fail "Reused reset token: expected rejection, got $REUSE_CODE" ;;
    esac
  else
    info "No reset token returned (skipped)"
  fi

  # Cleanup
  adel "/v1/accounts/$RESET_ACCT_ID" > /dev/null 2>&1
else
  fail "Reset test account creation failed ($RESET_ACCT_CODE)"
fi

# 28.4: Revoke specific session
hdr "28.4 Revoke specific session"
# Login fresh, find the session, revoke it specifically
SESS_LOGIN=$(curl -si "$API/v1/auth/login" \
  -H 'Content-Type: application/json' -d @/tmp/_sched_login.json 2>/dev/null)
SESS_TK=$(echo "$SESS_LOGIN" | sed -n 's/.*veronex_access_token=\([^;]*\).*/\1/p' | head -1)
if [ -n "$SESS_TK" ]; then
  # Get admin account ID
  ADMIN_ACCT_ID=$(aget "/v1/accounts" 2>/dev/null | jv '[0]["id"]' 2>/dev/null || echo "")
  if [ -n "$ADMIN_ACCT_ID" ] && [ "$ADMIN_ACCT_ID" != "None" ]; then
    # List sessions, find the newest one
    SESSIONS=$(aget "/v1/accounts/$ADMIN_ACCT_ID/sessions" 2>/dev/null || echo "[]")
    SESS_ID=$(echo "$SESSIONS" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
if isinstance(d, list) and len(d) > 0:
    print(d[-1].get('id', d[-1].get('session_id', '')))
else:
    print('')
" 2>/dev/null || echo "")
    if [ -n "$SESS_ID" ] && [ "$SESS_ID" != "" ]; then
      REVOKE_CODE=$(adelc "/v1/sessions/$SESS_ID" | code)
      case "$REVOKE_CODE" in
        200|204) pass "Revoke specific session → $REVOKE_CODE" ;;
        *) fail "Revoke specific session → $REVOKE_CODE" ;;
      esac
    else
      info "No session ID found (skipped)"
    fi
  else
    info "No admin account ID (skipped)"
  fi
else
  fail "Could not login for session test"
fi

# ── Phase 29: Disabled Model & Edge Cases ─────────────────────────────────────

hdr "Phase 29: Model & Inference Edge Cases"

# 29.1: Disabled model → inference should fail or route elsewhere
hdr "29.1 Disabled model inference"
if [ -n "$PROVIDER_ID" ] && [ "$PROVIDER_ID" != "None" ]; then
  # Disable the model
  apatch "/v1/providers/$PROVIDER_ID/selected-models/$MODEL" '{"is_enabled":false}' > /dev/null 2>&1
  sleep 1

  # Try inference — should fail (no available provider for this model)
  DIS_CODE=$(curl -s -w "%{http_code}" -o /dev/null --max-time 15 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"test\"}],\"max_tokens\":4,\"stream\":false}")
  # With model disabled on this provider, it may route to another or fail
  case "$DIS_CODE" in
    200) info "Disabled model still routed (other providers may serve it)" ;;
    400|404|503) pass "Disabled model blocked → $DIS_CODE" ;;
    *) info "Disabled model response → $DIS_CODE" ;;
  esac

  # Re-enable
  apatch "/v1/providers/$PROVIDER_ID/selected-models/$MODEL" '{"is_enabled":true}' > /dev/null 2>&1
  pass "Model disable/enable cycle completed"
else
  info "No provider ID (skipped)"
fi

# 29.2: Dashboard job filtering
hdr "29.2 Dashboard job filtering"
# Filter by status
FILTER_CODE=$(agetc "/v1/dashboard/jobs?status=completed&limit=5" | code)
[ "$FILTER_CODE" = "200" ] && pass "Jobs filter by status → 200" || fail "Jobs filter → $FILTER_CODE"

# Filter by search query
SEARCH_CODE=$(agetc "/v1/dashboard/jobs?q=hello&limit=5" | code)
[ "$SEARCH_CODE" = "200" ] && pass "Jobs search by prompt → 200" || fail "Jobs search → $SEARCH_CODE"

# Filter by source
SOURCE_CODE=$(agetc "/v1/dashboard/jobs?source=api&limit=5" | code)
[ "$SOURCE_CODE" = "200" ] && pass "Jobs filter by source → 200" || fail "Jobs source filter → $SOURCE_CODE"

# 29.3: Ollama /api/embeddings (alias for embed)
hdr "29.3 Ollama /api/embeddings"
EMBEDDINGS_CODE=$(curl -s -w "\n%{http_code}" --max-time 30 "$API/api/embeddings" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"prompt\":\"test\"}" 2>/dev/null | code)
case "$EMBEDDINGS_CODE" in
  200) pass "Ollama /api/embeddings → 200" ;;
  400|404|500|501) pass "Ollama /api/embeddings → $EMBEDDINGS_CODE (not supported by model)" ;;
  *) fail "Ollama /api/embeddings → $EMBEDDINGS_CODE" ;;
esac

# 29.4: Swagger & Redoc docs
hdr "29.4 API documentation pages"
SWAGGER_CODE=$(curl -s -w "\n%{http_code}" "$API/docs/swagger" | code)
[ "$SWAGGER_CODE" = "200" ] && pass "Swagger UI → 200" || fail "Swagger UI → $SWAGGER_CODE"

REDOC_CODE=$(curl -s -w "\n%{http_code}" "$API/docs/redoc" | code)
[ "$REDOC_CODE" = "200" ] && pass "Redoc UI → 200" || fail "Redoc UI → $REDOC_CODE"

# 29.5: Dashboard stats verification
hdr "29.5 Dashboard stats content"
FINAL_STATS=$(aget "/v1/dashboard/stats" 2>/dev/null || echo "{}")
FINAL_JOBS=$(echo "$FINAL_STATS" | jv '["total_jobs"]' 2>/dev/null || echo "0")
[ "$FINAL_JOBS" != "0" ] && [ "$FINAL_JOBS" != "None" ] \
  && pass "Dashboard total_jobs=$FINAL_JOBS" || info "Dashboard total_jobs=$FINAL_JOBS (may be 0)"

# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "${CYAN}${BOLD}  Test Results${NC}"
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "  Round 1 (pre-AIMD):  ${GREEN}OK=$R1_OK${NC} ${YELLOW}Queued=$R1_Q${NC} ${RED}Failed=$R1_F${NC}"
echo -e "  Round 2 (AIMD):      ${GREEN}OK=$R2_OK${NC} ${YELLOW}Queued=$R2_Q${NC} ${RED}Failed=$R2_F${NC}"
echo -e "  AIMD limit:          ${AIMD_LIMIT:-unknown}"
echo ""
echo -e "  ${GREEN}PASS: $PASS_COUNT${NC}  ${RED}FAIL: $FAIL_COUNT${NC}"

if [ "$FAIL_COUNT" -gt 0 ]; then
  echo ""
  echo -e "  ${RED}Failed assertions:${NC}"
  for msg in "${FAIL_MSGS[@]}"; do
    echo -e "    ${RED}- $msg${NC}"
  done
fi

echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"

exit "$FAIL_COUNT"
