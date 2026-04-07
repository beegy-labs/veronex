#!/usr/bin/env bash
# ── Veronex E2E Shared Library ───────────────────────────────────────────────
# Sourced by all phase scripts. Provides helpers, state management, assertions.

# ── Color & output ───────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'
pass() { echo -e "  ${GREEN}[PASS]${NC} $1"; PASS_COUNT=$((PASS_COUNT+1)); }
fail() { echo -e "  ${RED}[FAIL]${NC} $1"; FAIL_COUNT=$((FAIL_COUNT+1)); FAIL_MSGS+=("$1"); }
info() { echo -e "  ${YELLOW}[INFO]${NC} $1"; }
hdr()  { echo -e "\n${CYAN}${BOLD}── $1 ──${NC}"; }

PASS_COUNT=${PASS_COUNT:-0}; FAIL_COUNT=${FAIL_COUNT:-0}; FAIL_MSGS=()

# ── Configuration ────────────────────────────────────────────────────────────
API="${API_URL:-http://localhost:3001}"

# Local Ollama provider — host.docker.internal so veronex container can reach host machine
OLLAMA_LOCAL="${OLLAMA_LOCAL:-http://host.docker.internal:11434}"
NODE_EXPORTER_LOCAL="${NODE_EXPORTER_LOCAL:-http://host.docker.internal:9100}"

# Remote Ollama provider (k8s-worker-ai-01)
OLLAMA_REMOTE="${OLLAMA_REMOTE:-https://ollama-1.kr1.girok.dev}"
NODE_EXPORTER_REMOTE="${NODE_EXPORTER_REMOTE:-http://192.168.1.21:9100}"

USERNAME="${USERNAME:-test}"
_E2E_DEFAULT='test1234!'
PASSWORD=${E2E_PASSWORD:-$_E2E_DEFAULT}
unset _E2E_DEFAULT

MODEL="${MODEL:-qwen3:8b}"
# Additional models for multi-model inference tests (auto-detected from synced models)
MODELS_EXTRA="${MODELS_EXTRA:-}"
CONCURRENT="${CONCURRENT:-6}"
SKIP_DB_RESET="${SKIP_DB_RESET:-0}"

# ── JSON helpers (no jq dependency) ──────────────────────────────────────────
jv()   { python3 -c "import sys,json; print(json.loads(sys.stdin.read())$1)" 2>/dev/null; }
body() { sed '$d'; }
code() { tail -1; }

# ── Authenticated curl wrappers (JWT) ─────────────────────────────────────────
aget()     { curl -sf "$API$1" -H "Authorization: Bearer $TK"; }
apost()    { curl -sf "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
apatch()   { curl -sf -X PATCH "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
adel()     { curl -sf -X DELETE "$API$1" -H "Authorization: Bearer $TK"; }
agetc()    { curl -s -w "\n%{http_code}" "$API$1" -H "Authorization: Bearer $TK" 2>/dev/null || printf "\n000"; }
apostc()   { curl -s -w "\n%{http_code}" "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2" 2>/dev/null || printf "\n000"; }
apatchc()  { curl -s -w "\n%{http_code}" -X PATCH "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2" 2>/dev/null || printf "\n000"; }
adelc()    { curl -s -w "\n%{http_code}" -X DELETE "$API$1" -H "Authorization: Bearer $TK" 2>/dev/null || printf "\n000"; }
rawc()     { curl -s -w "\n%{http_code}" "$API$1" 2>/dev/null || printf "\n000"; }
rawpostc() { curl -s -w "\n%{http_code}" "$API$1" -H 'Content-Type: application/json' -d "$2" 2>/dev/null || printf "\n000"; }

# ── Authenticated curl wrappers (API key) ─────────────────────────────────────
kpostc()   { curl -s -w "\n%{http_code}" "$API$1" -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' -d "$2" 2>/dev/null || printf "\n000"; }
kgetc()    { curl -s -w "\n%{http_code}" "$API$1" -H "Authorization: Bearer $API_KEY" 2>/dev/null || printf "\n000"; }
kget()     { curl -sf "$API$1" -H "Authorization: Bearer $API_KEY" 2>/dev/null || echo "{}"; }
kdelc()    { curl -s -w "\n%{http_code}" -X DELETE "$API$1" -H "Authorization: Bearer $API_KEY" 2>/dev/null || printf "\n000"; }

# ── Assertions ───────────────────────────────────────────────────────────────
assert_get() { local c; c=$(agetc "$1" | code); [ "$c" = "$2" ] && pass "$3 → $2" || fail "$3 → $c"; }

# ── Concurrent inference (routes through gateway to any provider) ─────────────
fire_concurrent() {
  local count="$1" prefix="$2"
  local tmpdir; tmpdir=$(mktemp -d)
  for i in $(seq 1 "$count"); do
    (
      set +u  # API_KEY may come from sourced state file
      T0=$(python3 -c "import time; print(int(time.time()*1000))")
      RES=$(curl -s -w "\n%{http_code}" "$API/v1/chat/completions" \
        -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
        -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"$prefix $i\"}],\"max_tokens\":16,\"stream\":false}" \
        --max-time 120)
      CODE=$(echo "$RES" | tail -1)
      T1=$(python3 -c "import time; print(int(time.time()*1000))")
      echo "$i $CODE $((T1 - T0))ms" > "$tmpdir/r_$i"
    ) &
  done
  wait; echo ""
  R_OK=0; R_Q=0; R_F=0
  shopt -s nullglob
  for f in "$tmpdir"/r_*; do
    read -r IDX CODE DUR < "$f"
    case "$CODE" in
      200)     echo -e "    #$IDX: ${GREEN}200${NC} ($DUR)"; R_OK=$((R_OK+1)) ;;
      429|503) echo -e "    #$IDX: ${YELLOW}${CODE}${NC} ($DUR) [queued/throttled]"; R_Q=$((R_Q+1)) ;;
      *)       echo -e "    #$IDX: ${RED}${CODE}${NC} ($DUR)"; R_F=$((R_F+1)) ;;
    esac
  done
  shopt -u nullglob
  rm -rf "$tmpdir"
}

