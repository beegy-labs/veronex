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
OLLAMA_URL="${OLLAMA_URL:-https://ollama.girok.dev}"
NODE_EXPORTER="${NODE_EXPORTER:-http://192.168.1.21:9100}"
USERNAME="${USERNAME:-admin}"
# E2E_PASSWORD env var required in CI; falls back to default for local dev.
_E2E_DEFAULT=admin2026!
PASSWORD=${E2E_PASSWORD:-$_E2E_DEFAULT}
unset _E2E_DEFAULT
MODEL="${MODEL:-qwen3:8b}"
CONCURRENT="${CONCURRENT:-6}"
SKIP_DB_RESET="${SKIP_DB_RESET:-0}"

# ── JSON helpers (no jq dependency) ──────────────────────────────────────────
jv() { python3 -c "import sys,json; print(json.loads(sys.stdin.read())$1)" 2>/dev/null; }
body() { sed '$d'; }
code() { tail -1; }

# ── Authenticated curl wrappers ──────────────────────────────────────────────
aget()   { curl -sf "$API$1" -H "Authorization: Bearer $TK"; }
apost()  { curl -sf "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
apatch() { curl -sf -X PATCH "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
adel()   { curl -sf -X DELETE "$API$1" -H "Authorization: Bearer $TK"; }
agetc()  { curl -s -w "\n%{http_code}" "$API$1" -H "Authorization: Bearer $TK"; }
apostc() { curl -s -w "\n%{http_code}" "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
apatchc(){ curl -s -w "\n%{http_code}" -X PATCH "$API$1" -H "Authorization: Bearer $TK" -H 'Content-Type: application/json' -d "$2"; }
adelc()  { curl -s -w "\n%{http_code}" -X DELETE "$API$1" -H "Authorization: Bearer $TK"; }
rawc()   { curl -s -w "\n%{http_code}" "$API$1"; }
rawpostc() { curl -s -w "\n%{http_code}" "$API$1" -H 'Content-Type: application/json' -d "$2"; }
# API-key authenticated
kpostc() { curl -s -w "\n%{http_code}" "$API$1" -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' -d "$2"; }
kgetc()  { curl -s -w "\n%{http_code}" "$API$1" -H "Authorization: Bearer $API_KEY"; }
kget()   { curl -sf "$API$1" -H "Authorization: Bearer $API_KEY"; }
kdelc()  { curl -s -w "\n%{http_code}" -X DELETE "$API$1" -H "Authorization: Bearer $API_KEY"; }

# ── Assertions ───────────────────────────────────────────────────────────────

# Assert authenticated GET returns expected status.
assert_get() { local c; c=$(agetc "$1" | code); [ "$c" = "$2" ] && pass "$3 → $2" || fail "$3 → $c"; }

# ── Reusable patterns ───────────────────────────────────────────────────────

# Fire N concurrent inference requests. Sets R_OK, R_Q, R_F.
fire_concurrent() {
  local count="$1" prefix="$2"
  local tmpdir; tmpdir=$(mktemp -d)
  for i in $(seq 1 "$count"); do
    (
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
  for f in "$tmpdir"/r_*; do
    read -r IDX CODE DUR < "$f"
    case "$CODE" in
      200) echo -e "    #$IDX: ${GREEN}200${NC} ($DUR)"; R_OK=$((R_OK+1)) ;;
      429|503) echo -e "    #$IDX: ${YELLOW}${CODE}${NC} ($DUR) [queued/throttled]"; R_Q=$((R_Q+1)) ;;
      *) echo -e "    #$IDX: ${RED}${CODE}${NC} ($DUR)"; R_F=$((R_F+1)) ;;
    esac
  done
  rm -rf "$tmpdir"
}

# Print capacity snapshot JSON
print_capacity() {
  echo "$1" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
for p in d.get('providers', []):
    name = p.get('provider_name', '?')
    used, total = p.get('used_vram_mb', 0), p.get('total_vram_mb', 0)
    thermal = p.get('thermal_state', 'unknown')
    print(f'    {name}: VRAM={used}/{total}MB thermal={thermal}')
    for m in p.get('loaded_models', []):
        print(f'      {m[\"model_name\"]}: weight={m[\"weight_mb\"]}MB active={m[\"active_requests\"]}/{m[\"max_concurrent\"]}')
" 2>/dev/null || echo "    (no data)"
}

# ── State management ─────────────────────────────────────────────────────────
E2E_STATE="${E2E_STATE:-/tmp/veronex-e2e-state.env}"

save_var()   { echo "export $1=\"$2\"" >> "$E2E_STATE"; export "$1"="$2"; }
load_state() { [ -f "$E2E_STATE" ] && source "$E2E_STATE" || true; }

# Write pass/fail counts to state file for aggregation.
# Parallel phases set E2E_COUNTS_FILE to a phase-specific path.
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
