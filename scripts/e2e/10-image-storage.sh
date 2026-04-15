#!/usr/bin/env bash
# Phase 10: Image Storage — WebP upload, thumbnails, provider_name
#
# Runs AFTER parallel phases (08-sdd-advanced may Scale-In providers).
# Tests both API key (/api/generate) and session auth (/v1/chat/completions) paths.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; ensure_auth

hdr "Image Storage & Provider Name (post-parallel)"

# ── Detect vision model ──────────────────────────────────────────────────────

VISION_MODEL=$(get_vision_model)

if [ -z "$VISION_MODEL" ]; then
  fail "No vision model on local Ollama — a vision model (llava/qwen-vl/minicpm-v) must be loaded"
  save_counts
  exit 1
fi
info "Vision model: $VISION_MODEL"

# ── Generate test image ──────────────────────────────────────────────────────

BEE_IMG=$(python3 -c "
from PIL import Image, ImageDraw
import base64, io
img = Image.new('RGB', (128, 128), '#87CEEB')
draw = ImageDraw.Draw(img)
draw.ellipse([35,45,95,85], fill='#FFD700', outline='black', width=2)
draw.ellipse([85,50,110,80], fill='#FFD700', outline='black', width=2)
buf = io.BytesIO()
img.save(buf, format='JPEG', quality=85)
print(base64.b64encode(buf.getvalue()).decode())
" 2>/dev/null || echo "")

if [ -z "$BEE_IMG" ]; then
  fail "Pillow not installed — run: pip install pillow"
  save_counts
  exit 1
fi

# ── Ensure providers are active (Scale-In recovery) ─────────────────────────

# Sync vision model + warm up with vision model (not text model)
# This triggers Scale-Out and loads the vision model into VRAM
apost "/v1/ollama/models/sync" "{}" > /dev/null 2>&1 || true
for i in $(seq 1 10); do
  MODELS=$(aget "/v1/ollama/models" 2>/dev/null || echo "[]")
  echo "$MODELS" | python3 -c "
import sys, json
try:
    for m in json.loads(sys.stdin.read()).get('models', []):
        if '$VISION_MODEL' in m.get('model_name',''): exit(0)
except: pass
exit(1)
" 2>/dev/null && break
  sleep 2
done

# Warm-up + immediate image test in quick succession to beat Scale-In holddown.
# We send a text request to load the vision model, then immediately fire image requests.
info "Warming up with vision model ($VISION_MODEL)..."
curl -s --max-time 120 "$API/api/generate" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$VISION_MODEL\",\"prompt\":\"say ok\",\"stream\":false}" > /dev/null 2>&1 || true

# Fire both image inference requests IMMEDIATELY (no sleep — Scale-In runs every 5s)
info "Firing image tests immediately after warm-up..."

# Record start time before firing — used to filter out concurrent jobs from other test scripts
IMG_TEST_START=$(python3 -c "import datetime; print(datetime.datetime.utcnow().isoformat())")

API_IMG_RES=""
TEST_IMG_RES=""
TMPDIR_IMG=$(mktemp -d)

(curl -s -w "\n%{http_code}" --max-time 120 "$API/api/generate" \
  -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
  -d "{\"model\":\"$VISION_MODEL\",\"prompt\":\"/no_think Describe this image in one sentence.\",\"images\":[\"$BEE_IMG\"],\"stream\":false}" \
  > "$TMPDIR_IMG/api" 2>/dev/null || printf "\n000" > "$TMPDIR_IMG/api") &

(curl -s -w "\n%{http_code}" --max-time 120 "$API/v1/chat/completions" \
  -H "Authorization: Bearer $TK" -H "Content-Type: application/json" \
  -d "{\"model\":\"$VISION_MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"/no_think What is this?\"}],\"images\":[\"$BEE_IMG\"],\"stream\":false,\"provider_type\":\"ollama\"}" \
  > "$TMPDIR_IMG/test" 2>/dev/null || printf "\n000" > "$TMPDIR_IMG/test") &

wait

API_IMG_CODE=$(tail -1 "$TMPDIR_IMG/api" 2>/dev/null || echo "000")
API_IMG_BODY=$(sed '$d' "$TMPDIR_IMG/api" 2>/dev/null || echo "")
TEST_IMG_CODE=$(tail -1 "$TMPDIR_IMG/test" 2>/dev/null || echo "000")
TEST_IMG_BODY=$(sed '$d' "$TMPDIR_IMG/test" 2>/dev/null || echo "")
rm -rf "$TMPDIR_IMG"

# ── Helper function ──────────────────────────────────────────────────────────

verify_image_job() {
  local job_id="$1" label="$2"
  if [ -z "$job_id" ] || [ "$job_id" = "None" ]; then
    fail "$label: job ID not found"
    return
  fi

  # Poll for job completion + image_keys (async upload; longer wait under parallel load)
  local img_body=""
  for attempt in 1 2 3 4 5 6 7 8 9 10; do
    local detail_res
    detail_res=$(agetc "/v1/dashboard/jobs/$job_id")
    local detail_code
    detail_code=$(echo "$detail_res" | code)
    img_body=$(echo "$detail_res" | body)
    [ "$detail_code" != "200" ] && { fail "$label: job detail → $detail_code"; return; }

    local ready
    ready=$(echo "$img_body" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
status = d.get('status','')
has_keys = bool(d.get('image_keys') or [])
print('yes' if status == 'completed' and has_keys else 'no')
" 2>/dev/null || echo "no")
    [ "$ready" = "yes" ] && break
    sleep 3
  done

  local parsed
  parsed=$(echo "$img_body" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
keys = d.get('image_keys') or []
urls = d.get('image_urls') or []
pname = d.get('provider_name')
status = d.get('status')
thumb = next((u for u in urls if '_thumb' in u), '')
print(f'{status}|{len(keys)}|{len(urls)}|{pname or \"\"}|{thumb}')
" 2>/dev/null || echo "error|0|0||")

  local job_status key_count url_count prov_name thumb_url
  IFS='|' read -r job_status key_count url_count prov_name thumb_url <<< "$parsed"

  if [ "$job_status" != "completed" ]; then
    fail "$label: status=$job_status (expected completed)"
    return
  fi
  pass "$label: completed"

  if [ "$key_count" -gt 0 ] && [ "$url_count" -gt 0 ]; then
    pass "$label: S3 stored (keys=$key_count, urls=$url_count)"
  elif [ "$key_count" = "0" ]; then
    fail "$label: image_keys empty (async upload failed)"
    return
  else
    fail "$label: image_urls empty"
    return
  fi

  if [ -n "$prov_name" ]; then
    pass "$label: provider_name=$prov_name"
  else
    fail "$label: provider_name not set in job record"
  fi

  if [ -n "$thumb_url" ]; then
    local tcode
    tcode=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$thumb_url" 2>/dev/null || echo "000")
    [ "$tcode" = "200" ] \
      && pass "$label: thumbnail → 200" \
      || fail "$label: thumbnail → $tcode"
  fi
}

# ── Verify results ───────────────────────────────────────────────────────────

hdr "Image Inference — API key (/api/generate)"

case "$API_IMG_CODE" in
  200) pass "API image inference → 200" ;;
  503) fail "API image inference → 503 (no eligible provider)"; save_counts; exit 1 ;;
  *)   fail "API image inference → $API_IMG_CODE"; save_counts; exit 0 ;;