# ── Capacity snapshot printer ─────────────────────────────────────────────────
print_capacity() {
  echo "$1" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
for p in d.get('providers', []):
    name    = p.get('provider_name', '?')
    used    = p.get('used_vram_mb', 0)
    total   = p.get('total_vram_mb', 0)
    thermal = p.get('thermal_state', 'unknown')
    temp    = p.get('temp_c')
    temp_str = f'{temp:.1f}C' if temp is not None else 'N/A'
    print(f'    {name}: VRAM={used}/{total}MB thermal={thermal} temp={temp_str}')
    for m in p.get('loaded_models', []):
        print(f'      {m[\"model_name\"]}: weight={m.get(\"weight_mb\",0)}MB active={m[\"active_requests\"]}/{m[\"max_concurrent\"]}')
" 2>/dev/null || echo "    (no data)"
}

# ── Valkey helpers (requires docker compose in PATH) ──────────────────────────
valkey_zcard() { docker compose exec -T valkey valkey-cli ZCARD "$1" 2>/dev/null | tr -d ' \r\n' || echo "0"; }
valkey_get()   { docker compose exec -T valkey valkey-cli GET "$1" 2>/dev/null | tr -d ' \r\n' || echo ""; }
valkey_hlen()  { docker compose exec -T valkey valkey-cli HLEN "$1" 2>/dev/null | tr -d ' \r\n' || echo "0"; }

# ── Queue drain helper ────────────────────────────────────────────────────────
# Wait up to MAX_WAIT seconds for the inference queue and active jobs to drain.
# Prevents test-ordering failures caused by prior tests leaving running/queued jobs.
wait_queue_empty() {
  local max_wait="${1:-30}" waited=0
  while [ "$waited" -lt "$max_wait" ]; do
    local depth
    depth=$(curl -sf "$API/v1/dashboard/queue" -H "Authorization: Bearer $TK" \
      2>/dev/null | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print(d.get('depth',0))" \
      2>/dev/null || echo "0")
    [ "${depth:-0}" -le 0 ] && return 0
    sleep 2; waited=$((waited + 2))
  done
  return 0  # non-fatal: proceed even if queue is still busy
}

# ── Self-sufficient auth bootstrap ───────────────────────────────────────────
# Replace bare `load_state` in script headers.
# Loads state first; if TK or API_KEY is absent, self-authenticates so every
# script can run standalone without 01-setup.sh having run first.
ensure_auth() {
  load_state

  # Bootstrap JWT if missing or expired (validate with a cheap API call)
  local _need_login=0
  if [ -z "${TK:-}" ]; then
    _need_login=1
  else
    local _probe_code
    _probe_code=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 \
      "$API/v1/accounts" -H "Authorization: Bearer $TK" 2>/dev/null || echo "000")
    [ "$_probe_code" = "401" ] && _need_login=1
  fi
  if [ "$_need_login" = "1" ]; then
    TK=""
    curl -s "$API/v1/setup" -H 'Content-Type: application/json' \
      -d "{\"username\":\"$USERNAME\",\"password\":\"$PASSWORD\"}" > /dev/null 2>&1 || true
    local _login_raw
    _login_raw=$(curl -si "$API/v1/auth/login" \
      -H 'Content-Type: application/json' \
      -d "{\"username\":\"$USERNAME\",\"password\":\"$PASSWORD\"}" 2>&1)
    TK=$(echo "$_login_raw" | sed -n 's/.*veronex_access_token=\([^;]*\).*/\1/p')
    [ -z "$TK" ] && { echo "[ERROR] ensure_auth: login failed"; exit 1; }
  fi

  # Bootstrap API key if missing
  if [ -z "${API_KEY:-}" ]; then
    local _acct_id
    _acct_id=$(curl -sf "$API/v1/accounts" -H "Authorization: Bearer $TK" 2>/dev/null \
      | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('accounts',[{}])[0].get('id',''))" 2>/dev/null || echo "")
    if [ -n "$_acct_id" ]; then
      local _key_res
      _key_res=$(curl -sf "$API/v1/keys" -H "Authorization: Bearer $TK" \
        -H 'Content-Type: application/json' \
        -d "{\"tenant_id\":\"$_acct_id\",\"name\":\"e2e-auto-$$\",\"tier\":\"paid\"}" 2>/dev/null || echo "{}")
      API_KEY=$(echo "$_key_res" \
        | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('key',''))" 2>/dev/null || echo "")
    fi
    [ -z "$API_KEY" ] && { echo "[ERROR] ensure_auth: API key creation failed"; exit 1; }
  fi
}

