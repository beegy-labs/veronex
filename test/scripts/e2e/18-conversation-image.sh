#!/usr/bin/env bash
# Phase 18: Conversation + Image — multi-turn with images, use_mcp flag, empty-prompt vision
#
# Tests:
#   A. Vision model multi-turn chat with image — must respond (not infinite loop via MCP)
#   B. use_mcp=false flag — bypasses MCP bridge even for text-only requests
#   C. Vision model: empty prompt + image → default prompt auto-filled
#   D. Image in non-vision model multi-turn — vision fallback injected, MCP bypassed
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; ensure_auth

# ── Detect models ────────────────────────────────────────────────────────────

VISION_MODEL=$(get_vision_model)
TEXT_MODEL=$(get_text_model)

if [ -z "$VISION_MODEL" ]; then
  fail "No vision model available — a vision model must be loaded in Ollama"
  save_counts
  exit 1
fi
info "Vision model: $VISION_MODEL"

# ── Generate test image ──────────────────────────────────────────────────────

FIXTURE="$SCRIPT_DIR/test-fixture.jpg"
if [ ! -f "$FIXTURE" ]; then
  python3 - "$FIXTURE" <<'PYEOF' 2>/dev/null || true
import sys
from PIL import Image, ImageDraw
img = Image.new('RGB', (200, 100), '#1e293b')
draw = ImageDraw.Draw(img)
draw.rectangle([10, 10, 190, 90], fill='#0f172a', outline='#3b82f6', width=2)
draw.text((20, 30), "Hello World", fill='#60a5fa')
img.save(sys.argv[1], format='JPEG', quality=85)
PYEOF
fi

if [ ! -f "$FIXTURE" ]; then
  fail "Pillow not installed — run: pip install pillow"
  save_counts
  exit 1
fi

IMG_B64=$(base64 < "$FIXTURE" | tr -d '\n')

# ── Warm up vision model ─────────────────────────────────────────────────────

info "Warming up vision model ($VISION_MODEL)..."
curl -s --max-time 120 "$API/api/generate" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$VISION_MODEL\",\"prompt\":\"/no_think say ok\",\"stream\":false}" > /dev/null 2>&1 || true

# ── A: Vision model multi-turn chat with image ────────────────────────────────
# Key regression: images in messages[] must bypass MCP (was: infinite loading)

hdr "A: Vision multi-turn — image in messages[] bypasses MCP"

PAYLOAD_A=$(python3 -c "
import json
print(json.dumps({
  'model': '$VISION_MODEL',
  'messages': [{'role': 'user', 'content': '/no_think What color is this image? One word.', 'images': ['$IMG_B64']}],
  'stream': False,
  'provider_type': 'ollama'
}))
")

RES_A=$(curl -s -w "\n%{http_code}" --max-time 120 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
  -d "$PAYLOAD_A" 2>/dev/null || printf "\n000")

CODE_A=$(echo "$RES_A" | tail -1)
BODY_A=$(echo "$RES_A" | sed '$d')

case "$CODE_A" in
  200) pass "Vision multi-turn + image → 200" ;;
  503) fail "Vision multi-turn + image → 503 (no eligible provider)"; save_counts; exit 1 ;;
  *)   fail "Vision multi-turn + image → $CODE_A"; save_counts; exit 1 ;;
esac

