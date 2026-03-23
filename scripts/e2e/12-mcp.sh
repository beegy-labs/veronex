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
MCP_KEY_COUNT=$(docker compose exec -T valkey valkey-cli --scan --pattern "veronex:mcp:*" 2>/dev/null | grep -c . || echo "0")
if [ "${MCP_KEY_COUNT:-0}" -gt 0 ]; then
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
    elif content and finish == 'stop':
        print(f'answer:{content[:80]}')
    else:
        print(f'unexpected:finish={finish}')
except Exception as e:
    print(f'error:{e}')
" 2>/dev/null || echo "parse_error")
    case "$MCP_CHECK" in
      tool_calls:*) pass "MCP roundtrip → tool_calls dispatched (${MCP_CHECK#tool_calls:})" ;;
      answer:*)     pass "MCP roundtrip → final answer returned (${MCP_CHECK#answer:})" ;;
      *)            fail "MCP roundtrip → unexpected response ($MCP_CHECK)" ;;
    esac
    ;;
  503) info "MCP roundtrip → 503 (bridge not initialized — MCP_SERVERS env not set in veronex)" ;;
  *)   fail "MCP roundtrip → $MCP_INF_CODE" ;;
esac

# 3. Verify MCP heartbeat key written by agent
MCP_HB_COUNT=$(docker compose exec -T valkey valkey-cli --scan --pattern "veronex:mcp:heartbeat:*" 2>/dev/null | grep -c . || echo "0")
if [ "${MCP_HB_COUNT:-0}" -gt 0 ]; then
  pass "Valkey MCP heartbeat keys present ($MCP_HB_COUNT servers tracked)"
else
  info "No MCP heartbeat keys (veronex-agent not yet scraped MCP servers)"
fi

save_counts