# ── Dynamic provider/server ID lookup ────────────────────────────────────────
# Called by scripts that use PROVIDER_ID_LOCAL/REMOTE or SERVER_ID_LOCAL/REMOTE.
# Fast-path: skips API calls when IDs are already loaded from state (after 01-setup).
ensure_provider_ids() {
  local _providers _servers
  if [ -z "${PROVIDER_ID_LOCAL:-}" ] || [ "${PROVIDER_ID_LOCAL}" = "None" ]; then
    _providers=$(curl -sf "$API/v1/providers" -H "Authorization: Bearer $TK" 2>/dev/null || echo '{"providers":[]}')
    PROVIDER_ID_LOCAL=$(echo "$_providers" | python3 -c "
import sys,json
d=json.loads(sys.stdin.read())
for p in d.get('providers',[]):
    if p.get('provider_type')=='ollama' and 'local' in p.get('name','').lower():
        print(p['id']); break
" 2>/dev/null || echo "")
    PROVIDER_ID_REMOTE=$(echo "$_providers" | python3 -c "
import sys,json
d=json.loads(sys.stdin.read())
for p in d.get('providers',[]):
    if p.get('provider_type')=='ollama' and 'local' not in p.get('name','').lower():
        print(p['id']); break
" 2>/dev/null || echo "")
  fi
  if [ -z "${SERVER_ID_LOCAL:-}" ] || [ "${SERVER_ID_LOCAL}" = "None" ]; then
    _servers=$(curl -sf "$API/v1/servers" -H "Authorization: Bearer $TK" 2>/dev/null || echo '{"servers":[]}')
    SERVER_ID_LOCAL=$(echo "$_servers" | python3 -c "
import sys,json
d=json.loads(sys.stdin.read())
for s in d.get('servers',[]):
    if 'local' in s.get('name','').lower():
        print(s['id']); break
" 2>/dev/null || echo "")
    SERVER_ID_REMOTE=$(echo "$_servers" | python3 -c "
import sys,json
d=json.loads(sys.stdin.read())
for s in d.get('servers',[]):
    if 'local' not in s.get('name','').lower():
        print(s['id']); break
" 2>/dev/null || echo "")
  fi
}

# ── State management ──────────────────────────────────────────────────────────
E2E_STATE="${E2E_STATE:-/tmp/veronex-e2e-state.env}"

save_var()   { echo "export $1=\"$2\"" >> "$E2E_STATE"; export "$1"="$2"; }
load_state() { [ -f "$E2E_STATE" ] && source "$E2E_STATE" || true; }

save_counts() {
  local cf="${E2E_COUNTS_FILE:-$E2E_STATE.counts}"
  echo "PASS_COUNT=$PASS_COUNT" >> "$cf"
  echo "FAIL_COUNT=$FAIL_COUNT" >> "$cf"
  if [ ${#FAIL_MSGS[@]} -gt 0 ]; then
    for msg in "${FAIL_MSGS[@]}"; do
      echo "FAIL_MSG=$msg" >> "$cf"
    done
  fi
}
