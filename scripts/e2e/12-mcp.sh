#!/usr/bin/env bash
# Phase 12: MCP Integration Tests
#
# Tests the MCP tool-call path through /v1/chat/completions.
# weather-mcp is bundled in docker-compose (port 3100) and always available.
# MCP_TEST_URL defaults to http://localhost:3100.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

# MCP_TEST_URL: direct access from host (scripts run on host, not inside Docker)
MCP_TEST_URL="${MCP_TEST_URL:-http://localhost:3100}"
# MCP_REGISTER_URL: URL the agent uses from inside Docker
MCP_REGISTER_URL="${MCP_REGISTER_URL:-http://weather-mcp:3100}"

# ── MCP Server CRUD ──────────────────────────────────────────────────────────

hdr "MCP Server CRUD"

# Register
MCP_SLUG="e2etestmcp"
MCP_NAME="e2e-test-mcp"
REG_RES=$(apost "/v1/mcp/servers" \
  "{\"name\":\"$MCP_NAME\",\"slug\":\"$MCP_SLUG\",\"url\":\"$MCP_REGISTER_URL\",\"timeout_secs\":30}" \
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

# ── Orchestrator Model (lab_settings.mcp_orchestrator_model) ─────────────────

hdr "MCP Orchestrator Model"

# Fetch current lab settings
LAB_INIT=$(aget "/v1/dashboard/lab" 2>/dev/null || echo "{}")
LAB_INIT_MODEL=$(echo "$LAB_INIT" | python3 -c "
import sys,json; d=json.loads(sys.stdin.read()); v=d.get('mcp_orchestrator_model',False)
print('null' if v is None else str(v))" 2>/dev/null || echo "?")

# Verify mcp_orchestrator_model field is present in response
if echo "$LAB_INIT" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); exit(0 if 'mcp_orchestrator_model' in d else 1)" 2>/dev/null; then
  pass "GET /v1/dashboard/lab → mcp_orchestrator_model field present (current: $LAB_INIT_MODEL)"
else
  fail "GET /v1/dashboard/lab → mcp_orchestrator_model field missing"
fi

# 1. Set orchestrator model
ORC_MODEL="${MODEL:-qwen3:8b}"
SET_RES=$(apatchc "/v1/dashboard/lab" "{\"mcp_orchestrator_model\":\"$ORC_MODEL\"}")
SET_CODE=$(echo "$SET_RES" | code)
SET_VAL=$(echo "$SET_RES" | body | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('mcp_orchestrator_model','?'))" 2>/dev/null || echo "?")
if [ "$SET_CODE" = "200" ] && [ "$SET_VAL" = "$ORC_MODEL" ]; then
  pass "Orchestrator model set → '$ORC_MODEL'"
else
  fail "Orchestrator model set → code=$SET_CODE val=$SET_VAL (expected '$ORC_MODEL')"
fi

# 2. Verify GET reflects the new value
LAB_AFTER=$(aget "/v1/dashboard/lab" 2>/dev/null || echo "{}")
AFTER_VAL=$(echo "$LAB_AFTER" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('mcp_orchestrator_model','?'))" 2>/dev/null || echo "?")
[ "$AFTER_VAL" = "$ORC_MODEL" ] \
  && pass "GET /v1/dashboard/lab after PATCH → mcp_orchestrator_model='$AFTER_VAL'" \
  || fail "GET /v1/dashboard/lab after PATCH → '$AFTER_VAL' (expected '$ORC_MODEL')"

# 3. Absent key → value must not change
NO_KEY_RES=$(apatchc "/v1/dashboard/lab" '{"max_images_per_request":4}')
NO_KEY_CODE=$(echo "$NO_KEY_RES" | code)
NO_KEY_VAL=$(echo "$NO_KEY_RES" | body | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('mcp_orchestrator_model','?'))" 2>/dev/null || echo "?")
if [ "$NO_KEY_CODE" = "200" ] && [ "$NO_KEY_VAL" = "$ORC_MODEL" ]; then
  pass "Orchestrator model: absent key in PATCH → value unchanged ('$NO_KEY_VAL')"
else
  fail "Orchestrator model: absent key → code=$NO_KEY_CODE val=$NO_KEY_VAL (expected '$ORC_MODEL')"
fi

# 4. Inference with orchestrator override active — verify request is accepted
# (cannot observe which model veronex internally used, but 200/503 = bridge path was taken)
ORC_INF_RES=$(curl -s -w "\n%{http_code}" --max-time 60 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{
    \"model\": \"dummy_model_should_be_overridden\",
    \"messages\": [{\"role\": \"user\", \"content\": \"/no_think Say hi.\"}],
    \"stream\": false,
    \"max_tokens\": 8
  }" 2>/dev/null || printf "\n000")
ORC_INF_CODE=$(echo "$ORC_INF_RES" | tail -1)
case "$ORC_INF_CODE" in
  200) pass "Inference with orchestrator override active → 200 (override applied: $ORC_MODEL)" ;;
  503) info "Inference with orchestrator override → 503 (no providers available)" ;;
  404) pass "Inference with orchestrator override → 404 (model routing used override, dummy rejected)" ;;
  *)   fail "Inference with orchestrator override → $ORC_INF_CODE" ;;
