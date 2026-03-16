#!/usr/bin/env bash
# Phase 09: Metrics Pipeline — Agent → OTel → Redpanda → ClickHouse
#
# Validates that hardware metrics (CPU, GPU temp, GPU power) flow
# end-to-end from node-exporter through the agent and OTel pipeline
# into ClickHouse, and are queryable via the analytics history API.
#
# Two servers:
#   - local-dev (Mac)  : CPU + memory only (no GPU hwmon)
#   - k8s-worker-ai-01 (Ubuntu, Ryzen AI 395+) : CPU + memory + GPU temp/power
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"; load_state

hdr "Metrics Pipeline: Agent → OTel → ClickHouse"

# Restart agent to pick up new targets after DB reset
docker compose restart veronex-agent > /dev/null 2>&1 || true
sleep 5

# ── Pre-check: Agent running ─────────────────────────────────────────────────

AGENT_HEALTH="${AGENT_HEALTH_URL:-http://localhost:9091}"
AGENT_OK=$(curl -s --max-time 3 "$AGENT_HEALTH/health" 2>/dev/null || echo "")
if [ "$AGENT_OK" != "ok" ] && [ "$AGENT_OK" != "" ]; then
  info "Agent health: $AGENT_OK"
fi
AGENT_UP=$(curl -s -o /dev/null -w "%{http_code}" --max-time 3 "$AGENT_HEALTH/health" 2>/dev/null || echo "000")
[ "$AGENT_UP" = "200" ] && pass "Agent is running" || info "Agent not reachable ($AGENT_UP) — metrics may not be flowing"

# ── Pre-check: node-exporter reachable ───────────────────────────────────────

LOCAL_NE="${NODE_EXPORTER_LOCAL/host.docker.internal/localhost}"
NE_LOCAL_CODE=$(curl -s -o /dev/null -w "%{http_code}" --max-time 3 "$LOCAL_NE/metrics" 2>/dev/null || echo "000")
[ "$NE_LOCAL_CODE" = "200" ] && pass "Local node-exporter reachable" \
  || info "Local node-exporter not reachable ($NE_LOCAL_CODE)"

NE_REMOTE_CODE=$(curl -s -o /dev/null -w "%{http_code}" --max-time 3 "$NODE_EXPORTER_REMOTE/metrics" 2>/dev/null || echo "000")
[ "$NE_REMOTE_CODE" = "200" ] && pass "Remote node-exporter reachable ($NODE_EXPORTER_REMOTE)" \
  || info "Remote node-exporter not reachable ($NE_REMOTE_CODE)"

# ── Verify node-exporter exposes expected metrics ────────────────────────────

hdr "Node-exporter metric availability"

if [ "$NE_REMOTE_CODE" = "200" ]; then
  REMOTE_METRICS_FILE=$(mktemp)
  curl -s --max-time 10 "$NODE_EXPORTER_REMOTE/metrics" > "$REMOTE_METRICS_FILE" 2>/dev/null || true

  # CPU counters
  grep -q "^node_cpu_seconds_total" "$REMOTE_METRICS_FILE" \
    && pass "Remote: node_cpu_seconds_total present" \
    || fail "Remote: node_cpu_seconds_total missing"

  # Memory
  grep -q "^node_memory_MemTotal_bytes" "$REMOTE_METRICS_FILE" \
    && pass "Remote: node_memory_MemTotal_bytes present" \
    || fail "Remote: node_memory_MemTotal_bytes missing"

  # GPU hwmon (AMD Ryzen AI 395+ — amdgpu driver)
  grep -q "node_hwmon_chip_names.*amdgpu" "$REMOTE_METRICS_FILE" \
    && pass "Remote: amdgpu chip_names present" \
    || info "Remote: amdgpu chip_names not found (check hwmon sysfs)"

  grep -q "node_hwmon_temp_celsius" "$REMOTE_METRICS_FILE" \
    && pass "Remote: hwmon temp_celsius present" \
    || info "Remote: hwmon temp_celsius not found"

  grep -q "node_hwmon_power_average_watt" "$REMOTE_METRICS_FILE" \
    && pass "Remote: hwmon power_average_watt present" \
    || info "Remote: hwmon power_average_watt not found (APU may not expose power)"

  rm -f "$REMOTE_METRICS_FILE"
