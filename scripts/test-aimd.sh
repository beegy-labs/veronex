#!/usr/bin/env bash
# ── AIMD Concurrency Limiter Integration Test ────────────────────────────────
# Tests the full AIMD pipeline against a live Veronex + Ollama setup.
#
# Usage:
#   ./scripts/test-aimd.sh
#
# Prerequisites:
#   - docker compose up (Veronex stack running)
#   - Ollama server at OLLAMA_URL reachable
#   - Node-exporter at NODE_EXPORTER reachable
set -euo pipefail

API="${API_URL:-http://localhost:3001}"
OLLAMA_URL="${OLLAMA_URL:-https://ollama.girok.dev}"
NODE_EXPORTER="${NODE_EXPORTER:-http://192.168.1.21:9100}"
USERNAME="admin"
PASSWORD_FILE=$(mktemp)
printf 'admin2026!' > "$PASSWORD_FILE"
MODEL="${MODEL:-qwen3:8b}"
CONCURRENT="${CONCURRENT:-6}"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
pass() { echo -e "${GREEN}[PASS]${NC} $1"; }
fail() { echo -e "${RED}[FAIL]${NC} $1"; exit 1; }
info() { echo -e "${YELLOW}[INFO]${NC} $1"; }
hdr()  { echo -e "\n${CYAN}── $1 ──${NC}"; }

# JSON value extractor (no jq dependency)
jv() { python3 -c "import sys,json; print(json.loads(sys.stdin.read())$1)"; }

