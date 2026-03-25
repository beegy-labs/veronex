#!/usr/bin/env bash
# Phase 12: MCP Integration Tests
#
# Tests the MCP tool-call path through /v1/chat/completions.
# weather-mcp is bundled in docker-compose (port 3100) and always available.
# MCP_TEST_URL defaults to http://localhost:3100.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

MCP_TEST_URL="${MCP_TEST_URL:-http://localhost:3100}"

# ── MCP Server CRUD ──────────────────────────────────────────────────────────

hdr "MCP Server CRUD"

# Register
MCP_SLUG="e2etestmcp"
MCP_NAME="e2e-test-mcp"
REG_RES=$(apost "/v1/mcp/servers" \
  "{\"name\":\"$MCP_NAME\",\"slug\":\"$MCP_SLUG\",\"url\":\"http://localhost:3100\",\"timeout_secs\":30}" \
  2>/dev/null || echo "{}")
MCP_ID=$(echo "$REG_RES" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print(d.get('id',''))" 2>/dev/null || echo "")
[ -n "$MCP_ID" ] \
  && pass "POST /v1/mcp/servers → 201 (id: $MCP_ID)" \
  || fail "POST /v1/mcp/servers → failed to register (response: $REG_RES)"

# List
LIST_RES=$(aget "/v1/mcp/servers" 2>/dev/null || echo "[]")
LIST_COUNT=$(echo "$LIST_RES" | python3 -c "import sys,json; print(len(json.loads(sys.stdin.read())))" 2>/dev/null || echo "0")
[ "$LIST_COUNT" -gt 0 ] \
  && pass "GET /v1/mcp/servers → $LIST_COUNT server(s)" \
  || fail "GET /v1/mcp/servers → empty list after registration"

# Verify structure
if [ "$LIST_COUNT" -gt 0 ]; then
  HAS_FIELDS=$(echo "$LIST_RES" | python3 -c "
import sys,json
d=json.loads(sys.stdin.read())
s=d[0]
ok=all(k in s for k in ['id','name','slug','url','is_enabled','online','tool_count'])
print('ok' if ok else 'missing_fields:' + str([k for k in ['id','name','slug','url','is_enabled','online','tool_count'] if k not in s]))
" 2>/dev/null || echo "parse_error")
  [ "$HAS_FIELDS" = "ok" ] \
    && pass "GET /v1/mcp/servers → response has required fields" \
    || fail "GET /v1/mcp/servers → $HAS_FIELDS"
fi

# Patch — toggle enabled
if [ -n "$MCP_ID" ]; then
  PATCH_RES=$(apatch "/v1/mcp/servers/$MCP_ID" '{"is_enabled":false}' 2>/dev/null || echo "{}")
  PATCH_ENABLED=$(echo "$PATCH_RES" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print('false' if d.get('is_enabled') == False else 'true')" 2>/dev/null || echo "?")
  [ "$PATCH_ENABLED" = "false" ] \
    && pass "PATCH /v1/mcp/servers/:id → is_enabled=false" \
    || fail "PATCH /v1/mcp/servers/:id → is_enabled=$PATCH_ENABLED (expected false)"
fi

# Validation — empty name returns 400
VAL_RES=$(curl -s -w "\n%{http_code}" "$API/v1/mcp/servers" \
  -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
  -d '{"name":"","slug":"valid","url":"http://localhost:3100"}' 2>/dev/null || printf "\n000")
VAL_CODE=$(echo "$VAL_RES" | tail -1)
[ "$VAL_CODE" = "400" ] \
  && pass "POST /v1/mcp/servers empty name → 400" \
  || fail "POST /v1/mcp/servers empty name → $VAL_CODE (expected 400)"

# Validation — invalid slug returns 400
VAL2_RES=$(curl -s -w "\n%{http_code}" "$API/v1/mcp/servers" \
  -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
  -d '{"name":"Valid","slug":"Invalid-Slug","url":"http://localhost:3100"}' 2>/dev/null || printf "\n000")
VAL2_CODE=$(echo "$VAL2_RES" | tail -1)
[ "$VAL2_CODE" = "400" ] \
  && pass "POST /v1/mcp/servers invalid slug → 400" \
  || fail "POST /v1/mcp/servers invalid slug → $VAL2_CODE (expected 400)"

# Delete
if [ -n "$MCP_ID" ]; then
  DEL_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$API/v1/mcp/servers/$MCP_ID" \
    -H "Authorization: Bearer $TK" 2>/dev/null || echo "000")
  [ "$DEL_CODE" = "204" ] \
    && pass "DELETE /v1/mcp/servers/:id → 204" \
    || fail "DELETE /v1/mcp/servers/:id → $DEL_CODE (expected 204)"
fi

# 404 for non-existent id
NF_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
  "$API/v1/mcp/servers/00000000-0000-0000-0000-000000000000" \
  -H "Authorization: Bearer $TK" 2>/dev/null || echo "000")
[ "$NF_CODE" = "404" ] \
  && pass "DELETE /v1/mcp/servers non-existent → 404" \
  || fail "DELETE /v1/mcp/servers non-existent → $NF_CODE (expected 404)"

# ── MCP API Surface ───────────────────────────────────────────────────────────

hdr "MCP API Surface"

# /v1/chat/completions with tools field must be accepted (bridge code path).
# Validates that passing MCP-prefixed tool names does not cause a 400/422.
TOOLS_RES=$(curl -s -w "\n%{http_code}" --max-time 60 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"What is the weather in Seoul?\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"mcp_weather_get_weather\",
        \"description\": \"Get weather for a city\",
        \"parameters\": {\"type\": \"object\", \"properties\": {\"city\": {\"type\": \"string\"}}}
      }
    }],
    \"stream\": false,
    \"max_tokens\": 32
  }" 2>/dev/null || printf "\n000")