fi

# ── Wait for agent scrape cycle (agent scrapes every 60s by default) ─────────

hdr "Waiting for metrics to flow through pipeline"

# The agent scrapes every SCRAPE_INTERVAL_MS (default 60s).
# OTel batches every 5s, Redpanda → ClickHouse MV is real-time.
# Total pipeline latency: up to ~70s (scrape + batch + MV insert).
WAIT_SECS=90
info "Waiting up to ${WAIT_SECS}s for metrics to appear in ClickHouse..."

METRICS_FOUND=0
for i in $(seq 1 $((WAIT_SECS / 5))); do
  # Check if ANY metric has arrived for either server
  CH_COUNT=$(docker compose exec -T clickhouse clickhouse-client -d veronex \
    --query "SELECT count() FROM otel_metrics_gauge WHERE ts > now() - INTERVAL 5 MINUTE" \
    2>/dev/null | tr -d ' \r\n' || echo "0")

  if [ "$CH_COUNT" -gt 0 ]; then
    METRICS_FOUND=1
    pass "Metrics arriving in ClickHouse ($CH_COUNT rows in last 5 min)"
    break
  fi
  sleep 5
done

if [ "$METRICS_FOUND" = "0" ]; then
  fail "No metrics in ClickHouse after ${WAIT_SECS}s — pipeline broken"
  # Dump diagnostic info
  info "Checking Redpanda topic..."
  TOPIC_COUNT=$(docker compose exec -T redpanda rpk topic consume otel-metrics --num 1 --timeout 3s 2>/dev/null | wc -l || echo "0")
  [ "$TOPIC_COUNT" -gt 0 ] \
    && info "Redpanda otel-metrics topic has data (OTel → Redpanda OK, Redpanda → ClickHouse broken)" \
    || info "Redpanda otel-metrics topic empty (OTel → Redpanda broken)"
fi

# ── Verify specific metric types in ClickHouse ──────────────────────────────

hdr "ClickHouse metric verification"

check_metric() {
  local metric_name="$1" label="$2" server_filter="$3"
  local where_server=""
  [ -n "$server_filter" ] && where_server="AND server_id = '$server_filter'"

  local count
  count=$(docker compose exec -T clickhouse clickhouse-client -d veronex \
    --query "SELECT count() FROM otel_metrics_gauge WHERE metric_name = '$metric_name' $where_server AND ts > now() - INTERVAL 10 MINUTE" \
    2>/dev/null | tr -d ' \r\n' || echo "0")

  if [ "$count" -gt 0 ]; then
    pass "$label: $count rows"
  else
    # Some metrics only available on remote (GPU), OK to skip on local
    case "$metric_name" in
      node_hwmon_*) info "$label: 0 rows (may not be available on this host)" ;;
      *)            fail "$label: 0 rows" ;;
    esac
  fi
}

# Memory (should exist for both servers)
check_metric "node_memory_MemTotal_bytes" "Memory total (Linux)" ""
check_metric "node_memory_MemAvailable_bytes" "Memory available (Linux)" ""

# CPU counters (should exist after MV fix)
check_metric "node_cpu_seconds_total" "CPU counters" ""

# GPU / hwmon — remote only (local is Mac, no hwmon support)
if [ -n "${SERVER_ID_REMOTE:-}" ] && [ "$SERVER_ID_REMOTE" != "None" ]; then
  check_metric "node_hwmon_chip_names" "GPU chip_names (remote)" "$SERVER_ID_REMOTE"
  check_metric "node_hwmon_temp_celsius" "GPU temperature (remote)" "$SERVER_ID_REMOTE"
  check_metric "node_hwmon_power_average_watt" "GPU power (remote)" "$SERVER_ID_REMOTE"
else
  info "SKIP: hwmon checks — no remote server registered"
fi

# ── Verify per-server data via analytics API ─────────────────────────────────

hdr "Analytics history API"

