#!/usr/bin/env bash
# ── Veronex E2E Integration Test ─────────────────────────────────────────────
#
# Validates the full scheduler stack with dual Ollama providers:
#   - Local:  OLLAMA_LOCAL  (default: http://localhost:11434)  + NODE_EXPORTER_LOCAL  (default: http://localhost:9100)
#   - Remote: OLLAMA_REMOTE (default: https://ollama-1.kr1.girok.dev) + NODE_EXPORTER_REMOTE (default: http://192.168.1.21:9100)
#
# Usage:
#   ./scripts/test-e2e.sh
#   SKIP_DB_RESET=1 ./scripts/test-e2e.sh
#   MODEL=qwen3:8b CONCURRENT=8 ./scripts/test-e2e.sh
#   OLLAMA_LOCAL=http://localhost:11434 OLLAMA_REMOTE=https://ollama-1.kr1.girok.dev ./scripts/test-e2e.sh
#
# Execution strategy (optimized for speed):
#   Phase 1 (sequential) : 01-setup
#   Phase 2 (parallel)   : 03-inference + [04-crud, 05-security, 09-metrics, 10-image, 11-verify]
#   Phase 3 (parallel)   : [02-scheduler, 06-api-surface, 07-lifecycle, 08-sdd-advanced]
#     → Phase 3 starts only after 03-inference completes (AIMD state needed)
#     → Phase 2 independents start immediately after setup
#
# Prerequisites:
#   - docker compose up (Veronex stack running)
#   - At least one Ollama provider reachable
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
E2E_DIR="$SCRIPT_DIR/e2e"

# ── State & counter files ─────────────────────────────────────────────────────
export E2E_STATE; E2E_STATE=$(mktemp /tmp/veronex-e2e-state.XXXXXX)
COUNTS_FILE="$E2E_STATE.counts"
: > "$E2E_STATE"
: > "$COUNTS_FILE"
cleanup() { rm -f "$E2E_STATE" "$COUNTS_FILE" "$COUNTS_FILE".* /tmp/_sched_login.json 2>/dev/null; }
trap cleanup EXIT

# ── Helpers ──────────────────────────────────────────────────────────────────
CYAN='\033[0;36m'; BOLD='\033[1m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'
PARALLEL_EXIT=0
ALL_PHASE_COUNTS=("$COUNTS_FILE")

run_phase() {
  local phase="$1"
  local phase_counts="$COUNTS_FILE.${phase%.sh}"
  : > "$phase_counts"
  ALL_PHASE_COUNTS+=("$phase_counts")
  E2E_COUNTS_FILE="$phase_counts" bash "$E2E_DIR/$phase"
}

run_phase_bg() {
  local phase="$1"
  local phase_counts="$COUNTS_FILE.${phase%.sh}"
  : > "$phase_counts"
  ALL_PHASE_COUNTS+=("$phase_counts")
  E2E_COUNTS_FILE="$phase_counts" bash "$E2E_DIR/$phase" &
  echo $!
}