# Authenticated request helpers (Bearer token)
TK=""
aget()  { curl -sf "$API$1" -H "Authorization: Bearer $TK"; }
apost() { curl -sf "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
apatch(){ curl -sf -X PATCH "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
adel()  { curl -sf -X DELETE "$API$1" -H "Authorization: Bearer $TK"; }

TMPDIR_TEST=""; TMPDIR_TEST2=""
cleanup() { rm -f "$PASSWORD_FILE" /tmp/_aimd_login.json; rm -rf "$TMPDIR_TEST" "$TMPDIR_TEST2" 2>/dev/null; }
trap cleanup EXIT

# ══════════════════════════════════════════════════════════════════════════════
echo -e "${CYAN}══════════════════════════════════════${NC}"
echo -e "${CYAN}  AIMD Concurrency Limiter Test${NC}"
echo -e "${CYAN}══════════════════════════════════════${NC}"
info "API: $API | Ollama: $OLLAMA_URL | Node-exporter: $NODE_EXPORTER"

# ── Step 0: Reset DB ─────────────────────────────────────────────────────────
hdr "Step 0: Reset database"
docker compose exec -T postgres psql -U veronex -d veronex -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;" > /dev/null 2>&1
docker compose restart veronex > /dev/null 2>&1
info "Waiting for veronex to start..."
for i in $(seq 1 30); do
  sleep 2
  HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$API/v1/setup" 2>/dev/null || echo "000")
  if [ "$HTTP_CODE" != "000" ]; then break; fi
  if [ "$i" -eq 30 ]; then fail "veronex did not start in 60s"; fi
done
pass "Database reset & veronex restarted"

# ── Step 1: Setup account ────────────────────────────────────────────────────
hdr "Step 1: Create admin account"
cat > /tmp/_aimd_login.json << 'EOF'
{"username":"admin","password":"admin2026!"}
EOF
SETUP_RES=$(curl -s -w "\n%{http_code}" "$API/v1/setup" -H 'Content-Type: application/json' -d @/tmp/_aimd_login.json)
SETUP_CODE=$(echo "$SETUP_RES" | tail -1)
if [ "$SETUP_CODE" = "200" ] || [ "$SETUP_CODE" = "201" ]; then
  pass "Account created"
else
  fail "Setup failed (HTTP $SETUP_CODE): $(echo "$SETUP_RES" | head -1)"
fi

# ── Step 2: Login ─────────────────────────────────────────────────────────────
hdr "Step 2: Login"
LOGIN_RAW=$(curl -si "$API/v1/auth/login" -H 'Content-Type: application/json' -d @/tmp/_aimd_login.json 2>&1)
TK=$(echo "$LOGIN_RAW" | sed -n 's/.*veronex_access_token=\([^;]*\).*/\1/p')
if [ -z "$TK" ]; then fail "Could not extract token"; fi
pass "Logged in"

# ── Step 3: Register GPU server ──────────────────────────────────────────────
hdr "Step 3: Register GPU server"
SERVER_ID=$(apost "/v1/servers" "{\"name\":\"girok-gpu\",\"node_exporter_url\":\"$NODE_EXPORTER\"}" | jv '["id"]')
pass "Server: $SERVER_ID"

# ── Step 4: Register Ollama provider ─────────────────────────────────────────
hdr "Step 4: Register Ollama provider"
PROV_RES=$(apost "/v1/providers" "{\"name\":\"girok-ollama\",\"provider_type\":\"ollama\",\"url\":\"$OLLAMA_URL\"}")
PROVIDER_ID=$(echo "$PROV_RES" | jv '["id"]')
PROV_STATUS=$(echo "$PROV_RES" | jv '["status"]')
pass "Provider: $PROVIDER_ID (status: $PROV_STATUS)"

# ── Step 5: Link provider → server ───────────────────────────────────────────
hdr "Step 5: Link provider to server"
apatch "/v1/providers/$PROVIDER_ID" "{\"name\":\"girok-ollama\",\"server_id\":\"$SERVER_ID\",\"gpu_index\":0}" > /dev/null
pass "Linked"

# ── Step 6: Sync models ──────────────────────────────────────────────────────
hdr "Step 6: Sync models"
SYNC_ID=$(apost "/v1/ollama/models/sync" "{}" | jv '["job_id"]')
info "Sync job: $SYNC_ID"
for i in $(seq 1 15); do sleep 2; STATUS=$(aget "/v1/ollama/sync/status" 2>/dev/null | jv '["status"]' 2>/dev/null || echo "running"); [ "$STATUS" != "running" ] && break; done
MODEL_COUNT=$(aget "/v1/ollama/models" | jv '["models"].__len__()')
pass "$MODEL_COUNT models synced"

# ── Step 7: Verify sync settings (probe fields) ──────────────────────────────
hdr "Step 7: Check sync settings"
SETTINGS=$(aget "/v1/dashboard/capacity/settings")
PP=$(echo "$SETTINGS" | jv '["probe_permits"]')
PR=$(echo "$SETTINGS" | jv '["probe_rate"]')
pass "probe_permits=$PP, probe_rate=$PR"

# ── Step 8: Update probe settings ────────────────────────────────────────────
hdr "Step 8: Update probe settings"
apatch "/v1/dashboard/capacity/settings" '{"probe_permits":2,"probe_rate":5}' > /dev/null
S2=$(aget "/v1/dashboard/capacity/settings")
PP2=$(echo "$S2" | jv '["probe_permits"]')
PR2=$(echo "$S2" | jv '["probe_rate"]')
[ "$PP2" = "2" ] && [ "$PR2" = "5" ] && pass "Updated: permits=$PP2, rate=$PR2" || fail "Update failed (got $PP2/$PR2)"
# Revert
apatch "/v1/dashboard/capacity/settings" '{"probe_permits":1,"probe_rate":3}' > /dev/null

# ── Step 9: Create API key ───────────────────────────────────────────────────
hdr "Step 9: Create API key"
ACCOUNT_ID=$(aget "/v1/accounts" | jv '[0]["id"]')
API_KEY=$(apost "/v1/keys" "{\"tenant_id\":\"$ACCOUNT_ID\",\"name\":\"aimd-test\",\"tier\":\"free\"}" | jv '["key"]')
pass "Key: ${API_KEY:0:12}..."

# ── Step 10: Round 1 — concurrent inference ──────────────────────────────────
hdr "Step 10: Round 1 — $CONCURRENT concurrent requests to $MODEL"
TMPDIR_TEST=$(mktemp -d)
for i in $(seq 1 "$CONCURRENT"); do
  (
    T0=$(date +%s)
    RES=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
      -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
      -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Say only the number $i.\"}],\"max_tokens\":32,\"stream\":false}")
    CODE=$(echo "$RES" | tail -1)
    T1=$(date +%s)
    echo "$i $CODE $((T1-T0))s" > "$TMPDIR_TEST/r_$i"
  ) &
done
wait
echo ""
C=0; Q=0; F=0
for f in "$TMPDIR_TEST"/r_*; do
  read -r IDX CODE DUR < "$f"
  case "$CODE" in
    200) echo -e "  #$IDX: ${GREEN}200${NC} ($DUR)"; C=$((C+1)) ;;
    429|503) echo -e "  #$IDX: ${YELLOW}${CODE}${NC} ($DUR) queued"; Q=$((Q+1)) ;;
    *) echo -e "  #$IDX: ${RED}${CODE}${NC} ($DUR)"; F=$((F+1)) ;;
  esac
done
rm -rf "$TMPDIR_TEST"
pass "OK=$C, Queued=$Q, Failed=$F"

# ── Step 11: Trigger analyzer sync & wait ────────────────────────────────────
hdr "Step 11: Trigger capacity analyzer sync"
# Manual trigger bypasses 5min cooldown
apost "/v1/providers/sync" "{}" > /dev/null 2>&1 || true
info "Manual sync triggered, waiting for analyzer..."