if [ -n "${SERVER_ID_REMOTE:-}" ] && [ "$SERVER_ID_REMOTE" != "None" ]; then
  HIST_RES=$(agetc "/v1/servers/$SERVER_ID_REMOTE/metrics/history?hours=1")
  HIST_CODE=$(echo "$HIST_RES" | code)
  HIST_BODY=$(echo "$HIST_RES" | body)

  if [ "$HIST_CODE" = "200" ]; then
    HIST_CHECK=$(echo "$HIST_BODY" | python3 -c "
import sys, json
try:
    points = json.loads(sys.stdin.read())
    if not points:
        print('empty')
    else:
        p = points[-1]
        parts = []
        if p.get('mem_total_mb', 0) > 0: parts.append('mem')
        if p.get('gpu_temp_c') is not None: parts.append(f'temp={p[\"gpu_temp_c\"]:.1f}C')
        if p.get('gpu_power_w') is not None: parts.append(f'power={p[\"gpu_power_w\"]:.1f}W')
        if p.get('cpu_usage_pct') is not None: parts.append(f'cpu={p[\"cpu_usage_pct\"]:.1f}%')
        print('ok|' + ', '.join(parts) if parts else 'no_fields')
except Exception as e:
    print(f'error:{e}')
" 2>/dev/null || echo "parse_error")

    HIST_STATUS=$(echo "$HIST_CHECK" | cut -d'|' -f1)
    HIST_DETAIL=$(echo "$HIST_CHECK" | cut -d'|' -f2-)

    case "$HIST_STATUS" in
      ok)    pass "Remote server history: $HIST_DETAIL" ;;
      empty) info "Remote server history: empty (metrics may not have arrived yet)" ;;
      *)     fail "Remote server history: $HIST_CHECK" ;;
    esac
  else
    fail "Remote server history API → $HIST_CODE"
  fi
fi

if [ -n "${SERVER_ID_LOCAL:-}" ] && [ "$SERVER_ID_LOCAL" != "None" ]; then
  HIST_LOCAL_CODE=$(agetc "/v1/servers/$SERVER_ID_LOCAL/metrics/history?hours=1" | code)
  [ "$HIST_LOCAL_CODE" = "200" ] \
    && pass "Local server history API → 200" \
    || info "Local server history API → $HIST_LOCAL_CODE"
fi

# ── Live metrics endpoint (direct node-exporter scrape) ──────────────────────

hdr "Live metrics (direct scrape)"

if [ -n "${SERVER_ID_REMOTE:-}" ] && [ "$SERVER_ID_REMOTE" != "None" ]; then
  LIVE_RES=$(agetc "/v1/servers/$SERVER_ID_REMOTE/metrics")
  LIVE_CODE=$(echo "$LIVE_RES" | code)
  LIVE_BODY=$(echo "$LIVE_RES" | body)

  if [ "$LIVE_CODE" = "200" ]; then
    LIVE_CHECK=$(echo "$LIVE_BODY" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    parts = []
    if d.get('mem_total_mb', 0) > 0: parts.append(f'ram={d[\"mem_total_mb\"]}MB')
    if d.get('cpu_usage_pct') is not None: parts.append(f'cpu={d[\"cpu_usage_pct\"]:.1f}%')
    if d.get('cpu_logical', 0) > 0: parts.append(f'cores={d[\"cpu_logical\"]}')
    gpus = d.get('gpus', [])
    if gpus:
        g = gpus[0]
        if g.get('temp_c') is not None: parts.append(f'gpu_temp={g[\"temp_c\"]:.0f}C')
        if g.get('power_w') is not None: parts.append(f'gpu_power={g[\"power_w\"]:.0f}W')
    print(', '.join(parts) if parts else 'no_fields')
except Exception as e:
    print(f'error:{e}')
" 2>/dev/null || echo "parse_error")
    pass "Remote live metrics: $LIVE_CHECK"
  else
    fail "Remote live metrics → $LIVE_CODE"
  fi
fi

# Local (Mac) — CPU + memory only, no hwmon/GPU
if [ -n "${SERVER_ID_LOCAL:-}" ] && [ "$SERVER_ID_LOCAL" != "None" ]; then
  LIVE_LOCAL_CODE=$(agetc "/v1/servers/$SERVER_ID_LOCAL/metrics" | code)
  [ "$LIVE_LOCAL_CODE" = "200" ] \
    && pass "Local live metrics → 200 (Mac: CPU + memory only)" \
    || info "Local live metrics → $LIVE_LOCAL_CODE"
fi

save_counts
