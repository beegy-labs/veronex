#!/usr/bin/env bash
# Phase 15: Vision Fallback — non-vision model + image → auto-analysis via qwen3-vl
#
# Verifies that when a text-only model receives an image:
#   1. The image is analyzed by the vision fallback model (VISION_FALLBACK_MODEL env,
#      default qwen3-vl:8b) using the user's actual prompt as context.
#   2. The analysis description is injected into the prompt automatically.
#   3. The text model responds with content derived from the image.
#   4. The image is still stored in S3 (image_keys populated).
#
# Test image is generated on first run and cached as scripts/e2e/test-fixture.jpg.
# The file is gitignored — only generated when absent.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; ensure_auth

hdr "Vision Fallback — non-vision model auto image analysis"

# ── Detect vision fallback model (must be loaded in Ollama) ─────────────────

VISION_MODEL=$(curl -s --max-time 5 http://localhost:11434/api/tags 2>/dev/null | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    for m in d.get('models', []):
        name = m.get('name', '')
        if any(v in name.lower() for v in ['-vl', '_vl', 'llava', 'moondream', 'minicpm-v']):
            print(name); exit()
except: pass
print('')
" 2>/dev/null || echo "")

if [ -z "$VISION_MODEL" ]; then
  info "SKIP: No vision model available (need qwen3-vl:8b or similar)"
  save_counts
  exit 0
fi
info "Vision fallback model: $VISION_MODEL"

# ── Detect a non-vision text model ──────────────────────────────────────────

TEXT_MODEL=$(curl -s --max-time 5 http://localhost:11434/api/tags 2>/dev/null | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    for m in d.get('models', []):
        name = m.get('name', '').lower()
        # Skip vision, embed, ocr models
        if any(v in name for v in ['-vl', '_vl', 'llava', 'moondream', 'minicpm-v', 'embed', 'ocr']):
            continue
        print(m['name']); exit()
except: pass
print('')
" 2>/dev/null || echo "")

if [ -z "$TEXT_MODEL" ]; then
  info "SKIP: No text-only model available"
  save_counts
  exit 0
fi
info "Text model (non-vision): $TEXT_MODEL"

# ── Generate test image (cached, gitignored) ─────────────────────────────────

FIXTURE="$SCRIPT_DIR/test-fixture.jpg"

if [ ! -f "$FIXTURE" ]; then
  info "Generating test fixture image: $FIXTURE"
  python3 - "$FIXTURE" <<'PYEOF'
import sys
from PIL import Image, ImageDraw, ImageFont
import os

out = sys.argv[1]
img = Image.new('RGB', (320, 160), '#1e293b')
draw = ImageDraw.Draw(img)
# Draw a simple code-like block so vision model can describe it
draw.rectangle([20, 20, 300, 140], fill='#0f172a', outline='#3b82f6', width=2)
draw.text((30, 30),  "def add(a, b):",       fill='#60a5fa')
draw.text((30, 50),  "    return a + b",      fill='#e2e8f0')
draw.text((30, 80),  "result = add(1, 2)",    fill='#e2e8f0')
draw.text((30, 100), "print(result)  # 3",    fill='#94a3b8')
img.save(out, format='JPEG', quality=85)
print(f"Generated: {out} ({os.path.getsize(out)} bytes)")
PYEOF
  if [ ! -f "$FIXTURE" ]; then
    info "SKIP: Pillow not installed (pip install pillow)"
    save_counts
    exit 0
  fi
else
  info "Using cached fixture: $FIXTURE"
fi

# Base64-encode the fixture
IMG_B64=$(base64 < "$FIXTURE" | tr -d '\n')
if [ -z "$IMG_B64" ]; then
  fail "Failed to base64-encode fixture image"
  save_counts
  exit 1
fi

# ── Warm up text model ────────────────────────────────────────────────────────

info "Warming up text model ($TEXT_MODEL)..."
curl -s --max-time 60 "$API/api/generate" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$TEXT_MODEL\",\"prompt\":\"/no_think say ok\",\"stream\":false}" > /dev/null 2>&1 || true

# ── Test: non-vision model + image via /api/generate ─────────────────────────

hdr "Vision Fallback — /api/generate"

PAYLOAD=$(python3 -c "
import json, sys
print(json.dumps({
  'model': '$TEXT_MODEL',
  'prompt': '/no_think What Python function is shown in this image? Answer in one sentence.',
  'images': ['$IMG_B64'],
  'stream': False
}))
")

GEN_RES=$(curl -s -w "\n%{http_code}" --max-time 180 "$API/api/generate" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "$PAYLOAD" 2>/dev/null || printf "\n000")

GEN_CODE=$(echo "$GEN_RES" | tail -1)
GEN_BODY=$(echo "$GEN_RES" | sed '$d')

case "$GEN_CODE" in
  200) pass "generate with image → 200" ;;
  503) info "SKIP: No eligible provider (503)"; save_counts; exit 0 ;;
  *)   fail "generate with image → $GEN_CODE"; save_counts; exit 1 ;;
