#!/usr/bin/env bash
# Phase 11: Server/Provider Verify Endpoints + Liveness (PR #25)
#
# Tests:
#   - POST /v1/servers/verify   (format, duplicate, reachability)
#   - POST /v1/providers/verify (format, duplicate, reachability)
#   - Server registration validation (node_exporter_url required, duplicate, scheme)
#   - Provider registration validation (duplicate URL, unreachable)
#   - PROVIDERS_ONLINE_COUNTER in Valkey
#   - Provider heartbeat keys (if agent running)
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

# ── POST /v1/servers/verify ─────────────────────────────────────────────────

hdr "Server Verify — POST /v1/servers/verify"

# Empty URL → 400
c=$(apostc "/v1/servers/verify" '{"url":""}' | code)
[ "$c" = "400" ] && pass "Verify server: empty URL → 400" || fail "Verify server: empty URL → $c (expected 400)"

# Invalid scheme → 400
c=$(apostc "/v1/servers/verify" '{"url":"ftp://example.com"}' | code)
[ "$c" = "400" ] && pass "Verify server: ftp:// scheme → 400" || fail "Verify server: ftp:// → $c (expected 400)"

# Duplicate URL → 409 (node_exporter_url already registered in 01-setup)
NE_URL="${NODE_EXPORTER_LOCAL:-http://host.docker.internal:9100}"
c=$(apostc "/v1/servers/verify" "{\"url\":\"$NE_URL\"}" | code)
[ "$c" = "409" ] && pass "Verify server: duplicate URL → 409" || fail "Verify server: duplicate URL → $c (expected 409)"

# Unreachable URL → 502
c=$(apostc "/v1/servers/verify" '{"url":"http://192.0.2.1:19999"}' | code)
[ "$c" = "502" ] && pass "Verify server: unreachable → 502" || fail "Verify server: unreachable → $c (expected 502)"

# Valid reachable URL (use a unique port offset to avoid duplicate)
# Only test if node-exporter is accessible from host
NE_HOST_URL="${NE_URL/host.docker.internal/localhost}"
NE_ALIVE=$(curl -sf --max-time 3 "$NE_HOST_URL" > /dev/null 2>&1 && echo "yes" || echo "no")
if [ "$NE_ALIVE" = "yes" ]; then
  # Cannot verify a non-duplicate reachable URL easily without a second node-exporter
  info "Verify server: reachable test skipped (no spare node-exporter URL)"
else
  info "Verify server: reachable test skipped (node-exporter not accessible from host)"
fi

# ── POST /v1/providers/verify ────────────────────────────────────────────────

hdr "Provider Verify — POST /v1/providers/verify"

# Empty URL → 400
c=$(apostc "/v1/providers/verify" '{"url":""}' | code)
[ "$c" = "400" ] && pass "Verify provider: empty URL → 400" || fail "Verify provider: empty URL → $c (expected 400)"

# Invalid scheme → 400
c=$(apostc "/v1/providers/verify" '{"url":"ftp://example.com:11434"}' | code)
[ "$c" = "400" ] && pass "Verify provider: ftp:// scheme → 400" || fail "Verify provider: ftp:// → $c (expected 400)"

# Duplicate URL → 409 (Ollama URL already registered in 01-setup)
OLLAMA_URL="${OLLAMA_LOCAL:-http://host.docker.internal:11434}"
c=$(apostc "/v1/providers/verify" "{\"url\":\"$OLLAMA_URL\"}" | code)
[ "$c" = "409" ] && pass "Verify provider: duplicate URL → 409" || fail "Verify provider: duplicate URL → $c (expected 409)"

# Unreachable URL → 502
c=$(apostc "/v1/providers/verify" '{"url":"http://192.0.2.1:11434"}' | code)
[ "$c" = "502" ] && pass "Verify provider: unreachable → 502" || fail "Verify provider: unreachable → $c (expected 502)"

# ── Server Registration Validation ──────────────────────────────────────────

hdr "Server Registration Validation"

# Missing node_exporter_url → 400
c=$(apostc "/v1/servers" '{"name":"test-no-url"}' | code)
[ "$c" = "400" ] && pass "Register server: no URL → 400" || fail "Register server: no URL → $c (expected 400)"

