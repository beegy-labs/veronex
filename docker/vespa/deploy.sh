#!/bin/sh
# Deploy Vespa application package (mcp_tools schema) to the config server.
# Runs as a one-shot container after Vespa is healthy.
set -eu

VESPA_CONFIG="${VESPA_CONFIG_URL:-http://vespa:19071}"
APP_DIR="${APP_DIR:-/vespa-app}"
MAX_WAIT="${MAX_WAIT:-120}"

# ── Wait for config server ────────────────────────────────────────────────────
echo "[vespa-init] Waiting for config server at $VESPA_CONFIG ..."
waited=0
until curl -sf "$VESPA_CONFIG/ApplicationStatus" > /dev/null 2>&1; do
  if [ "$waited" -ge "$MAX_WAIT" ]; then
    echo "[vespa-init] ERROR: config server not ready after ${MAX_WAIT}s — aborting"
    exit 1
  fi
  sleep 3
  waited=$((waited + 3))
done
echo "[vespa-init] Config server ready (${waited}s)"

# ── Package and deploy ────────────────────────────────────────────────────────
echo "[vespa-init] Packaging application..."
ZIP=/tmp/vespa-app.zip
cd "$APP_DIR"
zip -qr "$ZIP" .

echo "[vespa-init] Deploying application package..."
HTTP_CODE=$(curl -sf -w "%{http_code}" -o /tmp/vespa-deploy.out \
  -X POST "$VESPA_CONFIG/application/v2/tenant/default/prepareandactivate" \
  --data-binary @"$ZIP" \
  -H "Content-Type: application/zip")

echo "[vespa-init] Deploy response (HTTP $HTTP_CODE):"
cat /tmp/vespa-deploy.out
echo ""

case "$HTTP_CODE" in
  200) echo "[vespa-init] Application deployed successfully" ;;
  *) echo "[vespa-init] ERROR: deploy returned HTTP $HTTP_CODE"; exit 1 ;;
esac

# ── Wait for application to become active ────────────────────────────────────
echo "[vespa-init] Waiting for application to activate..."
waited=0
until curl -sf "$VESPA_CONFIG/application/v2/tenant/default/application/default" > /dev/null 2>&1; do
  if [ "$waited" -ge 60 ]; then
    echo "[vespa-init] WARNING: application not confirmed active after 60s — proceeding"
    exit 0
  fi
  sleep 2
  waited=$((waited + 2))
done
echo "[vespa-init] Application active (${waited}s)"