esac

if [ "$API_IMG_CODE" = "200" ]; then
  API_IMG_TEXT=$(echo "$API_IMG_BODY" | python3 -c "
import sys, json
try: print(json.loads(sys.stdin.read()).get('response', ''))
except: print('')
" 2>/dev/null || echo "")
  [ -n "$API_IMG_TEXT" ] \
    && pass "API image: non-empty response text" \
    || fail "API image: empty response — model may not have processed the image"
  info "API response: ${API_IMG_TEXT:0:100}"
fi

hdr "Image Inference — Session auth (/v1/chat/completions)"

case "$TEST_IMG_CODE" in
  200) pass "Test image inference → 200" ;;
  503) fail "Test image inference → 503 (no eligible provider)" ;;
  *)   fail "Test image inference → $TEST_IMG_CODE" ;;
esac

if [ "$TEST_IMG_CODE" = "200" ]; then
  TEST_IMG_TEXT=$(echo "$TEST_IMG_BODY" | python3 -c "
import sys, json
try: print(json.loads(sys.stdin.read()).get('choices',[{}])[0].get('message',{}).get('content',''))
except: print('')
" 2>/dev/null || echo "")
  [ -n "$TEST_IMG_TEXT" ] \
    && pass "Test image: non-empty response text" \
    || fail "Test image: empty response — model may not have processed the image"
  info "Test response: ${TEST_IMG_TEXT:0:100}"
fi

# Wait for async image uploads (extra time under parallel load)
sleep 8

hdr "Image Storage Verification"

API_JOB_ID=$(aget "/v1/dashboard/jobs?limit=20&source=api&model=$VISION_MODEL" 2>/dev/null | python3 -c "
import sys, json
try:
    # Filter jobs created after IMG_TEST_START to exclude concurrent no-image jobs from other scripts
    start = '${IMG_TEST_START}'
    for j in json.loads(sys.stdin.read()).get('jobs', []):
        if j.get('created_at', '') >= start:
            print(j['id']); break
except: pass
" 2>/dev/null || echo "")
verify_image_job "$API_JOB_ID" "API image job"

if [ "$TEST_IMG_CODE" = "200" ]; then
  TEST_JOB_ID=$(aget "/v1/dashboard/jobs?limit=20&source=test&model=$VISION_MODEL" 2>/dev/null | python3 -c "
import sys, json
try:
    start = '${IMG_TEST_START}'
    for j in json.loads(sys.stdin.read()).get('jobs', []):
        if j.get('created_at', '') >= start:
            print(j['id']); break
except: pass
" 2>/dev/null || echo "")
  verify_image_job "$TEST_JOB_ID" "Test image job"
fi

save_counts