wait_all() {
  # Args: pid1 name1 pid2 name2 ...
  while [ $# -ge 2 ]; do
    local pid="$1" name="$2"; shift 2
    if ! wait "$pid"; then
      echo -e "${RED}[ERROR]${NC} $name exited non-zero" >&2
      PARALLEL_EXIT=1
    fi
  done
}

# ── Banner ────────────────────────────────────────────────────────────────────
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "${CYAN}${BOLD}  Veronex E2E — Dual-Provider Scheduler Test${NC}"
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "  ${CYAN}API        = ${API_URL:-http://localhost:3001}${NC}"
echo -e "  ${CYAN}Local      = ${OLLAMA_LOCAL:-http://localhost:11434} (node: ${NODE_EXPORTER_LOCAL:-http://localhost:9100})${NC}"
echo -e "  ${CYAN}Remote     = ${OLLAMA_REMOTE:-https://ollama-1.kr1.girok.dev} (node: ${NODE_EXPORTER_REMOTE:-http://192.168.1.21:9100})${NC}"
echo -e "  ${CYAN}Model      = ${MODEL:-qwen3:8b}  Concurrency = ${CONCURRENT:-6}${NC}"
echo ""

# ── Phase 1: Setup (sequential — must complete before anything else) ──────────
echo -e "${CYAN}${BOLD}[Phase 1] Setup: infra + auth + dual providers + API keys${NC}"
run_phase "01-setup.sh"

# ── Phase 2: Inference + Independent tests (parallel) ────────────────────────
# 03-inference runs alongside independent tests that don't need AIMD state.
# This saves ~2-3 minutes by not waiting for inference to finish before CRUD/security.
echo ""
echo -e "${CYAN}${BOLD}[Phase 2] Inference + independent tests (parallel)${NC}"

P2_WAIT_ARGS=()

# Inference (needs to finish before Phase 3)
INFERENCE_COUNTS="$COUNTS_FILE.03-inference"
: > "$INFERENCE_COUNTS"
ALL_PHASE_COUNTS+=("$INFERENCE_COUNTS")
E2E_COUNTS_FILE="$INFERENCE_COUNTS" bash "$E2E_DIR/03-inference.sh" &
P2_WAIT_ARGS+=($! "03-inference.sh")

# Independent tests (no AIMD dependency)
for phase in 04-crud.sh 05-security.sh 09-metrics-pipeline.sh 10-image-storage.sh 11-verify-liveness.sh; do
  pid=$(run_phase_bg "$phase")
  P2_WAIT_ARGS+=($pid "$phase")
done

# Wait for ALL Phase 2 (inference must finish for Phase 3)
wait_all "${P2_WAIT_ARGS[@]}"

# ── Phase 3: AIMD-dependent tests (parallel) ─────────────────────────────────
# These tests require AIMD learning data from 03-inference.
echo ""
echo -e "${CYAN}${BOLD}[Phase 3] AIMD-dependent tests (parallel)${NC}"

P3_WAIT_ARGS=()

for phase in 02-scheduler.sh 06-api-surface.sh 07-lifecycle.sh 08-sdd-advanced.sh; do
  pid=$(run_phase_bg "$phase")
  P3_WAIT_ARGS+=($pid "$phase")
done

wait_all "${P3_WAIT_ARGS[@]}"

# ── Aggregate results ─────────────────────────────────────────────────────────
TOTAL_PASS=0; TOTAL_FAIL=0; ALL_FAIL_MSGS=()
for cf in "${ALL_PHASE_COUNTS[@]}"; do
  [ -f "$cf" ] || continue
  while IFS= read -r line; do
    case "$line" in
      PASS_COUNT=*) TOTAL_PASS=$((TOTAL_PASS + ${line#PASS_COUNT=})) ;;
      FAIL_COUNT=*) TOTAL_FAIL=$((TOTAL_FAIL + ${line#FAIL_COUNT=})) ;;
      FAIL_MSG=*)   ALL_FAIL_MSGS+=("${line#FAIL_MSG=}") ;;
    esac
  done < "$cf"
done

# ── Summary ───────────────────────────────────────────────────────────────────
source "$E2E_STATE" 2>/dev/null || true

echo ""
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "${CYAN}${BOLD}  Test Results${NC}"
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "  Round 1 (cold start) : ${GREEN}OK=${R1_OK:-?}${NC}  Queued=${R1_Q:-?}  Failed=${R1_F:-?}"
echo -e "  Round 2 (AIMD)       : ${GREEN}OK=${R2_OK:-?}${NC}  Queued=${R2_Q:-?}  Failed=${R2_F:-?}"
echo -e "  AIMD limit           : ${AIMD_LIMIT:-unknown}"
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

[ "$PARALLEL_EXIT" -ne 0 ] && [ "$TOTAL_FAIL" -eq 0 ] && TOTAL_FAIL=1
exit "$TOTAL_FAIL"