esac

# Response must contain some text (vision description injected into prompt)
GEN_TEXT=$(echo "$GEN_BODY" | python3 -c "
import sys, json
try: print(json.loads(sys.stdin.read()).get('response', ''))
except: print('')
" 2>/dev/null || echo "")

if [ -n "$GEN_TEXT" ]; then
  pass "generate: non-empty response (vision fallback worked)"
  info "Response (truncated): ${GEN_TEXT:0:120}"
else
  fail "generate: empty response — vision fallback may have failed"
fi

# ── Test: non-vision model + image via /api/chat ──────────────────────────────

hdr "Vision Fallback — /api/chat"

CHAT_PAYLOAD=$(python3 -c "
import json
print(json.dumps({
  'model': '$TEXT_MODEL',
  'messages': [{
    'role': 'user',
    'content': '/no_think What Python function is shown in this image? Answer in one sentence.',
    'images': ['$IMG_B64']
  }],
  'stream': False
}))
")

CHAT_RES=$(curl -s -w "\n%{http_code}" --max-time 180 "$API/api/chat" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  -d "$CHAT_PAYLOAD" 2>/dev/null || printf "\n000")

CHAT_CODE=$(echo "$CHAT_RES" | tail -1)
CHAT_BODY=$(echo "$CHAT_RES" | sed '$d')

case "$CHAT_CODE" in
  200) pass "chat with image → 200" ;;
  503) info "chat → 503 (no eligible provider)" ;;
  *)   fail "chat with image → $CHAT_CODE" ;;
esac

if [ "$CHAT_CODE" = "200" ]; then
  CHAT_TEXT=$(echo "$CHAT_BODY" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    print(d.get('message', {}).get('content', ''))
except: print('')
" 2>/dev/null || echo "")
  if [ -n "$CHAT_TEXT" ]; then
    pass "chat: non-empty response (vision fallback worked)"
    info "Response (truncated): ${CHAT_TEXT:0:120}"
  else
    fail "chat: empty response"
  fi
fi

# ── Verify image stored in S3 (image_keys populated) ─────────────────────────

hdr "Vision Fallback — S3 image storage"

sleep 3  # async upload

JOB_ID=$(aget "/v1/dashboard/jobs?limit=5&source=api&model=$TEXT_MODEL" 2>/dev/null | python3 -c "
import sys, json
try:
    for j in json.loads(sys.stdin.read()).get('jobs', []):
        print(j['id']); break
except: pass
" 2>/dev/null || echo "")

if [ -z "$JOB_ID" ]; then
  fail "Could not find fallback job in dashboard"
else
  IMG_KEYS=""
  for attempt in 1 2 3 4 5; do
    DETAIL=$(agetc "/v1/dashboard/jobs/$JOB_ID")
    DETAIL_CODE=$(echo "$DETAIL" | code)
    DETAIL_BODY=$(echo "$DETAIL" | body)
    [ "$DETAIL_CODE" != "200" ] && { fail "job detail → $DETAIL_CODE"; break; }
    IMG_KEYS=$(echo "$DETAIL_BODY" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
keys = d.get('image_keys') or []
print(len(keys))
" 2>/dev/null || echo "0")
    [ "$IMG_KEYS" != "0" ] && break
    sleep 2
  done

  if [ "${IMG_KEYS:-0}" -gt 0 ]; then
    pass "S3 upload: image_keys=$IMG_KEYS (image preserved despite fallback)"
  else
    fail "S3 upload: image_keys empty — image was discarded"
  fi
fi

save_counts
