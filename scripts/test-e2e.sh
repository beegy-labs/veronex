#!/usr/bin/env bash
# ── Veronex E2E Integration Test ─────────────────────────────────────────────
# Single entry point — runs all phase scripts in order.
#
# Usage:
#   ./scripts/test-e2e.sh                    # full test (DB reset)
#   SKIP_DB_RESET=1 ./scripts/test-e2e.sh    # reuse existing DB
#   MODEL=qwen3:8b CONCURRENT=8 ./scripts/test-e2e.sh
#
# Individual phase (after setup):
#   ./scripts/e2e/03-crud.sh
#
# Prerequisites:
#   - docker compose up (Veronex stack running)
#   - At least 1 Ollama server reachable at OLLAMA_URL
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
E2E_DIR="$SCRIPT_DIR/e2e"

# ── State & counter files ────────────────────────────────────────────────────
export E2E_STATE; E2E_STATE=$(mktemp /tmp/veronex-e2e-state.XXXXXX)
COUNTS_FILE="$E2E_STATE.counts"
: > "$E2E_STATE"
: > "$COUNTS_FILE"
cleanup() { rm -f "$E2E_STATE" "$COUNTS_FILE" "$COUNTS_FILE".* /tmp/_sched_login.json 2>/dev/null; }
trap cleanup EXIT

# ── Banner ───────────────────────────────────────────────────────────────────
CYAN='\033[0;36m'; BOLD='\033[1m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "${CYAN}${BOLD}  Veronex E2E Integration Test${NC}"
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "  ${CYAN}API=${API_URL:-http://localhost:3001}  Model=${MODEL:-qwen3:8b}  Concurrency=${CONCURRENT:-6}${NC}"

# ── Run phases ───────────────────────────────────────────────────────────────
# Sequential: 01-setup (creates state) then 02-inference (generates data)
bash "$E2E_DIR/01-setup.sh"
bash "$E2E_DIR/02-inference.sh"

# Parallel: 03-06 read shared state but write separate counts files
PARALLEL_PHASES=("03-crud.sh" "04-security.sh" "05-api-surface.sh" "06-lifecycle.sh")
PARALLEL_PIDS=()
PARALLEL_COUNTS=()
PARALLEL_EXIT=0

for phase in "${PARALLEL_PHASES[@]}"; do
  phase_counts="$COUNTS_FILE.${phase%.sh}"
  : > "$phase_counts"
  PARALLEL_COUNTS+=("$phase_counts")
  E2E_COUNTS_FILE="$phase_counts" bash "$E2E_DIR/$phase" &
  PARALLEL_PIDS+=($!)
done

# Wait for all parallel phases; collect any failures
for i in "${!PARALLEL_PIDS[@]}"; do
  if ! wait "${PARALLEL_PIDS[$i]}"; then
    echo -e "${RED}[ERROR]${NC} ${PARALLEL_PHASES[$i]} exited non-zero" >&2
    PARALLEL_EXIT=1
  fi
done

# ── Aggregate results ────────────────────────────────────────────────────────
# Collect counts from sequential phases (main counts file) + parallel phase files
ALL_COUNTS_FILES=("$COUNTS_FILE" "${PARALLEL_COUNTS[@]}")
TOTAL_PASS=0; TOTAL_FAIL=0; ALL_FAIL_MSGS=()
for cf in "${ALL_COUNTS_FILES[@]}"; do
  [ -f "$cf" ] || continue
  while IFS= read -r line; do
    case "$line" in
      PASS_COUNT=*) TOTAL_PASS=$((TOTAL_PASS + ${line#PASS_COUNT=})) ;;
      FAIL_COUNT=*) TOTAL_FAIL=$((TOTAL_FAIL + ${line#FAIL_COUNT=})) ;;
      FAIL_MSG=*)   ALL_FAIL_MSGS+=("${line#FAIL_MSG=}") ;;
    esac
  done < "$cf"
done

# ── Summary ──────────────────────────────────────────────────────────────────
# Load inference round stats from state
source "$E2E_STATE" 2>/dev/null || true

echo ""
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "${CYAN}${BOLD}  Test Results${NC}"
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "  Round 1 (pre-AIMD):  ${GREEN}OK=${R1_OK:-?}${NC}  Queued=${R1_Q:-?}  Failed=${R1_F:-?}"
echo -e "  Round 2 (AIMD):      ${GREEN}OK=${R2_OK:-?}${NC}  Queued=${R2_Q:-?}  Failed=${R2_F:-?}"
echo -e "  AIMD limit:          ${AIMD_LIMIT:-unknown}"
echo ""
echo -e "  ${GREEN}PASS: $TOTAL_PASS${NC}  ${RED}FAIL: $TOTAL_FAIL${NC}"

if [ "$TOTAL_FAIL" -gt 0 ]; then
  echo ""
  echo -e "  ${RED}Failed assertions:${NC}"
  for msg in "${ALL_FAIL_MSGS[@]}"; do
    echo -e "    ${RED}- $msg${NC}"
  done
fi

echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"

# Exit non-zero if any test failed or a parallel phase crashed
[ "$PARALLEL_EXIT" -ne 0 ] && [ "$TOTAL_FAIL" -eq 0 ] && TOTAL_FAIL=1
exit "$TOTAL_FAIL"