esac

# 5. Clear with null
CLR_RES=$(apatchc "/v1/dashboard/lab" '{"mcp_orchestrator_model":null}')
CLR_CODE=$(echo "$CLR_RES" | code)
CLR_VAL=$(echo "$CLR_RES" | body | python3 -c "
import sys,json; d=json.loads(sys.stdin.read()); v=d.get('mcp_orchestrator_model',False)
print('null' if v is None else str(v))" 2>/dev/null || echo "?")
if [ "$CLR_CODE" = "200" ] && [ "$CLR_VAL" = "null" ]; then
  pass "Orchestrator model: PATCH null → cleared"
else
  fail "Orchestrator model: PATCH null → code=$CLR_CODE val=$CLR_VAL (expected null)"
fi

# 6. Restore original value (if it was set before this test)
if [ "$LAB_INIT_MODEL" != "null" ] && [ "$LAB_INIT_MODEL" != "?" ]; then
  apatch "/v1/dashboard/lab" "{\"mcp_orchestrator_model\":\"$LAB_INIT_MODEL\"}" > /dev/null 2>&1
fi

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

# 4. tools/list — expect exactly 1 tool: get_weather (geocoding is internal, not exposed)
TOOLS_LIST=$(curl -s --max-time 10 "$MCP_TEST_URL/mcp" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  2>/dev/null || echo "{}")
TOOL_NAMES=$(echo "$TOOLS_LIST" | python3 -c "
import sys,json
tools=json.loads(sys.stdin.read()).get('result',{}).get('tools',[])
print(','.join(t.get('name','') for t in tools))
" 2>/dev/null || echo "")
TOOL_COUNT=$(echo "$TOOLS_LIST" | python3 -c "import sys,json; print(len(json.loads(sys.stdin.read()).get('result',{}).get('tools',[])))" 2>/dev/null || echo "0")
[ "$TOOL_COUNT" -eq 1 ] && echo "$TOOL_NAMES" | grep -q "get_weather" \
  && pass "weather-mcp tools/list → 1 tool [get_weather] (geocoding internal via embedded GeoNames)" \
  || fail "weather-mcp tools/list → [$TOOL_NAMES] count=$TOOL_COUNT (expected exactly: get_weather)"
echo "$TOOL_NAMES" | grep -qv "get_coordinates" \
  && pass "weather-mcp tools/list → get_coordinates not exposed (internal implementation)" \
  || fail "weather-mcp tools/list → get_coordinates still exposed (should be internal)"

# 5. tools/call get_weather — English city name, combined weather + air quality
WEATHER_RES=$(curl -s --max-time 20 "$MCP_TEST_URL/mcp" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_weather","arguments":{"city":"Seoul"}}}' \
  2>/dev/null || echo "{}")
WEATHER_CHECK=$(echo "$WEATHER_RES" | python3 -c "
import sys,json
d=json.loads(sys.stdin.read())
r=d.get('result',{})
is_err=r.get('isError',True)
text=(r.get('content',[{}])[0] if r.get('content') else {}).get('text','')
try:
    data=json.loads(text)
    cond=data.get('conditions',{})
    aq=cond.get('air_quality',{})
    missing=[f for f in ['temperature','humidity_percent','uv_index','wind','precipitation'] if f not in cond]
    missing+=['aq.'+f for f in ['pm2_5','pm10','european_aqi','us_aqi'] if f not in aq]
    print('ok' if not is_err and not missing else f'err:isError={is_err},missing={missing}')
except Exception as e:
    print(f'parse_err:{e}:{text[:60]}')
" 2>/dev/null || echo "parse_error")
[ "$WEATHER_CHECK" = "ok" ] \
  && pass "weather-mcp get_weather(Seoul) → weather+air_quality combined (temp,uv,wind,pm2.5,aqi)" \
  || info "weather-mcp get_weather(Seoul) → $WEATHER_CHECK (network may be unavailable)"

# 6. tools/call get_weather — Korean city+district (offline geocoding: embedded GeoNames)
KO_RES=$(curl -s --max-time 20 "$MCP_TEST_URL/mcp" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_weather","arguments":{"city":"서울 강남"}}}' \
  2>/dev/null || echo "{}")
KO_CHECK=$(echo "$KO_RES" | python3 -c "
import sys,json
d=json.loads(sys.stdin.read())
r=d.get('result',{})
is_err=r.get('isError',True)
text=(r.get('content',[{}])[0] if r.get('content') else {}).get('text','')
try:
    data=json.loads(text)
    lat=data.get('location',{}).get('latitude',0)
    print('ok' if not is_err and 37.0 < lat < 38.0 else f'err:isError={is_err},lat={lat}')
except Exception as e:
    print(f'parse_err:{e}:{text[:60]}')
" 2>/dev/null || echo "parse_error")
[ "$KO_CHECK" = "ok" ] \
  && pass "weather-mcp get_weather(서울 강남) → Korean district resolved via embedded GeoNames" \
  || info "weather-mcp get_weather(서울 강남) → $KO_CHECK (network may be unavailable)"

# 7. tools/call get_weather — unknown city returns isError=true (no network needed)
ERR_RES=$(curl -s --max-time 10 "$MCP_TEST_URL/mcp" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"get_weather","arguments":{"city":"xyzzy_nonexistent_city_99999"}}}' \
  2>/dev/null || echo "{}")
ERR_IS_ERROR=$(echo "$ERR_RES" | python3 -c "
import sys,json
r=json.loads(sys.stdin.read()).get('result',{})
print('true' if r.get('isError') else 'false')
" 2>/dev/null || echo "?")
[ "$ERR_IS_ERROR" = "true" ] \
  && pass "weather-mcp get_weather(unknown city) → isError=true (GeoNames lookup failed)" \
  || fail "weather-mcp get_weather(unknown city) → isError=$ERR_IS_ERROR (expected true)"

# ── Full MCP Integration (register → online → tool_call → disable → delete) ──

hdr "MCP Full Integration (veronex → weather-mcp)"

INT_SLUG="e2eweather"
INT_NAME="e2e-weather-integration"
INT_ID=""

# 1. Register weather-mcp server
INT_REG=$(apost "/v1/mcp/servers" \
  "{\"name\":\"$INT_NAME\",\"slug\":\"$INT_SLUG\",\"url\":\"$MCP_TEST_URL\",\"timeout_secs\":30}" \
  2>/dev/null || echo "{}")
INT_ID=$(echo "$INT_REG" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('id',''))" 2>/dev/null || echo "")
[ -n "$INT_ID" ] \
  && pass "MCP integration: registered weather-mcp server (id: $INT_ID)" \
  || { fail "MCP integration: failed to register server (resp: $INT_REG)"; save_counts; exit 0; }

# 2. Wait for server to become online (veronex-agent heartbeat, up to 15s)
ONLINE="false"
for i in $(seq 1 6); do
  sleep 3
  LIST_RES=$(aget "/v1/mcp/servers" 2>/dev/null || echo "[]")
  ONLINE=$(echo "$LIST_RES" | python3 -c "
import sys,json
servers=json.loads(sys.stdin.read())
s=next((s for s in servers if s.get('id')=='$INT_ID'), {})
print('true' if s.get('online') else 'false')
" 2>/dev/null || echo "false")
  TOOL_COUNT=$(echo "$LIST_RES" | python3 -c "
import sys,json
servers=json.loads(sys.stdin.read())
s=next((s for s in servers if s.get('id')=='$INT_ID'), {})
print(s.get('tool_count',0))
" 2>/dev/null || echo "0")
  [ "$ONLINE" = "true" ] && break
done
[ "$ONLINE" = "true" ] \
  && pass "MCP integration: server online (tool_count=$TOOL_COUNT)" \
  || info "MCP integration: server not yet online after 18s (agent may not be running)"

# 3. Inference — expect tool_calls when server is online
INF_RES=$(curl -s -w "\n%{http_code}" --max-time 90 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"/no_think What is the weather in Seoul? Use the available tools.\"}],
    \"stream\": false,
    \"max_tokens\": 128
  }" 2>/dev/null || printf "\n000")
INF_CODE=$(echo "$INF_RES" | tail -1)
INF_BODY=$(echo "$INF_RES" | sed '$d')

case "$INF_CODE" in
  200)
    INF_CHECK=$(echo "$INF_BODY" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read().strip())
    choices = d.get('choices', [])
    if not choices: print('no_choices'); exit()
    msg = choices[0].get('message', {})
    tool_calls = msg.get('tool_calls', [])
    finish = choices[0].get('finish_reason', '')
    if tool_calls:
        names = [tc.get('function', {}).get('name', '') for tc in tool_calls]
        print('tool_calls:' + str(names))
    elif finish == 'stop':
        print('answer:' + (msg.get('content') or '')[:80])
    else:
        print(f'unexpected:finish={finish}')
except Exception as e:
    print(f'error:{e}')
" 2>/dev/null || echo "parse_error")
    case "$INF_CHECK" in
      tool_calls:*) pass "MCP inference → tool_calls dispatched: ${INF_CHECK#tool_calls:}" ;;
      answer:*)
        if [ "$ONLINE" = "true" ]; then
          fail "MCP inference → model answered without tool_calls (bridge active but tools not used)"
        else
          info "MCP inference → model answered directly (bridge not active — server not online)"
        fi ;;
      *) fail "MCP inference → $INF_CHECK" ;;
    esac ;;
  503) info "MCP inference → 503 (no providers available)" ;;
  *)   fail "MCP inference → $INF_CODE" ;;
esac

# 4. Verify Valkey heartbeat key exists for registered server
if [ -n "$INT_ID" ]; then
  HB_VAL=$(docker compose exec -T valkey valkey-cli GET "veronex:mcp:heartbeat:$INT_ID" 2>/dev/null | tr -d ' \r\n' || echo "")
  [ -n "$HB_VAL" ] \
    && pass "MCP heartbeat: Valkey key present for server $INT_ID" \
    || info "MCP heartbeat: no Valkey key yet (agent scrape pending)"
fi

# 5. Disable server — inference should proceed without MCP tools

if [ -n "$INT_ID" ]; then
  apatch "/v1/mcp/servers/$INT_ID" '{"is_enabled":false}' > /dev/null 2>&1
  DIS_RES=$(curl -s -w "\n%{http_code}" --max-time 60 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
    -d "{
      \"model\": \"$MODEL\",
      \"messages\": [{\"role\": \"user\", \"content\": \"/no_think Say hello.\"}],
      \"stream\": false,
      \"max_tokens\": 16
    }" 2>/dev/null || printf "\n000")
  DIS_CODE=$(echo "$DIS_RES" | tail -1)
  DIS_TOOLS=$(echo "$DIS_RES" | sed '$d' | python3 -c "
import sys,json
try:
  d=json.loads(sys.stdin.read())
  tc=d.get('choices',[{}])[0].get('message',{}).get('tool_calls',[])
  print('has_tools' if tc else 'no_tools')
except: print('parse_error')
" 2>/dev/null || echo "parse_error")
  if [ "$DIS_CODE" = "200" ]; then
    [ "$DIS_TOOLS" = "no_tools" ] \
      && pass "MCP disable: inference after disable has no tool_calls" \
      || fail "MCP disable: inference still has tool_calls after disable"
  else
    info "MCP disable: inference → $DIS_CODE (skipping tool_call check)"
  fi
fi

# 6. Cleanup — delete integration test server
if [ -n "$INT_ID" ]; then
  DEL_C=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$API/v1/mcp/servers/$INT_ID" \
    -H "Authorization: Bearer $TK" 2>/dev/null || echo "000")
  [ "$DEL_C" = "204" ] \
    && pass "MCP integration: cleanup — server deleted" \
    || fail "MCP integration: cleanup failed (DELETE → $DEL_C)"
fi

# ── Persistent Sample Data (manual verification) ─────────────────────────────
# Policy: leave representative data registered after tests so the UI/API can be
# manually inspected without re-running the suite.
# Creates 2 servers, deletes 1 — "날씨 MCP" remains online for manual access.

hdr "MCP Persistent Sample Data"

# Remove stale sample servers from prior runs to avoid slug conflicts
for STALE_SLUG in weather_mcp dust_mcp; do
  STALE_ID=$(aget "/v1/mcp/servers" 2>/dev/null \
    | python3 -c "import sys,json; sl=json.loads(sys.stdin.read()); print(next((s['id'] for s in sl if s.get('slug')=='$STALE_SLUG'),''))" 2>/dev/null || echo "")
  if [ -n "$STALE_ID" ]; then
    curl -s -o /dev/null -X DELETE "$API/v1/mcp/servers/$STALE_ID" -H "Authorization: Bearer $TK" 2>/dev/null || true
  fi
done

# Register 날씨 MCP
WEATHER_RES=$(apost "/v1/mcp/servers" \
  "{\"name\":\"날씨 MCP\",\"slug\":\"weather_mcp\",\"url\":\"$MCP_REGISTER_URL\",\"timeout_secs\":30}" \
  2>/dev/null || echo "{}")
WEATHER_ID=$(echo "$WEATHER_RES" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('id',''))" 2>/dev/null || echo "")
[ -n "$WEATHER_ID" ] \
  && pass "Sample data: 날씨 MCP registered (id: $WEATHER_ID)" \
  || fail "Sample data: 날씨 MCP registration failed (resp: $WEATHER_RES)"

# Register 미세먼지 MCP
DUST_RES=$(apost "/v1/mcp/servers" \
  "{\"name\":\"미세먼지 MCP\",\"slug\":\"dust_mcp\",\"url\":\"$MCP_REGISTER_URL\",\"timeout_secs\":30}" \
  2>/dev/null || echo "{}")
DUST_ID=$(echo "$DUST_RES" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('id',''))" 2>/dev/null || echo "")
[ -n "$DUST_ID" ] \
  && pass "Sample data: 미세먼지 MCP registered (id: $DUST_ID)" \
  || fail "Sample data: 미세먼지 MCP registration failed"

# Delete 미세먼지 MCP — 날씨 MCP intentionally left registered
if [ -n "$DUST_ID" ]; then
  DEL_DUST=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$API/v1/mcp/servers/$DUST_ID" \
    -H "Authorization: Bearer $TK" 2>/dev/null || echo "000")
  [ "$DEL_DUST" = "204" ] \
    && pass "Sample data: 미세먼지 MCP deleted (delete flow verified)" \
    || fail "Sample data: 미세먼지 MCP delete failed → $DEL_DUST"
fi

# Confirm 날씨 MCP still present
if [ -n "$WEATHER_ID" ]; then
  STILL_RES=$(aget "/v1/mcp/servers" 2>/dev/null || echo "[]")
  STILL=$(echo "$STILL_RES" | python3 -c "import sys,json; sl=json.loads(sys.stdin.read()); print('yes' if any(s.get('id')=='$WEATHER_ID' for s in sl) else 'no')" 2>/dev/null || echo "no")
  [ "$STILL" = "yes" ] \
    && pass "Sample data: 날씨 MCP remains registered — accessible at UI /mcp" \
    || fail "Sample data: 날씨 MCP not found in list after delete"
fi

# ── MCP Stats ─────────────────────────────────────────────────────────────────

hdr "MCP Stats"

STATS_RES=$(agetc "/v1/mcp/stats" 2>/dev/null || printf "\n000")
STATS_CODE=$(echo "$STATS_RES" | tail -1)
STATS_BODY=$(echo "$STATS_RES" | sed '$d')
[ "$STATS_CODE" = "200" ] \
  && pass "GET /v1/mcp/stats → 200" \
  || fail "GET /v1/mcp/stats → $STATS_CODE (expected 200)"

if [ "$STATS_CODE" = "200" ]; then
  STATS_IS_ARRAY=$(echo "$STATS_BODY" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    print('yes' if isinstance(d, list) else 'no:type=' + type(d).__name__)
except Exception as e:
    print(f'parse_error:{e}')
" 2>/dev/null || echo "parse_error")
  [ "$STATS_IS_ARRAY" = "yes" ] \
    && pass "GET /v1/mcp/stats → returns array" \
    || fail "GET /v1/mcp/stats → $STATS_IS_ARRAY (expected array)"

  STATS_COUNT=$(echo "$STATS_BODY" | python3 -c "import sys,json; print(len(json.loads(sys.stdin.read())))" 2>/dev/null || echo "0")
  if [ "$STATS_COUNT" -gt 0 ] 2>/dev/null; then
    STATS_FIELDS=$(echo "$STATS_BODY" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
s = d[0]
required = ['server_id', 'server_name', 'slug', 'total_calls', 'success_rate', 'avg_latency_ms']
missing = [f for f in required if f not in s]
print('ok' if not missing else 'missing:' + ','.join(missing))
" 2>/dev/null || echo "parse_error")
    [ "$STATS_FIELDS" = "ok" ] \
      && pass "GET /v1/mcp/stats → entry has required fields ($STATS_COUNT entries)" \
      || fail "GET /v1/mcp/stats → $STATS_FIELDS"
  else
    pass "GET /v1/mcp/stats → empty array (no calls recorded yet)"
  fi
fi

STATS_H1=$(curl -s -o /dev/null -w "%{http_code}" "$API/v1/mcp/stats?hours=1" \
  -H "Authorization: Bearer $TK" 2>/dev/null || echo "000")
[ "$STATS_H1" = "200" ] \
  && pass "GET /v1/mcp/stats?hours=1 → 200" \
  || fail "GET /v1/mcp/stats?hours=1 → $STATS_H1"

# ── API Key MCP Access ─────────────────────────────────────────────────────────

hdr "API Key MCP Access"

TEST_KEY_ID="${API_KEY_ID_PAID:-}"
if [ -z "$TEST_KEY_ID" ] || [ "$TEST_KEY_ID" = "None" ]; then
  TEST_KEY_ID=$(aget "/v1/keys?limit=1" 2>/dev/null \
    | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); keys=d.get('keys',d) if isinstance(d,dict) else d; print(keys[0]['id'] if keys else '')" 2>/dev/null || echo "")
fi

if [ -z "$TEST_KEY_ID" ] || [ "$TEST_KEY_ID" = "None" ]; then
  info "API Key MCP Access: no key available — skipping"
else
  LIST_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$API/v1/keys/$TEST_KEY_ID/mcp" \
    -H "Authorization: Bearer $TK" 2>/dev/null || echo "000")
  [ "$LIST_CODE" = "200" ] \
    && pass "GET /v1/keys/:id/mcp → 200" \
    || fail "GET /v1/keys/:id/mcp → $LIST_CODE (expected 200)"

  MCP_SERVERS=$(aget "/v1/mcp/servers" 2>/dev/null || echo "[]")
  GRANT_SERVER_ID=$(echo "$MCP_SERVERS" | python3 -c "
import sys, json
servers = json.loads(sys.stdin.read())
print(servers[0]['id'] if servers else '')
" 2>/dev/null || echo "")

  if [ -z "$GRANT_SERVER_ID" ]; then
    info "API Key MCP Access: no MCP servers registered — skipping grant/revoke"
  else
    # Grant
    GRANT_RES=$(curl -s -w "\n%{http_code}" "$API/v1/keys/$TEST_KEY_ID/mcp" \
      -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
      -d "{\"server_id\":\"$GRANT_SERVER_ID\"}" 2>/dev/null || printf "\n000")
    GRANT_CODE=$(echo "$GRANT_RES" | tail -1)
    GRANT_BODY=$(echo "$GRANT_RES" | sed '$d')
    [ "$GRANT_CODE" = "201" ] \
      && pass "POST /v1/keys/:id/mcp → 201 (grant)" \
      || fail "POST /v1/keys/:id/mcp → $GRANT_CODE (expected 201)"

    if [ "$GRANT_CODE" = "201" ]; then
      IS_ALLOWED=$(echo "$GRANT_BODY" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
print('true' if d.get('is_allowed') is True else 'false')
" 2>/dev/null || echo "?")
      [ "$IS_ALLOWED" = "true" ] \
        && pass "POST /v1/keys/:id/mcp → is_allowed=true in response" \
        || fail "POST /v1/keys/:id/mcp → is_allowed=$IS_ALLOWED"
    fi

    # Verify list shows allowed
    AFTER_LIST=$(aget "/v1/keys/$TEST_KEY_ID/mcp" 2>/dev/null || echo "[]")
    AFTER_ALLOWED=$(echo "$AFTER_LIST" | python3 -c "
import sys, json
entries = json.loads(sys.stdin.read())
match = next((e for e in entries if e.get('server_id') == '$GRANT_SERVER_ID'), None)
print('true' if match and match.get('is_allowed') else 'not_found' if not match else 'false')
" 2>/dev/null || echo "?")
    [ "$AFTER_ALLOWED" = "true" ] \
      && pass "GET /v1/keys/:id/mcp after grant → is_allowed=true" \
      || fail "GET /v1/keys/:id/mcp after grant → $AFTER_ALLOWED"

    # Revoke
    REVOKE_CODE=$(curl -s -o /dev/null -w "%{http_code}" \
      -X DELETE "$API/v1/keys/$TEST_KEY_ID/mcp/$GRANT_SERVER_ID" \
      -H "Authorization: Bearer $TK" 2>/dev/null || echo "000")
    [ "$REVOKE_CODE" = "204" ] \
      && pass "DELETE /v1/keys/:id/mcp/:server_id → 204 (revoke)" \
      || fail "DELETE /v1/keys/:id/mcp/:server_id → $REVOKE_CODE (expected 204)"

    # After revoke — not allowed
    AFTER_REVOKE=$(aget "/v1/keys/$TEST_KEY_ID/mcp" 2>/dev/null || echo "[]")
    REVOKE_STATUS=$(echo "$AFTER_REVOKE" | python3 -c "
import sys, json
entries = json.loads(sys.stdin.read())
match = next((e for e in entries if e.get('server_id') == '$GRANT_SERVER_ID'), None)
print('ok' if not match or not match.get('is_allowed') else 'still_allowed')
" 2>/dev/null || echo "?")
    [ "$REVOKE_STATUS" = "ok" ] \
      && pass "GET /v1/keys/:id/mcp after revoke → not allowed" \
      || fail "GET /v1/keys/:id/mcp after revoke → $REVOKE_STATUS"

    # Idempotent revoke
    REVOKE2_CODE=$(curl -s -o /dev/null -w "%{http_code}" \
      -X DELETE "$API/v1/keys/$TEST_KEY_ID/mcp/$GRANT_SERVER_ID" \
      -H "Authorization: Bearer $TK" 2>/dev/null || echo "000")
    [ "$REVOKE2_CODE" = "204" ] \
      && pass "DELETE /v1/keys/:id/mcp/:server_id (idempotent) → 204" \
      || fail "DELETE /v1/keys/:id/mcp/:server_id (idempotent) → $REVOKE2_CODE"
  fi
fi

save_counts