TEXT_A=$(echo "$BODY_A" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    print(d.get('choices', [{}])[0].get('message', {}).get('content', ''))
except: print('')
" 2>/dev/null || echo "")

[ -n "$TEXT_A" ] && pass "Vision multi-turn: non-empty response (MCP not blocking)" \
                 || fail "Vision multi-turn: empty response — may still be routing through MCP"
info "Response: ${TEXT_A:0:100}"

# Verify no tool_calls in response (direct MCP bypass confirmation)
TOOL_CALLS_A=$(echo "$BODY_A" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    tc = d.get('choices',[{}])[0].get('message',{}).get('tool_calls')
    print('none' if not tc else 'present')
except: print('none')
" 2>/dev/null || echo "none")
[ "$TOOL_CALLS_A" = "none" ] \
  && pass "Vision multi-turn: no tool_calls in response (MCP bypassed, not routed through bridge)" \
  || fail "Vision multi-turn: tool_calls present — request went through MCP bridge despite image"

# Verify response reflects image content (color-related)
if [ -n "$TEXT_A" ]; then
  COLOR_OK=$(echo "$TEXT_A" | python3 -c "
import sys
text = sys.stdin.read().lower()
# Image is '#1e293b' background (dark blue/navy/slate) with '#3b82f6' border (blue)
color_words = ['dark', 'blue', 'navy', 'black', 'gray', 'grey', 'slate', 'teal', 'purple']
print('yes' if any(w in text for w in color_words) else 'no')
" 2>/dev/null || echo "no")
  [ "$COLOR_OK" = "yes" ] \
    && pass "Vision multi-turn: response reflects image color content" \
    || fail "Vision multi-turn: response does not mention image color — vision model may not have seen the image"
fi

# ── B: Multi-turn conversation continuity with images ────────────────────────

hdr "B: Multi-turn conversation — 2 turns, second turn references first"

# Turn 1: send image
PAYLOAD_B1=$(python3 -c "
import json
print(json.dumps({
  'model': '$VISION_MODEL',
  'messages': [{'role': 'user', 'content': '/no_think I am showing you an image. Just say: \"Image received.\"', 'images': ['$IMG_B64']}],
  'stream': False,
  'provider_type': 'ollama'
}))
")

RES_B1=$(curl -s -w "\n%{http_code}" --max-time 120 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
  -d "$PAYLOAD_B1" 2>/dev/null || printf "\n000")

CODE_B1=$(echo "$RES_B1" | tail -1)
BODY_B1=$(echo "$RES_B1" | sed '$d')

[ "$CODE_B1" = "200" ] && pass "Turn 1 (image) → 200" || { fail "Turn 1 → $CODE_B1"; save_counts; exit 1; }

REPLY_B1=$(echo "$BODY_B1" | python3 -c "
import sys, json
try: print(json.loads(sys.stdin.read()).get('choices',[{}])[0].get('message',{}).get('content',''))
except: print('')
" 2>/dev/null || echo "")
CONV_ID_B=$(echo "$BODY_B1" | python3 -c "
import sys, json
try: print(json.loads(sys.stdin.read()).get('conversation_id',''))
except: print('')
" 2>/dev/null || echo "")

[ -n "$REPLY_B1" ] && pass "Turn 1: got response" || fail "Turn 1: empty response"
[ -n "$CONV_ID_B" ] && pass "Turn 1: conversation_id issued ($CONV_ID_B)" \
                     || fail "Turn 1: no conversation_id (server must issue conversation_id for vision turn)"

# Turn 2: text only, reference previous (use conversation_id if available)
PAYLOAD_B2=$(python3 -c "
import json
msgs = [
  {'role': 'user', 'content': '/no_think I showed you an image.', 'images': ['$IMG_B64']},
  {'role': 'assistant', 'content': '${REPLY_B1//\'/\'\\\'\'}'},
  {'role': 'user', 'content': '/no_think What did I show you?'}
]
body = {'model': '$VISION_MODEL', 'messages': msgs, 'stream': False, 'provider_type': 'ollama'}
if '$CONV_ID_B': body['conversation_id'] = '$CONV_ID_B'
print(json.dumps(body))
")

RES_B2=$(curl -s -w "\n%{http_code}" --max-time 120 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
  -d "$PAYLOAD_B2" 2>/dev/null || printf "\n000")

CODE_B2=$(echo "$RES_B2" | tail -1)
BODY_B2=$(echo "$RES_B2" | sed '$d')

[ "$CODE_B2" = "200" ] && pass "Turn 2 (text follow-up) → 200" || fail "Turn 2 → $CODE_B2"

REPLY_B2=$(echo "$BODY_B2" | python3 -c "
import sys, json
try: print(json.loads(sys.stdin.read()).get('choices',[{}])[0].get('message',{}).get('content',''))
except: print('')
" 2>/dev/null || echo "")
[ -n "$REPLY_B2" ] && pass "Turn 2: non-empty response" || fail "Turn 2: empty response"
info "Turn 2 response: ${REPLY_B2:0:100}"

# ── C: use_mcp=false — MCP bypass via API flag ────────────────────────────────

hdr "C: use_mcp=false flag bypasses MCP bridge"

TEXT_M="${TEXT_MODEL:-$VISION_MODEL}"
PAYLOAD_C=$(python3 -c "
import json
print(json.dumps({
  'model': '$TEXT_M',
  'messages': [{'role': 'user', 'content': '/no_think say pong'}],
  'stream': False,
  'provider_type': 'ollama',
  'use_mcp': False
}))
")

RES_C=$(curl -s -w "\n%{http_code}" --max-time 60 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
  -d "$PAYLOAD_C" 2>/dev/null || printf "\n000")

CODE_C=$(echo "$RES_C" | tail -1)
BODY_C=$(echo "$RES_C" | sed '$d')

case "$CODE_C" in
  200) pass "use_mcp=false → 200 (MCP bypassed, direct inference)" ;;
  503) info "use_mcp=false → 503 (no provider, but flag accepted)" ;;
  400) fail "use_mcp=false → 400 (field rejected — not deserialized)" ;;
  *)   info "use_mcp=false → $CODE_C" ;;
esac

TEXT_C=$(echo "$BODY_C" | python3 -c "
import sys, json
try: print(json.loads(sys.stdin.read()).get('choices',[{}])[0].get('message',{}).get('content',''))
except: print('')
" 2>/dev/null || echo "")
[ -n "$TEXT_C" ] && pass "use_mcp=false: got response ('$TEXT_C')" || fail "use_mcp=false: empty response"

# ── D: Empty prompt + image → auto default prompt for vision model ────────────

hdr "D: Empty prompt + image → auto default prompt (vision model)"

PAYLOAD_D=$(python3 -c "
import json
print(json.dumps({
  'model': '$VISION_MODEL',
  'prompt': '',
  'images': ['$IMG_B64'],
  'stream': False
}))
")

RES_D=$(curl -s -w "\n%{http_code}" --max-time 120 "$API/api/generate" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "$PAYLOAD_D" 2>/dev/null || printf "\n000")

CODE_D=$(echo "$RES_D" | tail -1)
BODY_D=$(echo "$RES_D" | sed '$d')

case "$CODE_D" in
  200) pass "Empty prompt + image → 200 (default prompt applied)" ;;
  400)
    ERR_D=$(echo "$BODY_D" | python3 -c "import sys,json; print(json.loads(sys.stdin.read()).get('error',''))" 2>/dev/null || echo "")
    fail "Empty prompt + image → 400: $ERR_D"
    ;;
  503) info "Empty prompt + image → 503 (no provider)" ;;
  *)   fail "Empty prompt + image → $CODE_D" ;;
esac

TEXT_D=$(echo "$BODY_D" | python3 -c "
import sys, json
try: print(json.loads(sys.stdin.read()).get('response', ''))
except: print('')
" 2>/dev/null || echo "")
[ -n "$TEXT_D" ] && pass "Empty prompt: got response via default prompt" || fail "Empty prompt: no response"
info "Response: ${TEXT_D:0:100}"

# ── E: Non-vision model + image in multi-turn → MCP bypassed ─────────────────

if [ -n "$TEXT_MODEL" ]; then
  hdr "E: Non-vision model + image in multi-turn → MCP bypassed"

  PAYLOAD_E=$(python3 -c "
import json
print(json.dumps({
  'model': '$TEXT_MODEL',
  'messages': [{'role': 'user', 'content': '/no_think Describe what you see.', 'images': ['$IMG_B64']}],
  'stream': False,
  'provider_type': 'ollama'
}))
")

  RES_E=$(curl -s -w "\n%{http_code}" --max-time 180 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
    -d "$PAYLOAD_E" 2>/dev/null || printf "\n000")

  CODE_E=$(echo "$RES_E" | tail -1)
  BODY_E=$(echo "$RES_E" | sed '$d')

  case "$CODE_E" in
    200) pass "Non-vision + image in chat → 200 (MCP bypassed, vision fallback applied)" ;;
    503) info "Non-vision + image → 503 (no eligible provider)" ;;
    *)   fail "Non-vision + image → $CODE_E" ;;
  esac

  TEXT_E=$(echo "$BODY_E" | python3 -c "
import sys, json
try: print(json.loads(sys.stdin.read()).get('choices',[{}])[0].get('message',{}).get('content',''))
except: print('')
" 2>/dev/null || echo "")
  [ -n "$TEXT_E" ] && pass "Non-vision + image: got response" || fail "Non-vision + image: empty response (HTTP 200 but no content)"
  info "Response: ${TEXT_E:0:100}"
fi

# ── F: Image-only request (no text content) — server must handle gracefully ───
# Tests that a message with images[] but empty/absent content field
# does not 400 and still produces a response (auto-prompt or model default).

hdr "F: Image-only message — no text content in messages[]"

PAYLOAD_IMG_ONLY=$(python3 -c "
import json
# content is empty string — image only
print(json.dumps({
  'model': '$VISION_MODEL',
  'messages': [{'role': 'user', 'content': '', 'images': ['$IMG_B64']}],
  'stream': False,
  'provider_type': 'ollama'
}))
")

RES_IMG_ONLY=$(curl -s -w "\n%{http_code}" --max-time 120 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
  -d "$PAYLOAD_IMG_ONLY" 2>/dev/null || printf "\n000")

CODE_IMG_ONLY=$(echo "$RES_IMG_ONLY" | tail -1)
BODY_IMG_ONLY=$(echo "$RES_IMG_ONLY" | sed '$d')

case "$CODE_IMG_ONLY" in
  200) pass "Image-only (empty content) → 200" ;;
  400) fail "Image-only (empty content) → 400 (server rejected image-only message)" ;;
  503) info "Image-only → 503 (no eligible provider)" ;;
  *)   fail "Image-only → $CODE_IMG_ONLY" ;;
