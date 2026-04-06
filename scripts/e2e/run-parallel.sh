#!/usr/bin/env bash
# run-parallel.sh — Parallel E2E test runner
#
# Execution model:
#   Phase 0  (sequential) : 01-setup — DB reset + auth + providers + API keys
#   Wave  1  (parallel)   : 05-security  09-metrics  11-liveness  13-frontend
#   Wave  2  (parallel)   : 04-crud  06-api-surface  10-image-storage  12-mcp  15-vision-fallback  17-mcp-analytics
#   Wave  3  (sequential) : 02-scheduler  03-inference  07-lifecycle  08-sdd-advanced  16-context-compression  14-vespa-load-test
#
# Wave 1: read-heavy / fully isolated — safe to run in parallel.
# Wave 2: create their own resources; 12-mcp/17 use unique slug via E2E_RUN_ID.
# Wave 3: share AIMD + provider state → must run sequentially.
#         16-context-compression patches global lab settings — sequential to avoid conflicts.
#         14-vespa-load-test is write-heavy (100K docs) — moved here so it runs
#         after veronex-agent's post-restart re-index settles.
#
# Env vars:
#   SKIP_SETUP=1    skip Phase 0 (01-setup) — use when state already exists
#   E2E_RUN_ID      unique suffix for resource isolation (default: timestamp)

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# ── Run-level isolation ───────────────────────────────────────────────────────
# E2E_RUN_ID: unique suffix for per-run resource names (e.g. MCP slugs).
# E2E_STATE: shared state file — use the default path so scripts find auth
#            credentials saved by 01-setup.sh.
export E2E_RUN_ID="${E2E_RUN_ID:-$(date +%s)}"

RED='\033[0;31m'; GREEN='\033[0;32m'; CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'
TOTAL_PASS=0; TOTAL_FAIL=0

# ── Helpers ───────────────────────────────────────────────────────────────────
_count() { python3 -c "import sys; print(sys.stdin.read().count('$1'))"; }

_print_result() {
  local label="$1" out="$2" failed="$3" p f
  p=$(echo "$out" | _count '[PASS]')
  f=$(echo "$out" | _count '[FAIL]')
  if [ "$f" -gt 0 ] || [ "$failed" = "1" ]; then
    echo -e "  ${RED}[FAIL]${NC} $label  pass=$p fail=$f"
    echo "$out" | grep '\[FAIL\]' | head -10 | sed 's/^/         /' || true
  else
    echo -e "  ${GREEN}[PASS]${NC} $label  pass=$p"
  fi
  TOTAL_PASS=$((TOTAL_PASS + p))
  TOTAL_FAIL=$((TOTAL_FAIL + f))
}

_run_one() {
  local label="$1" script="$2" tmpf out failed
  tmpf=$(mktemp)
  failed=0
  bash "$script" > "$tmpf" 2>&1 || failed=1
  out=$(cat "$tmpf"); rm -f "$tmpf"
  _print_result "$label" "$out" "$failed"
}

# _run_wave <script1> <script2> ...
# Launches all in background; collects results in launch order.
# Uses per-script temp files — no associative arrays (bash 3 compat).
_run_wave() {
  local WAVE_TMPDIR; WAVE_TMPDIR=$(mktemp -d)

  for entry in "$@"; do
    local s; s=$(basename "$entry" .sh)
    bash "$SCRIPT_DIR/$entry.sh" > "$WAVE_TMPDIR/$s.out" 2>&1 &
    echo $! > "$WAVE_TMPDIR/$s.pid"
  done

  for entry in "$@"; do
    local s out failed
    s=$(basename "$entry" .sh)
    failed=0
    wait "$(cat "$WAVE_TMPDIR/$s.pid")" || failed=1
    out=$(cat "$WAVE_TMPDIR/$s.out")
    _print_result "$s" "$out" "$failed"
  done

  rm -rf "$WAVE_TMPDIR"
}

# ── Phase 0: Setup ────────────────────────────────────────────────────────────
if [ "${SKIP_SETUP:-0}" = "0" ]; then
  echo -e "\n${CYAN}${BOLD}── Phase 0: Setup ──${NC}"
  _run_one "01-setup" "$SCRIPT_DIR/01-setup.sh"
fi

# ── Wave 1: Read-only / independent (parallel) ───────────────────────────────
echo -e "\n${CYAN}${BOLD}── Wave 1 (parallel): security · metrics · liveness · frontend ──${NC}"
_run_wave 05-security 09-metrics-pipeline 11-verify-liveness 13-frontend

# ── Wave 2: Feature tests with isolated resources (parallel) ─────────────────
echo -e "\n${CYAN}${BOLD}── Wave 2 (parallel): crud · api-surface · image-storage · mcp · vision-fallback · mcp-analytics ──${NC}"
_run_wave 04-crud 06-api-surface 10-image-storage 12-mcp 15-vision-fallback 17-mcp-analytics

# ── Wave 3: Inference pipeline + Vespa load test (sequential) ────────────────
# 14-vespa-load-test is write-heavy (100K docs) — runs after agent re-index settles.
# 16-context-compression patches global lab settings — sequential to avoid conflicts.
echo -e "\n${CYAN}${BOLD}── Wave 3 (sequential): scheduler · inference · lifecycle · sdd-advanced · context-compression · vespa-load-test ──${NC}"
for s in 02-scheduler 03-inference 07-lifecycle 08-sdd-advanced 16-context-compression 14-vespa-load-test; do
  _run_one "$s" "$SCRIPT_DIR/$s.sh"
done

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
TOTAL=$((TOTAL_PASS + TOTAL_FAIL))
if [ "$TOTAL_FAIL" -eq 0 ]; then
  echo -e "${GREEN}${BOLD}ALL PASS${NC}  total=$TOTAL  pass=$TOTAL_PASS  fail=0"
else
  echo -e "${RED}${BOLD}FAILURES${NC}  total=$TOTAL  pass=$TOTAL_PASS  fail=${TOTAL_FAIL}"
  exit 1
fi
