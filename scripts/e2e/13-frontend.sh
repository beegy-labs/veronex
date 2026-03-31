#!/usr/bin/env bash
# Phase 13: Frontend E2E Tests (Playwright)
#
# Runs Playwright tests against the running veronex-web instance.
# Requires: veronex-web up at WEB_URL (default http://localhost:3002)
#           playwright installed: cd web && npx playwright install
#
# Env vars:
#   WEB_URL          — frontend base URL (default http://localhost:3002)
#   PLAYWRIGHT_GREP  — filter tests by pattern (optional)
#   PW_WORKERS       — parallelism (default 4)
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; ensure_auth

WEB_URL="${WEB_URL:-http://localhost:3002}"
WEB_DIR="$(cd "$SCRIPT_DIR/../../web" && pwd)"
PW_WORKERS="${PW_WORKERS:-4}"

# ── Clear login rate limit before Playwright login ────────────────────────────
# Phase 05 fires 11 failed login attempts to test rate limiting. Clear all
# IP-based counters so Playwright global-setup login isn't blocked by a 429.
_REPO_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null || echo "$SCRIPT_DIR/../..")"
_RLKEYS=$(docker compose -f "$_REPO_ROOT/docker-compose.yml" exec -T valkey \
  valkey-cli KEYS 'veronex:login_attempts:*' 2>/dev/null | tr -d '\r')
if [ -n "$_RLKEYS" ]; then
  # shellcheck disable=SC2086
  docker compose -f "$_REPO_ROOT/docker-compose.yml" exec -T valkey \
    valkey-cli del $_RLKEYS > /dev/null 2>&1 || true
fi
unset _REPO_ROOT _RLKEYS

# ── Preflight ─────────────────────────────────────────────────────────────────

hdr "Frontend Preflight"

# Check web server is reachable
WEB_STATUS=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$WEB_URL" 2>/dev/null || echo "000")
case "$WEB_STATUS" in
  200|301|302|307|308)
    pass "veronex-web reachable at $WEB_URL (HTTP $WEB_STATUS)" ;;
  000)
    fail "veronex-web unreachable at $WEB_URL — is docker compose up?"
    save_counts
    exit 0 ;;
  *)
    info "veronex-web returned HTTP $WEB_STATUS — proceeding anyway" ;;
esac

# Check playwright is installed
if ! command -v npx &>/dev/null; then
  fail "npx not found — Node.js required for Playwright tests"
  save_counts
  exit 1
fi

PW_BINARY="$WEB_DIR/node_modules/.bin/playwright"
if [ ! -f "$PW_BINARY" ]; then
  fail "Playwright not installed — run: cd web && npm install"
  save_counts
  exit 1
fi

# Check browsers are installed (non-fatal — may work without explicit install)
if ! "$PW_BINARY" install --dry-run chromium &>/dev/null 2>&1; then
  info "Playwright browsers may not be installed — run: cd web && npx playwright install chromium"
fi

pass "Playwright found at $PW_BINARY"

# ── Run Playwright ────────────────────────────────────────────────────────────

hdr "Playwright Tests"

PW_ARGS=(
  "--workers=$PW_WORKERS"
  "--reporter=list"
  "--project=chromium"
)

[ -n "${PLAYWRIGHT_GREP:-}" ] && PW_ARGS+=("--grep=$PLAYWRIGHT_GREP")

# Playwright outputs its own pass/fail summary; we capture exit code only
PW_EXIT=0
(
  cd "$WEB_DIR"
  PLAYWRIGHT_BASE_URL="$WEB_URL" \
  PLAYWRIGHT_API_URL="${API:-http://localhost:3001}" \
  E2E_USERNAME="${E2E_USERNAME:-${USERNAME:-test}}" \
  E2E_PASSWORD="${E2E_PASSWORD:-${PASSWORD:-test1234!}}" \
  npx playwright test "${PW_ARGS[@]}" 2>&1
) || PW_EXIT=$?

if [ "$PW_EXIT" -eq 0 ]; then
  pass "Playwright test suite → all tests passed"
else
  fail "Playwright test suite → exit $PW_EXIT (see above for details)"
fi

# ── Per-spec summary ──────────────────────────────────────────────────────────

hdr "Playwright Spec Coverage"

SPEC_COUNT=$(find "$WEB_DIR/e2e" -name "*.spec.ts" | wc -l | tr -d ' ')
pass "Playwright spec files found: $SPEC_COUNT"

MCP_SPEC="$WEB_DIR/e2e/mcp.spec.ts"
[ -f "$MCP_SPEC" ] \
  && pass "mcp.spec.ts present" \
  || fail "mcp.spec.ts missing — MCP UI not covered"

save_counts