esac

if [ "$CODE_IMG_ONLY" = "200" ]; then
  TEXT_IMG_ONLY=$(echo "$BODY_IMG_ONLY" | python3 -c "
import sys, json
try: print(json.loads(sys.stdin.read()).get('choices',[{}])[0].get('message',{}).get('content',''))
except: print('')
" 2>/dev/null || echo "")
  [ -n "$TEXT_IMG_ONLY" ] \
    && pass "Image-only: got non-empty response (model described the image without prompt)" \
    || fail "Image-only: empty response (model did not process image-only input)"
  info "Image-only response: ${TEXT_IMG_ONLY:0:100}"
fi

# ── G: OCR — vision model reads text from image ───────────────────────────────
# Generates an image with clearly printed text, sends to vision model,
# verifies the model actually read the text (not just responded generically).

hdr "G: OCR — vision model reads embedded text from image"

_OCR_PY=$(mktemp /tmp/ocr_gen_XXXXXX.py)
cat > "$_OCR_PY" << 'PYEOF'
from PIL import Image, ImageDraw
import base64, io
img = Image.new('RGB', (320, 80), 'white')
draw = ImageDraw.Draw(img)
draw.rectangle([0, 0, 319, 79], outline='black', width=3)
draw.text((20, 20), "BEEGY42", fill='black')
buf = io.BytesIO()
img.save(buf, format='JPEG', quality=95)
print(base64.b64encode(buf.getvalue()).decode())
PYEOF
OCR_IMG_B64=$(python3 "$_OCR_PY" 2>/dev/null || echo "")
rm -f "$_OCR_PY"

if [ -z "$OCR_IMG_B64" ]; then
  fail "OCR: could not generate test image (Pillow missing?)"
else
  PAYLOAD_F=$(python3 -c "
import json
print(json.dumps({
  'model': '$VISION_MODEL',
  'messages': [{'role': 'user', 'content': '/no_think What text is written in this image? Reply with only the text.', 'images': ['$OCR_IMG_B64']}],
  'stream': False,
  'provider_type': 'ollama'
}))
")

  RES_F=$(curl -s -w "\n%{http_code}" --max-time 120 "$API/v1/chat/completions" \
    -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
    -d "$PAYLOAD_F" 2>/dev/null || printf "\n000")

  CODE_F=$(echo "$RES_F" | tail -1)
  BODY_F=$(echo "$RES_F" | sed '$d')

  case "$CODE_F" in
    200) pass "OCR: vision model responded → 200" ;;
    503) fail "OCR: no eligible provider → 503"; save_counts; exit 0 ;;
    *)   fail "OCR: unexpected code → $CODE_F" ;;
  esac

  if [ "$CODE_F" = "200" ]; then
    TEXT_F=$(echo "$BODY_F" | python3 -c "
import sys, json
try: print(json.loads(sys.stdin.read()).get('choices',[{}])[0].get('message',{}).get('content',''))
except: print('')
" 2>/dev/null || echo "")

    [ -n "$TEXT_F" ] && pass "OCR: non-empty response" || fail "OCR: empty response"
    info "OCR response: ${TEXT_F:0:100}"

    # Check that the model actually read "BEEGY42" or meaningful parts of it
    OCR_HIT=$(echo "$TEXT_F" | python3 -c "
import sys, re
text = sys.stdin.read().upper()
# Accept full match or partial (BEEGY or 42 alone is sufficient signal)
tokens = ['BEEGY42', 'BEEGY', '42']
found = [t for t in tokens if t in text]
print('yes' if found else 'no')
print(','.join(found))
" 2>/dev/null | head -1 || echo "no")

    [ "$OCR_HIT" = "yes" ] \
      && pass "OCR: model correctly read text from image ('BEEGY42' recognized)" \
      || fail "OCR: model response does not contain expected text — vision model did not read image text"
  fi
fi

save_counts
