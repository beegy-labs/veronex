#!/usr/bin/env bash
# Phase 14: Vespa 부하 테스트 — 10K 툴 인덱싱 후 ANN 검색 p99 < 20ms
#
# 사전 조건: Vespa (localhost:8080), veronex-embed (/embed endpoint) 실행 중
# 사용법: VESPA_URL=http://localhost:8080 EMBED_URL=http://localhost:5001 bash 14-vespa-load-test.sh
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/_lib.sh"

VESPA_URL="${VESPA_URL:-http://localhost:8080}"
EMBED_URL="${EMBED_URL:-http://localhost:5001}"
SERVICE_ID="${SERVICE_ID:-loadtest}"
TOOL_COUNT="${TOOL_COUNT:-10000}"
SEARCH_COUNT="${SEARCH_COUNT:-1000}"
TOP_K="${TOP_K:-16}"
P99_THRESHOLD_MS="${P99_THRESHOLD_MS:-20}"

hdr "Vespa Load Test — ${TOOL_COUNT} tools (default 10K; override with TOOL_COUNT=N), ${SEARCH_COUNT} queries, p99 < ${P99_THRESHOLD_MS}ms"

# ── 1. Vespa 헬스 체크 ───────────────────────────────────────────────────────

info "Checking Vespa health..."
STATUS=$(curl -sf "${VESPA_URL}/ApplicationStatus" -o /dev/null -w "%{http_code}" || echo "000")
[ "$STATUS" = "200" ] \
  && pass "Vespa is healthy (${VESPA_URL})" \
  || { fail "Vespa not reachable at ${VESPA_URL} (status: ${STATUS})"; exit 1; }

# ── 2. 기존 loadtest 문서 삭제 ──────────────────────────────────────────────

info "Deleting existing loadtest documents..."
SELECTION="mcp_tools.service_id+%3D%3D+%22${SERVICE_ID}%22"
curl -sf -X DELETE "${VESPA_URL}/document/v1/mcp_tools/mcp_tools/docid/?selection=mcp_tools.service_id+%3D%3D+%22${SERVICE_ID}%22&continuation=" \
  -o /dev/null || true
pass "Existing documents cleared"

# ── 3. 임베딩 샘플 생성 (고정 벡터 — embed 서비스 불필요) ─────────────────

# 부하 테스트는 Vespa ANN 레이턴시만 측정. 임베딩은 랜덤 1024-dim 벡터 사용.
# (실제 embed 서비스가 있으면 EMBED_URL에서 가져올 수도 있음)

python3 - <<'PYEOF'
import json, random, math, sys, time, subprocess, os, statistics

VESPA_URL    = os.environ.get("VESPA_URL", "http://localhost:8080")
SERVICE_ID   = os.environ.get("SERVICE_ID", "loadtest")
TOOL_COUNT   = int(os.environ.get("TOOL_COUNT", "10000"))
SEARCH_COUNT = int(os.environ.get("SEARCH_COUNT", "1000"))
TOP_K        = int(os.environ.get("TOP_K", "16"))
THRESHOLD_MS = float(os.environ.get("P99_THRESHOLD_MS", "20"))
DIM          = 1024

random.seed(42)

def rand_vec():
    v = [random.gauss(0, 1) for _ in range(DIM)]
    norm = math.sqrt(sum(x*x for x in v))
    return [x / norm for x in v]

CATEGORIES = ["weather", "maps", "search", "database", "calendar", "email",
              "file", "http", "math", "translate", "image", "audio",
              "video", "code", "data", "storage", "auth", "notify"]

def make_doc(i):
    cat = CATEGORIES[i % len(CATEGORIES)]
    return {
        "tool_id":      f"{SERVICE_ID}:server-{i//500}:{cat}_tool_{i}",
        "environment":  SERVICE_ID,
        "tenant_id":    "loadtest",
        "server_id":    f"server-{i//500}",
        "server_name":  f"mcp_{cat}_server",
        "tool_name":    f"{cat}_tool_{i}",
        "description":  f"Tool {i}: {cat} operation for query processing and result retrieval",
        "input_schema": json.dumps({"type": "object", "properties": {"query": {"type": "string"}}}),
        "embedding":    rand_vec(),
    }

import urllib.request, urllib.error

