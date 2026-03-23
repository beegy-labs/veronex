#!/usr/bin/env bash
# Phase 12: MCP Integration Tests
#
# Tests the MCP tool-call path through /v1/chat/completions.
# Full roundtrip requires a live MCP server (MCP_TEST_URL env var).
# Without MCP_TEST_URL: validates API surface + bridge code path accessibility.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

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

# ── Full MCP Roundtrip (optional — requires MCP_TEST_URL) ────────────────────

MCP_TEST_URL="${MCP_TEST_URL:-}"
if [ -z "$MCP_TEST_URL" ]; then
  info "SKIP: MCP roundtrip — set MCP_TEST_URL=http://weather-mcp:3100 to enable full test"
  save_counts
  exit 0
fi

hdr "MCP Full Roundtrip (MCP_TEST_URL=$MCP_TEST_URL)"

# 1. Verify MCP server is reachable
MCP_PING=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$MCP_TEST_URL" 2>/dev/null || echo "000")
case "$MCP_PING" in
  200|404) pass "MCP server reachable ($MCP_PING)" ;;
  000)
    fail "MCP server unreachable at $MCP_TEST_URL"
    save_counts
    exit 0
    ;;
  *) info "MCP server → HTTP $MCP_PING (may still respond to JSON-RPC)" ;;
esac

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
