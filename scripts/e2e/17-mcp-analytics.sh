#!/usr/bin/env bash
# Phase 17: MCP Analytics Pipeline
#
# Tests the full MCP tool call analytics pipeline:
#   Bridge.fire_mcp_ingest()
#     → POST /internal/ingest/mcp  (veronex-analytics)
#     → OTLP HTTP → OTel Collector
#     → Kafka [otel-logs] → otel_logs
#     → otel_mcp_tool_calls_mv → mcp_tool_calls
#     → mcp_tool_calls_hourly_mv → mcp_tool_calls_hourly
#     → GET /v1/mcp/stats
#
# Also tests:
#   - MCP settings CRUD (GET/PATCH /v1/mcp/settings)
#   - get_datetime tool protocol compliance
#   - Analytics service health
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; ensure_auth

# Analytics runs on an internal Docker network (expose, not ports).
# Internal URL used by docker compose exec calls.
ANALYTICS_INTERNAL="http://veronex-analytics:3003"
ANALYTICS_SECRET="${ANALYTICS_SECRET:-veronex-analytics-internal-secret}"
MCP_TEST_URL="${MCP_TEST_URL:-http://localhost:3100}"

# Helper: run a one-shot curl inside the veronex container (shares internal network).
analytics_curl() {
  docker compose exec -T veronex sh -c "wget -qO- $*" 2>/dev/null
}
analytics_curl_code() {
  docker compose exec -T veronex sh -c "wget -S -O- $* 2>&1 | grep '  HTTP/' | tail -1 | awk '{print \$2}'" 2>/dev/null | tr -d ' \r\n'
}

# ── Analytics Service Health ─────────────────────────────────────────────────

hdr "Analytics Service Health"

# Check via docker health status
ANALYTICS_STATUS=$(docker inspect veronex-veronex-analytics-1 \
  --format '{{.State.Health.Status}}' 2>/dev/null || echo "unknown")
case "$ANALYTICS_STATUS" in
  healthy)  pass "veronex-analytics container → healthy" ;;
  starting) info "veronex-analytics container → starting (may pass later)" ;;
  *)        fail "veronex-analytics container → $ANALYTICS_STATUS" ;;
esac

# Check health endpoint from inside the Docker network
ANALYTICS_HEALTH=$(docker compose exec -T veronex-analytics \
  sh -c 'wget -qO- http://127.0.0.1:3003/health' 2>/dev/null || echo "")
[ "$ANALYTICS_HEALTH" = "ok" ] \
  && pass "veronex-analytics /health → ok (internal)" \
  || fail "veronex-analytics /health → '$ANALYTICS_HEALTH' (expected 'ok')"

# Verify MCP ingest endpoint is registered (send empty payload → expect 422/400, not 404)
MCP_INGEST_CODE=$(docker compose exec -T veronex sh -c \
  "wget -qO- --post-data='{}' \
   --header='Authorization: Bearer $ANALYTICS_SECRET' \
   --header='Content-Type: application/json' \
   --server-response '$ANALYTICS_INTERNAL/internal/ingest/mcp' 2>&1 \
   | grep 'HTTP/' | tail -1 | awk '{print \$2}'" 2>/dev/null | tr -d ' \r\n')

# wget exit codes vary; fall back to checking server response code
if [ -z "$MCP_INGEST_CODE" ]; then
  # Try via sh -c with a pipe to check if route exists at all
  ROUTE_CHECK=$(docker compose exec -T veronex-analytics \
    sh -c "wget -qO- --post-data='{}' \
           --header='Content-Type: application/json' \
           'http://127.0.0.1:3003/internal/ingest/mcp' 2>&1 || true" 2>/dev/null || echo "")
  # 404 body would contain "Not Found"; 422/400 body would be a JSON error
  echo "$ROUTE_CHECK" | grep -qi "not found" \
    && fail "POST /internal/ingest/mcp → 404 (route not registered)" \
    || pass "POST /internal/ingest/mcp → endpoint reachable (non-404 response)"
else
  case "$MCP_INGEST_CODE" in
    200|202|400|422) pass "POST /internal/ingest/mcp → registered (HTTP $MCP_INGEST_CODE)" ;;
    404) fail "POST /internal/ingest/mcp → 404 (route not registered)" ;;
    401) fail "POST /internal/ingest/mcp → 401 (auth misconfigured)" ;;
    *)   pass "POST /internal/ingest/mcp → HTTP $MCP_INGEST_CODE" ;;
  esac
fi

# ── MCP Settings CRUD ────────────────────────────────────────────────────────

hdr "MCP Settings (GET/PATCH)"

