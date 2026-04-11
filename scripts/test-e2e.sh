#!/usr/bin/env bash
# ── Veronex E2E Integration Test ─────────────────────────────────────────────
#
# Usage:
#   ./scripts/test-e2e.sh                    # Full run — skips already-passed phases
#   ./scripts/test-e2e.sh 05                 # Run single phase (05-security)
#   ./scripts/test-e2e.sh --from 05          # Clear checkpoints 05+, run from 05
#   ./scripts/test-e2e.sh --reset            # Clear all checkpoints, full run
#   ./scripts/test-e2e.sh --no-cache         # Ignore checkpoints, run all
#   SKIP_DB_RESET=1 ./scripts/test-e2e.sh
#
# Execution order:
#   Phase 1 (sequential) : 01-setup
#   Phase 2 (parallel)   : 03-inference  +  04-crud  05-security  09-metrics
#                          10-image  12-mcp
#   Phase 3 (parallel)   : 02-scheduler  06-api-surface  08-sdd-advanced
#     → Phase 3 starts only after 03-inference completes (AIMD state required)
#   Phase 3.5 (sequential): 07-lifecycle  (restarts veronex — must not run in parallel)
#   Phase 4 (sequential) : 13-frontend (Playwright UI tests)
#     → runs after all backend phases complete
#
# Checkpoints persist in: ${E2E_CHECKPOINT_DIR:-$HOME/.cache/veronex-e2e}
# A phase is skipped when its .ok file exists and --no-cache is not set.
# Checkpoint is written only when a phase exits 0 with FAIL_COUNT=0.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
E2E_DIR="$SCRIPT_DIR/e2e"

# ── Checkpoint directory (persistent across runs) ─────────────────────────────
CKPT="${E2E_CHECKPOINT_DIR:-$HOME/.cache/veronex-e2e}"
mkdir -p "$CKPT"

# State file persists auth tokens + API keys between runs
export E2E_STATE="$CKPT/state.env"

# ── Parse arguments ───────────────────────────────────────────────────────────
ONLY_PHASE=""   # run exactly one phase (e.g. "05" or "05-security")
FROM_PHASE=""   # clear checkpoints >= N, then run all
NO_CACHE=0      # ignore checkpoints (still writes new ones on pass)