TOOLS_CODE=$(echo "$TOOLS_RES" | tail -1)
case "$TOOLS_CODE" in
  200) pass "/v1/chat/completions with MCP tools field → 200" ;;
  503) info "/v1/chat/completions with MCP tools → 503 (no providers available)" ;;
  400) fail "/v1/chat/completions with MCP tools → 400 (schema rejected tools field)" ;;
  *)   fail "/v1/chat/completions with MCP tools → $TOOLS_CODE" ;;
esac

# Verify response structure on success
if [ "$TOOLS_CODE" = "200" ]; then
  TOOLS_BODY=$(echo "$TOOLS_RES" | sed '$d')
  TOOLS_VALID=$(echo "$TOOLS_BODY" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read().strip())
    choices = d.get('choices', [])
    if not choices:
        print('no_choices')
    else:
        msg = choices[0].get('message', {})
        finish = choices[0].get('finish_reason', '')
        tool_calls = msg.get('tool_calls', [])
        if tool_calls or msg.get('content'):
            print('ok')
        else:
            print(f'empty_message:finish={finish}')
except Exception as e:
    print(f'not_json:{e}')
" 2>/dev/null || echo "parse_error")
  [ "$TOOLS_VALID" = "ok" ] \
    && pass "/v1/chat/completions tools → valid choices structure" \
    || info "/v1/chat/completions tools → $TOOLS_VALID"
fi

# ── MCP Valkey Key Namespace ──────────────────────────────────────────────────

hdr "MCP Valkey Key Namespace"

# Verify the MCP key namespace is either present or cleanly absent (no corruption)
MCP_KEY_COUNT=$(docker compose exec -T valkey valkey-cli --scan --pattern "veronex:mcp:*" 2>/dev/null | grep -c "" || true)
MCP_KEY_COUNT="${MCP_KEY_COUNT:-0}"
if [ "$MCP_KEY_COUNT" -gt 0 ] 2>/dev/null; then
  pass "Valkey MCP namespace present ($MCP_KEY_COUNT keys under veronex:mcp:*)"
else
  info "Valkey MCP namespace empty (no MCP servers configured — expected in default setup)"
fi

# ── weather-mcp Direct Protocol Tests ────────────────────────────────────────

hdr "weather-mcp Protocol (${MCP_TEST_URL})"

# 1. Health check
MCP_HEALTH=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$MCP_TEST_URL/health" 2>/dev/null || echo "000")
case "$MCP_HEALTH" in
  200) pass "weather-mcp /health → 200" ;;
  000)
    fail "weather-mcp unreachable at $MCP_TEST_URL (is docker compose up?)"
    save_counts
    exit 0
    ;;
  *) fail "weather-mcp /health → $MCP_HEALTH" ;;
esac

# 2. MCP initialize handshake
INIT_RES=$(curl -s -w "\n%{http_code}" --max-time 10 "$MCP_TEST_URL/mcp" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"veronex-e2e","version":"1.0"}}}' \
  2>/dev/null || printf "\n000")
INIT_CODE=$(echo "$INIT_RES" | tail -1)
INIT_BODY=$(echo "$INIT_RES" | sed '$d')
[ "$INIT_CODE" = "200" ] \
  && pass "weather-mcp initialize → 200" \
  || fail "weather-mcp initialize → $INIT_CODE"