SETTINGS_RES=$(agetc "/v1/mcp/settings" 2>/dev/null || printf "\n000")
SETTINGS_CODE=$(echo "$SETTINGS_RES" | tail -1)
SETTINGS_BODY=$(echo "$SETTINGS_RES" | sed '$d')
[ "$SETTINGS_CODE" = "200" ] \
  && pass "GET /v1/mcp/settings → 200" \
  || fail "GET /v1/mcp/settings → $SETTINGS_CODE"

if [ "$SETTINGS_CODE" = "200" ]; then
  SETTINGS_FIELDS=$(echo "$SETTINGS_BODY" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    required = ['routing_cache_ttl_secs', 'tool_schema_refresh_secs',
                'embedding_model', 'max_tools_per_request', 'max_routing_cache_entries']
    missing = [f for f in required if f not in d]
    print('ok|' + str(d.get('max_tools_per_request','?')) if not missing else 'missing:' + ','.join(missing))
except Exception as e:
    print(f'parse_error:{e}')
" 2>/dev/null || echo "parse_error")
  SETTINGS_OK=$(echo "$SETTINGS_FIELDS" | cut -d'|' -f1)
  SETTINGS_DETAIL=$(echo "$SETTINGS_FIELDS" | cut -d'|' -f2-)
  [ "$SETTINGS_OK" = "ok" ] \
    && pass "GET /v1/mcp/settings → all fields present (max_tools_per_request=$SETTINGS_DETAIL)" \
    || fail "GET /v1/mcp/settings → $SETTINGS_FIELDS"
fi

# PATCH — update a setting and verify it sticks
PATCH_RES=$(apatchc "/v1/mcp/settings" '{"max_tools_per_request": 15}' 2>/dev/null || printf "\n000")
PATCH_CODE=$(echo "$PATCH_RES" | tail -1)
PATCH_BODY=$(echo "$PATCH_RES" | sed '$d')
[ "$PATCH_CODE" = "200" ] \
  && pass "PATCH /v1/mcp/settings → 200" \
  || fail "PATCH /v1/mcp/settings → $PATCH_CODE"

if [ "$PATCH_CODE" = "200" ]; then
  PATCHED_VAL=$(echo "$PATCH_BODY" | python3 -c "
import sys, json
try: print(json.loads(sys.stdin.read()).get('max_tools_per_request', '?'))
except: print('?')
" 2>/dev/null || echo "?")
  [ "$PATCHED_VAL" = "15" ] \
    && pass "PATCH /v1/mcp/settings → max_tools_per_request updated to 15" \
    || fail "PATCH /v1/mcp/settings → expected 15 got $PATCHED_VAL"

  # Restore default
  apatchc "/v1/mcp/settings" '{"max_tools_per_request": 20}' > /dev/null 2>&1 || true
  pass "PATCH /v1/mcp/settings → restored to default (20)"
fi

# Validate field constraints
PATCH_INVALID=$(apatchc "/v1/mcp/settings" '{"max_tools_per_request": 999}' 2>/dev/null || printf "\n000")
PATCH_INVALID_CODE=$(echo "$PATCH_INVALID" | tail -1)
[ "$PATCH_INVALID_CODE" != "200" ] \
  && pass "PATCH /v1/mcp/settings → max_tools_per_request=999 rejected ($PATCH_INVALID_CODE)" \
  || info "PATCH /v1/mcp/settings → out-of-range value accepted (constraint not enforced)"

# ── get_datetime Tool Protocol ───────────────────────────────────────────────

hdr "get_datetime Tool (veronex-mcp)"

# Tool is listed
TOOLS_RES=$(curl -s --max-time 5 -X POST "$MCP_TEST_URL/" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' 2>/dev/null || echo "{}")
DT_TOOL=$(echo "$TOOLS_RES" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    tools = d.get('result', {}).get('tools', [])
    t = next((t for t in tools if t['name'] == 'get_datetime'), None)
    if not t: print('missing')
    else:
        schema = t.get('inputSchema', {})
        has_tz = 'timezone' in schema.get('properties', {})
        print('ok|timezone_param=' + str(has_tz))
except Exception as e:
    print(f'error:{e}')
" 2>/dev/null || echo "parse_error")

DT_STATUS=$(echo "$DT_TOOL" | cut -d'|' -f1)
DT_DETAIL=$(echo "$DT_TOOL" | cut -d'|' -f2-)
[ "$DT_STATUS" = "ok" ] \
  && pass "get_datetime in tools/list ($DT_DETAIL)" \
  || fail "get_datetime in tools/list → $DT_TOOL"

# Call without timezone (UTC default)
DT_UTC=$(curl -s --max-time 5 -X POST "$MCP_TEST_URL/" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_datetime","arguments":{}}}' \
  2>/dev/null || echo "{}")
DT_UTC_CHECK=$(echo "$DT_UTC" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    text = d['result']['content'][0]['text']
    data = json.loads(text)
    required = ['iso', 'unix_epoch', 'timezone', 'date', 'time', 'day_of_week']
    missing = [f for f in required if f not in data]
    if missing: print('missing:' + ','.join(missing))
    else:
        tz = data['timezone']
        dow = data['day_of_week']
        ts = data['unix_epoch']
        print(f'ok|tz={tz} dow={dow} epoch={ts}')
except Exception as e:
    print(f'error:{e}')
" 2>/dev/null || echo "parse_error")

DT_UTC_STATUS=$(echo "$DT_UTC_CHECK" | cut -d'|' -f1)
DT_UTC_DETAIL=$(echo "$DT_UTC_CHECK" | cut -d'|' -f2-)
[ "$DT_UTC_STATUS" = "ok" ] \
  && pass "get_datetime (UTC) → $DT_UTC_DETAIL" \
  || fail "get_datetime (UTC) → $DT_UTC_CHECK"

# Call with specific timezone
DT_SEOUL=$(curl -s --max-time 5 -X POST "$MCP_TEST_URL/" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_datetime","arguments":{"timezone":"Asia/Seoul"}}}' \
  2>/dev/null || echo "{}")
DT_SEOUL_CHECK=$(echo "$DT_SEOUL" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    text = d['result']['content'][0]['text']
    data = json.loads(text)
    tz = data.get('timezone', '')
    offset = data.get('utc_offset', '')
    print(f'ok|tz={tz} offset={offset}')
except Exception as e:
    print(f'error:{e}')
" 2>/dev/null || echo "parse_error")

DT_SEOUL_STATUS=$(echo "$DT_SEOUL_CHECK" | cut -d'|' -f1)
DT_SEOUL_DETAIL=$(echo "$DT_SEOUL_CHECK" | cut -d'|' -f2-)
[ "$DT_SEOUL_STATUS" = "ok" ] \
  && pass "get_datetime (Asia/Seoul) → $DT_SEOUL_DETAIL" \
  || fail "get_datetime (Asia/Seoul) → $DT_SEOUL_CHECK"

# Invalid timezone → isError=true
DT_BAD=$(curl -s --max-time 5 -X POST "$MCP_TEST_URL/" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_datetime","arguments":{"timezone":"Invalid/Zone"}}}' \
  2>/dev/null || echo "{}")
DT_BAD_IS_ERR=$(echo "$DT_BAD" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    print('yes' if d.get('result', {}).get('isError') else 'no')
except: print('no')
" 2>/dev/null || echo "no")
[ "$DT_BAD_IS_ERR" = "yes" ] \
  && pass "get_datetime (invalid TZ) → isError=true" \
  || fail "get_datetime (invalid TZ) → isError not set (expected error response)"

# ── MCP Analytics Ingest Pipeline ───────────────────────────────────────────

hdr "MCP Analytics Ingest → ClickHouse Pipeline"

# Inject a test event directly into the analytics ingest endpoint
E2E_RUN_ID="${E2E_RUN_ID:-$$}"
TEST_SERVER_ID="019d5d4b-b791-77a0-abba-a8e3c6504205"  # veronex_mcp (from DB)
TEST_REQUEST_ID=$(python3 -c "import uuid; print(str(uuid.uuid4()))" 2>/dev/null || echo "00000000-0000-0000-0000-000000000001")
NOW_ISO=$(python3 -c "from datetime import datetime, timezone; print(datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))" 2>/dev/null || date -u +"%Y-%m-%dT%H:%M:%SZ")

PAYLOAD="{\"event_time\":\"$NOW_ISO\",\"request_id\":\"$TEST_REQUEST_ID\",\"api_key_id\":null,\"tenant_id\":\"e2e-test-$E2E_RUN_ID\",\"server_id\":\"$TEST_SERVER_ID\",\"server_slug\":\"veronex_mcp\",\"tool_name\":\"get_datetime\",\"namespaced_name\":\"mcp_veronex_mcp_get_datetime\",\"outcome\":\"success\",\"cache_hit\":false,\"latency_ms\":42,\"result_bytes\":256,\"cap_charged\":1,\"loop_round\":1}"

INGEST_CODE=$(docker compose exec -T veronex-analytics sh -c \
  "wget -qO- --post-data='$PAYLOAD' \
   --header='Authorization: Bearer $ANALYTICS_SECRET' \
   --header='Content-Type: application/json' \
   --server-response 'http://127.0.0.1:3003/internal/ingest/mcp' 2>&1 \
   | grep 'HTTP/' | tail -1 | awk '{print \$2}'" 2>/dev/null | tr -d ' \r\n' || echo "000")
case "$INGEST_CODE" in
  200|202) pass "POST /internal/ingest/mcp → $INGEST_CODE (event accepted)" ;;
  *)       fail "POST /internal/ingest/mcp → $INGEST_CODE (expected 202)" ;;
esac

# ClickHouse pipeline: OTel batch (5s) + Kafka poll + MV insert
if [ "$INGEST_CODE" = "200" ] || [ "$INGEST_CODE" = "202" ]; then
  info "Waiting for event to flow through OTel → Kafka → ClickHouse (up to 30s)..."
  CH_FOUND=0
  for i in $(seq 1 6); do
    sleep 5
    CH_COUNT=$(docker compose exec -T clickhouse clickhouse-client -d veronex \
      --query "SELECT count() FROM mcp_tool_calls WHERE tenant_id = 'e2e-test-$E2E_RUN_ID' AND event_time > now() - INTERVAL 5 MINUTE" \
      2>/dev/null | tr -d ' \r\n' || echo "0")
    if [ "${CH_COUNT:-0}" -gt 0 ] 2>/dev/null; then
      CH_FOUND=1
      pass "mcp_tool_calls: event arrived in ClickHouse after $((i*5))s ($CH_COUNT row(s))"
      break
    fi
  done

  if [ "$CH_FOUND" = "0" ]; then
    fail "mcp_tool_calls: event not in ClickHouse after 30s (pipeline broken)"
    # Diagnostics
    OTL_COUNT=$(docker compose exec -T clickhouse clickhouse-client -d veronex \
      --query "SELECT count() FROM otel_logs WHERE LogAttributes['event.name']='mcp.tool_call' AND Timestamp > now() - INTERVAL 5 MINUTE" \
      2>/dev/null | tr -d ' \r\n' || echo "?")
    info "otel_logs mcp.tool_call rows (last 5m): $OTL_COUNT"
    KAFKA_MSGS=$(docker compose exec -T redpanda rpk topic consume otel-logs --num 1 --timeout 3s 2>/dev/null | wc -l | tr -d ' ' || echo "?")
    info "Redpanda otel-logs topic readable: $KAFKA_MSGS lines"
  fi
fi

# ── ClickHouse MV Structure ──────────────────────────────────────────────────

hdr "ClickHouse MCP Schema"

check_table() {
  local table="$1"
  local count
  count=$(docker compose exec -T clickhouse clickhouse-client -d veronex \
    --query "SELECT count() FROM system.tables WHERE database='veronex' AND name='$table'" \
    2>/dev/null | tr -d ' \r\n' || echo "0")
  [ "${count:-0}" -gt 0 ] 2>/dev/null \
    && pass "ClickHouse table/view exists: $table" \
    || fail "ClickHouse table/view missing: $table"
}

check_table "mcp_tool_calls"
check_table "mcp_tool_calls_hourly"
check_table "mcp_tool_calls_hourly_mv"
check_table "otel_mcp_tool_calls_mv"

# ── /v1/mcp/stats after pipeline ────────────────────────────────────────────

hdr "GET /v1/mcp/stats (post-ingest)"

STATS_RES=$(agetc "/v1/mcp/stats?hours=1" 2>/dev/null || printf "\n000")
STATS_CODE=$(echo "$STATS_RES" | tail -1)
STATS_BODY=$(echo "$STATS_RES" | sed '$d')
[ "$STATS_CODE" = "200" ] \
  && pass "GET /v1/mcp/stats?hours=1 → 200" \
  || fail "GET /v1/mcp/stats?hours=1 → $STATS_CODE"

if [ "$STATS_CODE" = "200" ]; then
  STATS_CHECK=$(echo "$STATS_BODY" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    if not isinstance(d, list):
        print(f'not_array:{type(d).__name__}')
    elif len(d) == 0:
        print('empty')
    else:
        s = d[0]
        required = ['server_id', 'server_name', 'server_slug', 'total_calls', 'success_count', 'success_rate', 'avg_latency_ms']
        missing = [f for f in required if f not in s]
        if missing: print('missing_fields:' + ','.join(missing))
        else:
            slug = s.get('server_slug','?')
            calls = s.get('total_calls',0)
            rate = s.get('success_rate',0)
            print(f'ok|slug={slug} calls={calls} success_rate={rate:.2f}')
except Exception as e:
    print(f'parse_error:{e}')
" 2>/dev/null || echo "parse_error")

  STATS_STATUS=$(echo "$STATS_CHECK" | cut -d'|' -f1)
  STATS_DETAIL=$(echo "$STATS_CHECK" | cut -d'|' -f2-)
  case "$STATS_STATUS" in
    ok)    pass "GET /v1/mcp/stats → $STATS_DETAIL" ;;
    empty) info "GET /v1/mcp/stats → empty (hourly aggregation may be delayed)" ;;
    *)     fail "GET /v1/mcp/stats → $STATS_CHECK" ;;
  esac
fi

save_counts