while [ $# -gt 0 ]; do
  case "$1" in
    --reset)
      echo "Clearing all checkpoints in $CKPT"
      rm -f "$CKPT"/*.ok "$CKPT"/state.env
      shift ;;
    --reset-from|--from)
      FROM_PHASE="$2"; shift 2 ;;
    --no-cache)
      NO_CACHE=1; shift ;;
    --only)
      ONLY_PHASE="$2"; shift 2 ;;
    [0-9]*)
      ONLY_PHASE="$1"; shift ;;
    *)
      shift ;;
  esac
done

# Clear checkpoints for phases >= FROM_PHASE
if [ -n "$FROM_PHASE" ]; then
  prefix="${FROM_PHASE%%-*}"   # "05-security" → "05", "5" → "5"
  prefix=$(printf '%02d' "$((10#$prefix))")   # normalise to 2 digits
  echo "Clearing checkpoints for phases >= $prefix"
  for f in "$CKPT"/*.ok; do
    [ -f "$f" ] || continue
    phase_num=$(basename "$f" .ok | grep -oE '^[0-9]+')
    phase_num=$(printf '%02d' "$((10#$phase_num))")
    [ "$phase_num" -ge "$((10#$prefix))" ] && rm -f "$f"
  done
fi

# ── Counters ──────────────────────────────────────────────────────────────────
COUNTS_FILE="$CKPT/.run-counts"
: > "$COUNTS_FILE"
ALL_PHASE_COUNTS=("$COUNTS_FILE")
PARALLEL_EXIT=0

# ── Colors ────────────────────────────────────────────────────────────────────
CYAN='\033[0;36m'; BOLD='\033[1m'; GREEN='\033[0;32m'
RED='\033[0;31m'; YELLOW='\033[1;33m'; NC='\033[0m'

# ── Checkpoint helpers ────────────────────────────────────────────────────────
_phase_id()  { basename "${1%.sh}"; }   # "05-security.sh" → "05-security"

_is_cached() {
  local id; id=$(_phase_id "$1")
  [ "$NO_CACHE" -eq 0 ] && [ -f "$CKPT/$id.ok" ]
}

# Write .ok only when exit=0 AND FAIL_COUNT=0 in counts file
_try_mark_ok() {
  local phase="$1" exit_code="$2"
  local id; id=$(_phase_id "$phase")
  [ "$exit_code" -ne 0 ] && return
  local cf="$CKPT/$id.counts"
  local fails=0
  [ -f "$cf" ] && fails=$(grep "^FAIL_COUNT=" "$cf" | awk -F= '{s+=$2} END{print s+0}')
  if [ "$fails" -eq 0 ]; then
    date '+%Y-%m-%d %H:%M:%S' > "$CKPT/$id.ok"
  fi
}

_load_cached_counts() {
  local id; id=$(_phase_id "$1")
  local cf="$CKPT/$id.counts"
  [ -f "$cf" ] && ALL_PHASE_COUNTS+=("$cf")
}

# ── Phase runners ─────────────────────────────────────────────────────────────
run_phase() {
  local phase="$1"
  local id; id=$(_phase_id "$phase")

  if _is_cached "$phase"; then
    echo -e "  ${YELLOW}⏭  SKIP${NC} $phase  (passed $(cat "$CKPT/$id.ok"))"
    _load_cached_counts "$phase"
    return 0
  fi

  local cf="$CKPT/$id.counts"
  : > "$cf"
  ALL_PHASE_COUNTS+=("$cf")
  set +e
  E2E_COUNTS_FILE="$cf" bash "$E2E_DIR/$phase"
  local rc=$?
  set -e
  _try_mark_ok "$phase" "$rc"
  return "$rc"
}

_BG_PID=0

_launch_bg() {
  local phase="$1"
  local id; id=$(_phase_id "$phase")

  if _is_cached "$phase"; then
    echo -e "  ${YELLOW}⏭  SKIP${NC} $phase  (passed $(cat "$CKPT/$id.ok"))"
    _load_cached_counts "$phase"
    _BG_PID=0   # sentinel: no background job launched
    return 0
  fi

  local cf="$CKPT/$id.counts"
  : > "$cf"
  ALL_PHASE_COUNTS+=("$cf")
  E2E_COUNTS_FILE="$cf" bash "$E2E_DIR/$phase" &
  _BG_PID=$!
}

wait_all() {
  # Args: pid1 phase1 pid2 phase2 ...
  while [ $# -ge 2 ]; do
    local pid="$1" phase="$2"; shift 2
    [ "$pid" -eq 0 ] && continue   # was skipped
    local rc=0
    wait "$pid" || rc=$?
    _try_mark_ok "$phase" "$rc"
    if [ "$rc" -ne 0 ]; then
      echo -e "${RED}[ERROR]${NC} $phase exited $rc" >&2
      PARALLEL_EXIT=1
    fi
  done
}

# ── Single-phase mode ─────────────────────────────────────────────────────────
if [ -n "$ONLY_PHASE" ]; then
  # Normalise: "5" → "05", "05-security" → find matching file
  norm=$(printf '%02d' "$((10#${ONLY_PHASE%%-*}))")
  match=$(find "$E2E_DIR" -name "${norm}-*.sh" | head -1)
  if [ -z "$match" ]; then
    echo "No phase matching '$ONLY_PHASE'" >&2; exit 1
  fi
  phase=$(basename "$match")
  echo -e "${CYAN}${BOLD}── Single phase: $phase ──${NC}"
  run_phase "$phase"

  # Print mini-summary
  cf="$CKPT/$(_phase_id "$phase").counts"
  pass=0; fail=0
  [ -f "$cf" ] && { pass=$(grep "^PASS_COUNT=" "$cf" | awk -F= '{s+=$2} END{print s+0}');
                    fail=$(grep "^FAIL_COUNT=" "$cf" | awk -F= '{s+=$2} END{print s+0}'); }
  echo ""
  echo -e "  ${GREEN}PASS: $pass${NC}  ${RED}FAIL: $fail${NC}"
  exit "$fail"
fi

# ── Banner ────────────────────────────────────────────────────────────────────
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "${CYAN}${BOLD}  Veronex E2E — Dual-Provider Scheduler Test${NC}"
echo -e "${CYAN}${BOLD}══════════════════════════════════════════════${NC}"
echo -e "  ${CYAN}API    = ${API_URL:-http://localhost:3001}${NC}"
echo -e "  ${CYAN}Local  = ${OLLAMA_LOCAL:-http://localhost:11434}${NC}"
echo -e "  ${CYAN}Remote = ${OLLAMA_REMOTE:-https://ollama-1.kr1.girok.dev}${NC}"
echo -e "  ${CYAN}Model  = ${MODEL:-qwen3:8b}  Concurrency = ${CONCURRENT:-6}${NC}"
echo -e "  ${CYAN}Ckpts  = $CKPT${NC}"
echo ""

# ── Phase 1: Setup (must complete before anything else) ───────────────────────
# Always re-run setup — JWT tokens expire between runs, so state.env must be fresh
echo -e "${CYAN}${BOLD}[Phase 1] Setup: infra + auth + dual providers + API keys${NC}"
rm -f "$CKPT/01-setup.ok"
run_phase "01-setup.sh"

# ── Phase 2: Inference + independent tests (parallel) ────────────────────────
echo ""
echo -e "${CYAN}${BOLD}[Phase 2] Inference + independent tests (parallel)${NC}"

P2_WAIT_ARGS=()

# 03-inference must finish before Phase 3 (AIMD state)
_launch_bg "03-inference.sh"
P2_WAIT_ARGS+=("$_BG_PID" "03-inference.sh")

for phase in 04-crud.sh 05-security.sh 09-metrics-pipeline.sh 10-image-storage.sh 12-mcp.sh; do
  _launch_bg "$phase"
  P2_WAIT_ARGS+=("$_BG_PID" "$phase")
done

wait_all "${P2_WAIT_ARGS[@]}"

# ── Phase 3: AIMD-dependent tests (parallel) ─────────────────────────────────
# NOTE: 07-lifecycle.sh is excluded here because it restarts the veronex
# container, which would break concurrent requests in 06-api-surface.sh.
echo ""
echo -e "${CYAN}${BOLD}[Phase 3] AIMD-dependent tests (parallel)${NC}"

P3_WAIT_ARGS=()

for phase in 02-scheduler.sh 06-api-surface.sh 08-sdd-advanced.sh; do
  _launch_bg "$phase"
  P3_WAIT_ARGS+=("$_BG_PID" "$phase")
done

wait_all "${P3_WAIT_ARGS[@]}"

# ── Phase 3.5: Lifecycle test (sequential — restarts veronex) ─────────────────
echo ""
echo -e "${CYAN}${BOLD}[Phase 3.5] Lifecycle test (sequential — restarts veronex)${NC}"
run_phase "07-lifecycle.sh"

# ── Phase 4: Frontend E2E (Playwright) ───────────────────────────────────────
echo ""
echo -e "${CYAN}${BOLD}[Phase 4] Frontend E2E (Playwright)${NC}"
run_phase "13-frontend.sh"

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

# Show checkpoint status
echo ""
echo -e "  ${CYAN}Checkpoint status (passed phases cached for next run):${NC}"
for f in $(ls "$CKPT"/*.ok 2>/dev/null | sort); do
  id=$(basename "$f" .ok)
  echo -e "    ${GREEN}✓${NC} $id  ($(cat "$f"))"
done

[ "$PARALLEL_EXIT" -ne 0 ] && [ "$TOTAL_FAIL" -eq 0 ] && TOTAL_FAIL=1
exit "$TOTAL_FAIL"