# Invalid scheme → 400
c=$(apostc "/v1/servers" '{"name":"test-bad-scheme","node_exporter_url":"ftp://bad"}' | code)
[ "$c" = "400" ] && pass "Register server: bad scheme → 400" || fail "Register server: bad scheme → $c (expected 400)"

# Duplicate node_exporter_url → 409
c=$(apostc "/v1/servers" "{\"name\":\"test-dup\",\"node_exporter_url\":\"$NE_URL\"}" | code)
[ "$c" = "409" ] && pass "Register server: duplicate URL → 409" || fail "Register server: duplicate URL → $c (expected 409)"

# Unreachable → 502
c=$(apostc "/v1/servers" '{"name":"test-unreachable","node_exporter_url":"http://192.0.2.1:19999"}' | code)
[ "$c" = "502" ] && pass "Register server: unreachable → 502" || fail "Register server: unreachable → $c (expected 502)"

# ── Provider Registration Validation ─────────────────────────────────────────

hdr "Provider Registration Validation"

# Duplicate Ollama URL → 409
c=$(apostc "/v1/providers" \
  "{\"name\":\"dup-test\",\"provider_type\":\"ollama\",\"url\":\"$OLLAMA_URL\"}" | code)
[ "$c" = "409" ] && pass "Register provider: duplicate URL → 409" || fail "Register provider: duplicate URL → $c (expected 409)"

# Unreachable Ollama → 502
c=$(apostc "/v1/providers" \
  '{"name":"bad-test","provider_type":"ollama","url":"http://192.0.2.1:11434"}' | code)
[ "$c" = "502" ] && pass "Register provider: unreachable → 502" || fail "Register provider: unreachable → $c (expected 502)"

# Missing URL for Ollama → 400
c=$(apostc "/v1/providers" \
  '{"name":"no-url","provider_type":"ollama"}' | code)
[ "$c" = "400" ] && pass "Register provider: no URL → 400" || fail "Register provider: no URL → $c (expected 400)"

# Invalid scheme for Ollama → 400
c=$(apostc "/v1/providers" \
  '{"name":"bad-scheme","provider_type":"ollama","url":"ftp://bad:11434"}' | code)
[ "$c" = "400" ] && pass "Register provider: bad scheme → 400" || fail "Register provider: bad scheme → $c (expected 400)"

# ── Provider Liveness: PROVIDERS_ONLINE_COUNTER ──────────────────────────────

hdr "Provider Liveness — Valkey Keys"

ONLINE_COUNT=$(valkey_get "veronex:stats:providers:online")
if [ -n "$ONLINE_COUNT" ] && [ "$ONLINE_COUNT" != "(nil)" ]; then
  pass "PROVIDERS_ONLINE_COUNTER exists (value=$ONLINE_COUNT)"
else
  info "PROVIDERS_ONLINE_COUNTER not set (health_checker may not have run yet)"
fi

# Check heartbeat keys (only if providers are registered)
if [ -n "${PROVIDER_ID_LOCAL:-}" ] && [ "$PROVIDER_ID_LOCAL" != "None" ]; then
  HB_KEY="veronex:provider:hb:$PROVIDER_ID_LOCAL"
  HB_VAL=$(valkey_get "$HB_KEY")
  if [ -n "$HB_VAL" ] && [ "$HB_VAL" != "(nil)" ]; then
    pass "Provider heartbeat key present ($HB_KEY)"
  else
    info "Provider heartbeat key absent — agent may not be pushing heartbeats yet"
  fi
fi

if [ -n "${PROVIDER_ID_REMOTE:-}" ] && [ "$PROVIDER_ID_REMOTE" != "None" ]; then
  HB_KEY="veronex:provider:hb:$PROVIDER_ID_REMOTE"
  HB_VAL=$(valkey_get "$HB_KEY")
  if [ -n "$HB_VAL" ] && [ "$HB_VAL" != "(nil)" ]; then
    pass "Remote provider heartbeat key present"
  else
    info "Remote provider heartbeat key absent — agent may not be running"
  fi
fi

save_counts
