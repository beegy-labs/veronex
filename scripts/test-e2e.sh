#!/usr/bin/env bash
# ── Veronex E2E Integration Test ─────────────────────────────────────────────
#
# Validates the full scheduler stack with dual Ollama providers:
#   - Local:  OLLAMA_LOCAL  (default: http://localhost:11434)  + NODE_EXPORTER_LOCAL  (default: http://localhost:9100)
#   - Remote: OLLAMA_REMOTE (default: https://ollama.girok.dev) + NODE_EXPORTER_REMOTE (default: http://192.168.1.21:9100)
#
# Usage:
#   ./scripts/test-e2e.sh
#   SKIP_DB_RESET=1 ./scripts/test-e2e.sh
#   MODEL=qwen3:8b CONCURRENT=8 ./scripts/test-e2e.sh
#   OLLAMA_LOCAL=http://localhost:11434 OLLAMA_REMOTE=https://ollama.girok.dev ./scripts/test-e2e.sh
#
# Phase execution:
#   Sequential : 01-setup → 03-inference
#   Parallel   : 02-scheduler, 04-crud, 05-security, 06-api-surface, 07-lifecycle
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

# ── Banner ────────────────────────────────────────────────────────────────────
CYAN='\033[0;36m'; BOLD='\033[1m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "${CYAN}${BOLD}  Veronex E2E — Dual-Provider Scheduler Test${NC}"
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "  ${CYAN}API        = ${API_URL:-http://localhost:3001}${NC}"
echo -e "  ${CYAN}Local      = ${OLLAMA_LOCAL:-http://localhost:11434} (node: ${NODE_EXPORTER_LOCAL:-http://localhost:9100})${NC}"
echo -e "  ${CYAN}Remote     = ${OLLAMA_REMOTE:-https://ollama.girok.dev} (node: ${NODE_EXPORTER_REMOTE:-http://192.168.1.21:9100})${NC}"
echo -e "  ${CYAN}Model      = ${MODEL:-qwen3:8b}  Concurrency = ${CONCURRENT:-6}${NC}"
echo ""

# ── Sequential phases (state must be established before parallel) ─────────────
echo -e "${CYAN}${BOLD}[01] Setup: infra + auth + dual providers + API keys${NC}"
bash "$E2E_DIR/01-setup.sh"

echo -e "${CYAN}${BOLD}[03] Inference: concurrent bursts + AIMD learning${NC}"
bash "$E2E_DIR/03-inference.sh"

# ── Parallel phases ───────────────────────────────────────────────────────────
PARALLEL_PHASES=(
  "02-scheduler.sh"    # ZSET queue, thermal, num_parallel, AIMD constraint
  "04-crud.sh"         # Account / Key / Provider(num_parallel) / Server CRUD
  "05-security.sh"     # Auth edge cases, SSRF, rate limiting, RBAC
  "06-api-surface.sh"  # Multi-format inference, endpoint smoke tests
  "07-lifecycle.sh"    # Job cancel, SSE, native API, password reset, edge cases
)
PARALLEL_PIDS=()
PARALLEL_COUNTS=()
PARALLEL_EXIT=0

echo ""
echo -e "${CYAN}${BOLD}Running parallel phases: ${PARALLEL_PHASES[*]}${NC}"

for phase in "${PARALLEL_PHASES[@]}"; do
  phase_counts="$COUNTS_FILE.${phase%.sh}"
  : > "$phase_counts"
  PARALLEL_COUNTS+=("$phase_counts")
  E2E_COUNTS_FILE="$phase_counts" bash "$E2E_DIR/$phase" &
  PARALLEL_PIDS+=($!)
done

for i in "${!PARALLEL_PIDS[@]}"; do
  if ! wait "${PARALLEL_PIDS[$i]}"; then
    echo -e "${RED}[ERROR]${NC} ${PARALLEL_PHASES[$i]} exited non-zero" >&2
    PARALLEL_EXIT=1
  fi
done

# ── Sequential post-parallel phase: SDD advanced tests ──────────────────────
echo ""
echo -e "${CYAN}${BOLD}[08] SDD Advanced: AIMD decrease, multi-model, scale-in/out, thermal${NC}"
SDD_COUNTS="$COUNTS_FILE.08-sdd-advanced"
: > "$SDD_COUNTS"
if E2E_COUNTS_FILE="$SDD_COUNTS" bash "$E2E_DIR/08-sdd-advanced.sh"; then
  true
else
  echo -e "${RED}[ERROR]${NC} 08-sdd-advanced.sh exited non-zero" >&2
  PARALLEL_EXIT=1
fi

# ── Sequential post-parallel phase: Metrics Pipeline ────────────────────────
echo ""
echo -e "${CYAN}${BOLD}[09] Metrics Pipeline: Agent → OTel → ClickHouse${NC}"
METRICS_COUNTS="$COUNTS_FILE.09-metrics-pipeline"
: > "$METRICS_COUNTS"
if E2E_COUNTS_FILE="$METRICS_COUNTS" bash "$E2E_DIR/09-metrics-pipeline.sh"; then
  true
else
  echo -e "${RED}[ERROR]${NC} 09-metrics-pipeline.sh exited non-zero" >&2
  PARALLEL_EXIT=1
fi

# ── Sequential post-parallel phase: Image Storage ───────────────────────────
echo ""
echo -e "${CYAN}${BOLD}[10] Image Storage: S3 WebP + provider_name (API + Test)${NC}"
IMG_COUNTS="$COUNTS_FILE.10-image-storage"
: > "$IMG_COUNTS"
if E2E_COUNTS_FILE="$IMG_COUNTS" bash "$E2E_DIR/10-image-storage.sh"; then
  true
else
  echo -e "${RED}[ERROR]${NC} 10-image-storage.sh exited non-zero" >&2
  PARALLEL_EXIT=1
fi

# ── Sequential post-parallel phase: Verify & Liveness ───────────────────────
echo ""
echo -e "${CYAN}${BOLD}[11] Verify & Liveness: server/provider verify, duplicate 409, heartbeat${NC}"
VERIFY_COUNTS="$COUNTS_FILE.11-verify-liveness"
: > "$VERIFY_COUNTS"
if E2E_COUNTS_FILE="$VERIFY_COUNTS" bash "$E2E_DIR/11-verify-liveness.sh"; then
  true
else
  echo -e "${RED}[ERROR]${NC} 11-verify-liveness.sh exited non-zero" >&2
  PARALLEL_EXIT=1
fi

# ── Aggregate results ─────────────────────────────────────────────────────────
ALL_COUNTS_FILES=("$COUNTS_FILE" "${PARALLEL_COUNTS[@]}" "$SDD_COUNTS" "$METRICS_COUNTS" "$IMG_COUNTS" "$VERIFY_COUNTS")
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