def feed(doc):
    doc_id = urllib.parse.quote(doc["tool_id"], safe="")
    url = f"{VESPA_URL}/document/v1/mcp_tools/mcp_tools/docid/{doc_id}"
    body = json.dumps({"fields": {
        "tool_id":      doc["tool_id"],
        "environment":  doc["environment"],
        "tenant_id":    doc["tenant_id"],
        "server_id":    doc["server_id"],
        "server_name":  doc["server_name"],
        "tool_name":    doc["tool_name"],
        "description":  doc["description"],
        "input_schema": doc["input_schema"],
        "embedding":    {"values": doc["embedding"]},
    }}).encode()
    req = urllib.request.Request(url, data=body, method="POST",
                                  headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=10):
            return True
    except Exception as e:
        return False

import urllib.parse

# ── 4. 인덱싱 ────────────────────────────────────────────────────────────────
print(f"  [INFO] Indexing {TOOL_COUNT} tools into Vespa...")
t0 = time.time()
ok = fail = 0
BATCH = 500
for i in range(TOOL_COUNT):
    if feed(make_doc(i)):
        ok += 1
    else:
        fail += 1
    if (i + 1) % BATCH == 0:
        elapsed = time.time() - t0
        tps = (i + 1) / elapsed
        print(f"  [INFO]   {i+1}/{TOOL_COUNT} ({tps:.0f} docs/s, {fail} failed)", flush=True)

elapsed = time.time() - t0
tps = ok / elapsed
print(f"  [INFO] Indexing complete: {ok} ok, {fail} failed, {tps:.0f} docs/s, {elapsed:.1f}s total")
if fail > TOOL_COUNT * 0.01:
    print(f"  [FAIL] Too many feed failures ({fail}/{TOOL_COUNT})")
    sys.exit(1)
print(f"  [PASS] Indexing: {ok}/{TOOL_COUNT} docs fed ({tps:.0f} docs/s)")

# Wait for Vespa to finish building HNSW index
print("  [INFO] Waiting 5s for HNSW index build...")
time.sleep(5)

# ── 5. ANN 검색 레이턴시 측정 ────────────────────────────────────────────────
print(f"  [INFO] Running {SEARCH_COUNT} ANN queries (top_k={TOP_K})...")

def search(embedding):
    yql = (
        f'select tool_id from mcp_tools '
        f'where environment contains "{SERVICE_ID}" '
        f'and ({{targetHits: {TOP_K}}}nearestNeighbor(embedding, qe)) '
        f'limit {TOP_K}'
    )
    body = json.dumps({
        "yql": yql,
        "hits": TOP_K,
        "ranking": "semantic",
        "input.query(qe)": {"values": embedding},
    }).encode()
    req = urllib.request.Request(
        f"{VESPA_URL}/search/",
        data=body,
        method="POST",
        headers={"Content-Type": "application/json"},
    )
    t = time.perf_counter()
    with urllib.request.urlopen(req, timeout=5):
        pass
    return (time.perf_counter() - t) * 1000  # ms

latencies = []
search_fail = 0
for i in range(SEARCH_COUNT):
    try:
        ms = search(rand_vec())
        latencies.append(ms)
    except Exception:
        search_fail += 1

if not latencies:
    print("  [FAIL] All search queries failed")
    sys.exit(1)

latencies.sort()
p50  = latencies[int(len(latencies) * 0.50)]
p95  = latencies[int(len(latencies) * 0.95)]
p99  = latencies[int(len(latencies) * 0.99)]
mean = statistics.mean(latencies)

print(f"  [INFO] Search results ({len(latencies)} queries, {search_fail} failed):")
print(f"  [INFO]   mean={mean:.2f}ms  p50={p50:.2f}ms  p95={p95:.2f}ms  p99={p99:.2f}ms")
print(f"  [INFO]   threshold: p99 < {THRESHOLD_MS}ms")

if p99 < THRESHOLD_MS:
    print(f"  [PASS] Vespa ANN p99 = {p99:.2f}ms < {THRESHOLD_MS}ms (TOOL_COUNT={TOOL_COUNT})")
else:
    print(f"  [FAIL] Vespa ANN p99 = {p99:.2f}ms >= {THRESHOLD_MS}ms (TOOL_COUNT={TOOL_COUNT})")
    sys.exit(1)

PYEOF

echo ""
hdr "Load test complete"
