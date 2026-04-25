#!/usr/bin/env bash
# Phase 19: MCP cold-start self-heal
#
# Validates the reconciler path from main.rs:reconcile_mcp_sessions:
#   1. Stop veronex-mcp.
#   2. Restart veronex — at boot, MCP startup connect fails (logged WARN).
#   3. Start veronex-mcp.
#   4. Within MCP_TOOL_REFRESH_INTERVAL (25s), the periodic refresh tick
#      re-reads enabled MCP rows from DB, sees no active session for the
#      registered server, and reconnects → emits "MCP session reconnected".
#
# Sequential phase only — restarts veronex+veronex-mcp; cannot run in parallel.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; ensure_auth

hdr "MCP cold-start self-heal"

# Pick any enabled MCP server registered in DB. 12-mcp.sh leaves "weather_mcp"
# (slug) as a persistent sample; if absent, the test is meaningless so register
# a transient one against the in-cluster veronex-mcp service.
MCP_REGISTER_URL="${MCP_REGISTER_URL:-http://veronex-mcp:3100}"
TARGET_SLUG=$(docker compose exec -T postgres psql -U veronex -d veronex -tAc \
  "SELECT slug FROM mcp_servers WHERE is_enabled=true LIMIT 1" 2>/dev/null | tr -d '[:space:]')

CLEANUP_ID=""
if [ -z "$TARGET_SLUG" ]; then
  REG_SLUG="e2ecoldstart$$"
  REG_NAME="e2e-coldstart-$$"
  REG_RES=$(apost "/v1/mcp/servers" \
    "{\"name\":\"$REG_NAME\",\"slug\":\"$REG_SLUG\",\"url\":\"$MCP_REGISTER_URL\",\"timeout_secs\":30}" \
    2>/dev/null || echo "{}")
  CLEANUP_ID=$(echo "$REG_RES" | python3 -c "import sys,json;print(json.loads(sys.stdin.read()).get('id',''))" 2>/dev/null || echo "")
  if [ -z "$CLEANUP_ID" ]; then
    fail "Could not register MCP server for cold-start test"
    save_counts
    exit 0
  fi
  TARGET_SLUG="$REG_SLUG"
  pass "Registered transient MCP server for test (slug=$TARGET_SLUG)"
else
  info "Reusing existing MCP server (slug=$TARGET_SLUG) for cold-start test"
fi

# T0: stop veronex-mcp
docker compose stop veronex-mcp > /dev/null 2>&1
pass "Stopped veronex-mcp"

# T1: restart veronex (cold-start, no MCP reachable)
RESTART_TS=$(date -u +%s)
docker compose restart veronex > /dev/null 2>&1

# Wait for /health to come back
for _ in $(seq 1 30); do
  curl -sf "$API/health" > /dev/null 2>&1 && break
  sleep 1
done
[ "$(curl -s -o /dev/null -w '%{http_code}' "$API/health")" = "200" ] \
  && pass "veronex restarted and healthy" \
  || fail "veronex did not become healthy after restart"

# Verify the cold-start failure was logged
STARTUP_FAIL=$(docker logs --since 60s veronex-veronex-1 2>&1 | grep -c "MCP startup connect failed" || true)
[ "$STARTUP_FAIL" -ge 1 ] \
  && pass "Cold-start: \"MCP startup connect failed\" logged ($STARTUP_FAIL line)" \
  || fail "Cold-start: expected \"MCP startup connect failed\" log line, none found"

# T2: bring veronex-mcp back
docker compose start veronex-mcp > /dev/null 2>&1
# wait for veronex-mcp /health
for _ in $(seq 1 20); do
  curl -sf "http://localhost:3100/health" > /dev/null 2>&1 && break
  sleep 1
done

# T3: poll veronex logs for "MCP session reconnected" within MCP_TOOL_REFRESH_INTERVAL+buffer.
# Constant in domain/constants.rs is 25s — allow up to 40s for clock skew + first tick.
RECONNECTED=0
for _ in $(seq 1 40); do
  HITS=$(docker logs --since 60s veronex-veronex-1 2>&1 | grep -c "MCP session reconnected" || true)
  if [ "$HITS" -ge 1 ]; then
    RECONNECTED=1
    break
  fi
  sleep 1
done
ELAPSED=$(( $(date -u +%s) - RESTART_TS ))
[ "$RECONNECTED" = 1 ] \
  && pass "Self-heal: \"MCP session reconnected\" emitted within ${ELAPSED}s of restart" \
  || fail "Self-heal: \"MCP session reconnected\" not seen after 40s wait"

# Cleanup the transient registration if we created one.
if [ -n "$CLEANUP_ID" ]; then
  curl -s -o /dev/null -X DELETE "$API/v1/mcp/servers/$CLEANUP_ID" -H "Authorization: Bearer $TK" 2>/dev/null || true
fi

save_counts