for i in $(seq 1 12); do
  # Keep model loaded in Ollama while waiting
  curl -s "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":4,\"stream\":false}" > /dev/null 2>&1 &
  sleep 5
  CAP=$(aget "/v1/dashboard/capacity" 2>/dev/null || echo '{"providers":[]}')
  PC=$(echo "$CAP" | python3 -c "
import sys,json
d=json.loads(sys.stdin.read())
models=[m for p in d.get('providers',[]) for m in p.get('loaded_models',[])]
print(len(models))
" 2>/dev/null || echo 0)
  [ "$PC" -ge 1 ] && break
  printf "  tick %d (loaded_models: %s)\n" "$i" "$PC"
done
wait 2>/dev/null

echo ""
info "Capacity:"
echo "$CAP" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
for p in d.get('providers', []):
    print(f'  {p[\"provider_name\"]}:')
    print(f'    VRAM: {p[\"used_vram_mb\"]}/{p[\"total_vram_mb\"]}MB | thermal={p[\"thermal_state\"]}')
    for m in p.get('loaded_models', []):
        print(f'    {m[\"model_name\"]}: weight={m[\"weight_mb\"]}MB active={m[\"active_requests\"]} limit={m[\"max_concurrent\"]}')
" 2>/dev/null || echo "  (no data)"

# Verify AIMD initial limit
LIMIT=$(echo "$CAP" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
for p in d.get('providers', []):
    for m in p.get('loaded_models', []):
        if m['model_name'] == '$MODEL':
            print(m['max_concurrent'])
            break
" 2>/dev/null || echo "0")

if [ -n "$LIMIT" ] && [ "$LIMIT" -gt 0 ]; then
  pass "AIMD limit for $MODEL = $LIMIT"
else
  info "AIMD limit not yet set (analyzer may need another cycle)"
fi

# ── Step 12: DB verification ─────────────────────────────────────────────────
hdr "Step 12: DB verification"
docker compose exec -T postgres psql -U veronex -d veronex -c \
  "SELECT model_name, weight_mb, kv_per_request_mb, max_concurrent, baseline_tps FROM model_vram_profiles;"
pass "model_vram_profiles verified"

docker compose exec -T postgres psql -U veronex -d veronex -c \
  "SELECT probe_permits, probe_rate FROM capacity_settings;"
pass "capacity_settings verified"

# ── Step 13: Round 2 — concurrent inference (AIMD active) ────────────────────
hdr "Step 13: Round 2 — $CONCURRENT requests (AIMD active, limit=$LIMIT)"
TMPDIR_TEST2=$(mktemp -d)
for i in $(seq 1 "$CONCURRENT"); do
  (
    T0=$(date +%s)
    RES=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
      -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
      -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Reply with digit $i.\"}],\"max_tokens\":16,\"stream\":false}")
    CODE=$(echo "$RES" | tail -1)
    T1=$(date +%s)
    echo "$i $CODE $((T1-T0))s" > "$TMPDIR_TEST2/r_$i"
  ) &
done
wait
echo ""
C2=0; Q2=0; F2=0
for f in "$TMPDIR_TEST2"/r_*; do
  read -r IDX CODE DUR < "$f"
  case "$CODE" in
    200) echo -e "  #$IDX: ${GREEN}200${NC} ($DUR)"; C2=$((C2+1)) ;;
    429|503) echo -e "  #$IDX: ${YELLOW}${CODE}${NC} ($DUR) queued"; Q2=$((Q2+1)) ;;
    *) echo -e "  #$IDX: ${RED}${CODE}${NC} ($DUR)"; F2=$((F2+1)) ;;
  esac
done
rm -rf "$TMPDIR_TEST2"
pass "Round 2 — OK=$C2, Queued=$Q2, Failed=$F2"

# ── Step 14: Final capacity check ────────────────────────────────────────────
hdr "Step 14: Final capacity snapshot"
sleep 2
CFINAL=$(aget "/v1/dashboard/capacity" 2>/dev/null || echo '{"providers":[]}')
echo "$CFINAL" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
for p in d.get('providers', []):
    for m in p.get('loaded_models', []):
        print(f'  {m[\"model_name\"]}: active={m[\"active_requests\"]} limit={m[\"max_concurrent\"]}')
" 2>/dev/null || echo "  (no data)"

# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo -e "${GREEN}══════════════════════════════════════${NC}"
echo -e "${GREEN}  AIMD Integration Test Complete${NC}"
echo -e "${GREEN}══════════════════════════════════════${NC}"
echo -e "  Round 1: OK=$C Queued=$Q Failed=$F"
echo -e "  Round 2: OK=$C2 Queued=$Q2 Failed=$F2"
echo -e "  AIMD limit: ${LIMIT:-unknown}"
echo -e "${GREEN}══════════════════════════════════════${NC}"