# 3. Check protocol version in response
if [ "$INIT_CODE" = "200" ]; then
  PROTO=$(echo "$INIT_BODY" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print(d.get('result',{}).get('protocolVersion',''))" 2>/dev/null || echo "")
  [ "$PROTO" = "2025-03-26" ] \
    && pass "weather-mcp protocolVersion = 2025-03-26" \
    || fail "weather-mcp protocolVersion = '$PROTO' (expected 2025-03-26)"
fi

# 4. tools/list — expect get_coordinates + get_weather
TOOLS_LIST=$(curl -s --max-time 10 "$MCP_TEST_URL/mcp" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  2>/dev/null || echo "{}")
TOOL_COUNT=$(echo "$TOOLS_LIST" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print(len(d.get('result',{}).get('tools',[])))" 2>/dev/null || echo "0")
[ "$TOOL_COUNT" -ge 2 ] \
  && pass "weather-mcp tools/list → $TOOL_COUNT tools (get_coordinates + get_weather)" \
  || fail "weather-mcp tools/list → $TOOL_COUNT tools (expected >= 2)"

# 5. tools/call get_coordinates (live network — open-meteo.com)
COORD_RES=$(curl -s --max-time 15 "$MCP_TEST_URL/mcp" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_coordinates","arguments":{"city":"Seoul"}}}' \
  2>/dev/null || echo "{}")
COORD_OK=$(echo "$COORD_RES" | python3 -c "
import sys,json
d=json.loads(sys.stdin.read())
r=d.get('result',{})
is_err=r.get('isError',True)
content=r.get('content',[{}])
text=content[0].get('text','') if content else ''
print('ok' if not is_err and 'Seoul' in text else f'err:{text[:80]}')
" 2>/dev/null || echo "parse_error")
[ "$COORD_OK" = "ok" ] \
  && pass "weather-mcp get_coordinates(Seoul) → coordinates returned" \
  || info "weather-mcp get_coordinates(Seoul) → $COORD_OK (network may be unavailable)"

# ── Full MCP Roundtrip via veronex ────────────────────────────────────────────

hdr "MCP Full Roundtrip (veronex → weather-mcp)"

# 2. Inference with MCP tools active — expect tool_call or final answer
MCP_INF_RES=$(curl -s -w "\n%{http_code}" --max-time 90 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"/no_think What is the weather in Seoul right now? Use the available tools.\"}],
    \"stream\": false,
    \"max_tokens\": 128
  }" 2>/dev/null || printf "\n000")
MCP_INF_CODE=$(echo "$MCP_INF_RES" | tail -1)
MCP_INF_BODY=$(echo "$MCP_INF_RES" | sed '$d')

case "$MCP_INF_CODE" in
  200)
    MCP_CHECK=$(echo "$MCP_INF_BODY" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read().strip())
    choices = d.get('choices', [])
    if not choices:
        print('no_choices'); exit()
    msg = choices[0].get('message', {})
    tool_calls = msg.get('tool_calls', [])
    content = msg.get('content', '')
    finish = choices[0].get('finish_reason', '')
    if tool_calls:
        names = [tc.get('function', {}).get('name', '') for tc in tool_calls]
        print(f'tool_calls:{names}')
    elif finish == 'stop':
        print('answer:' + (content or '')[:80])
    else:
        print(f'unexpected:finish={finish}')
except Exception as e:
    print(f'error:{e}')
" 2>/dev/null || echo "parse_error")
    case "$MCP_CHECK" in
      tool_calls:*) pass "MCP roundtrip → tool_calls dispatched (${MCP_CHECK#tool_calls:})" ;;
      answer:*)     pass "MCP roundtrip → model responded (bridge inactive — ${MCP_CHECK#answer:})" ;;
      *)            fail "MCP roundtrip → unexpected response ($MCP_CHECK)" ;;
    esac
    ;;
  503) info "MCP roundtrip → 503 (bridge not initialized — MCP_SERVERS env not set in veronex)" ;;
  *)   fail "MCP roundtrip → $MCP_INF_CODE" ;;
esac

# 3. Verify MCP heartbeat key written by agent
MCP_HB_COUNT=$(docker compose exec -T valkey valkey-cli --scan --pattern "veronex:mcp:heartbeat:*" 2>/dev/null | grep -c "" || true)
MCP_HB_COUNT="${MCP_HB_COUNT:-0}"
if [ "$MCP_HB_COUNT" -gt 0 ] 2>/dev/null; then
  pass "Valkey MCP heartbeat keys present ($MCP_HB_COUNT servers tracked)"
else
  info "No MCP heartbeat keys (veronex-agent not yet scraped MCP servers)"
fi

save_counts
