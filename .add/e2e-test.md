# E2E Test Suite

> ADD Execution | **Last Updated**: 2026-04-07

## Trigger

- Before PR / merge to develop
- After feature addition or bug fix that touches API/infrastructure
- Docker full rebuild verification

## Prerequisites

```bash
docker compose up -d          # All services running
bash test/scripts/e2e/01-setup.sh  # Infra + auth + provider registration
```

## Execution

Run scripts in order — each depends on state from prior scripts.

| Step | Script | Scope | Approx Time |
|------|--------|-------|-------------|
| 1 | `01-setup.sh` | Infra, auth, providers, model sync, API keys | 30s |
| 2 | `02-scheduler.sh` | Queue depth, capacity, AIMD, thermal states | 15s |
| 3 | `03-inference.sh` | Chat completions, usage tracking, performance | 60s |
| 4 | `04-crud.sh` | Account, key, provider, server, model CRUD | 30s |
| 5 | `05-security.sh` | Auth edge cases, SSRF, rate limiting, RBAC | 45s |
| 6 | `06-api-surface.sh` | Multi-format inference, endpoint smoke tests, audit | 90s |
| 7 | `07-lifecycle.sh` | Job submit → stream → cancel → delete | 60s |
| 8 | `08-sdd-advanced.sh` | Concurrent load (15 requests), AIMD stress | 120s |
| 9 | `09-metrics-pipeline.sh` | OTel → Redpanda → veronex-consumer → ClickHouse metrics ingestion | 30s |
| 10 | `10-image-storage.sh` | Image inference + S3 storage | 30s |
| 11 | `11-verify-liveness.sh` | Provider/server verification, heartbeat | 15s |
| 12 | `12-mcp.sh` | MCP CRUD, tools, embed, web search, ReAct loop | 120s |
| 13 | `13-frontend.sh` | Frontend smoke tests (Next.js page loads) | 30s |
| 14 | `14-vespa-load-test.sh` | Vespa ANN 부하 테스트 — 100K 툴, p99 < 20ms | 300s |
| 15 | `15-vision-fallback.sh` | Non-vision model + image → vision fallback injection | 60s |
| 16 | `16-context-compression.sh` | Lab settings, multi-turn eligibility, session handoff | 30s |
| 17 | `17-mcp-analytics.sh` | MCP analytics pipeline, settings CRUD, ClickHouse | 60s |

## Pass / Fail Criteria

| Result | Action |
|--------|--------|
| All PASS | Proceed with commit/PR |
| FAIL in code logic | Fix before proceeding |
| FAIL in environment (remote server unreachable, timing) | Note as INFO, verify manually |
| INFO (expected) | No action needed |

## Full Suite Command

```bash
for s in 01-setup 02-scheduler 03-inference 04-crud 05-security 06-api-surface \
         07-lifecycle 08-sdd-advanced 09-metrics-pipeline 10-image-storage \
         11-verify-liveness 12-mcp 13-frontend 15-vision-fallback \
         16-context-compression 17-mcp-analytics; do
  echo "=== $s ==="
  RESULT=$(bash test/scripts/e2e/$s.sh 2>&1)
  PASS=$(echo "$RESULT" | grep -c "\[PASS\]")
  FAIL=$(echo "$RESULT" | grep -c "\[FAIL\]")
  echo "  PASS: $PASS  FAIL: $FAIL"
  echo "$RESULT" | grep "\[FAIL\]" || true
done
```

## Parallel Runner

```bash
bash test/scripts/e2e/run-parallel.sh
```

| Wave | Scripts | Mode | Notes |
|------|---------|------|-------|
| Phase 0 | `01-setup` | sequential | DB reset + infra bootstrap |
| Wave 1 | `05` `09` `11` `13` | **parallel** | read-only / fully isolated |
| Wave 2 | `04` `06` `10` `12` `15` `17` | **parallel** | own resources; MCP slug unique per run |
| Wave 3 | `02` `03` `07` `08` `16` `14` | sequential | share AIMD + provider state; 16 patches global lab settings |

Skip setup (when state already exists):
```bash
SKIP_SETUP=1 bash test/scripts/e2e/run-parallel.sh
```

Each script is individually runnable standalone — `ensure_auth` self-bootstraps auth when no state file exists:
```bash
bash test/scripts/e2e/05-security.sh   # runs without 01-setup.sh
bash test/scripts/e2e/12-mcp.sh        # creates unique slug via E2E_RUN_ID=$$
```

## Script Ownership

| Script | Primary Domain |
|--------|---------------|
| 01-setup | Infra bootstrap |
| 02-scheduler | Scheduling, capacity, thermal |
| 03-inference | Inference pipeline |
| 04-crud | REST CRUD operations |
| 05-security | Auth, RBAC, rate limiting, SSRF |
| 06-api-surface | OpenAI/Ollama/Gemini compat, audit |
| 07-lifecycle | Job state machine |
| 08-sdd-advanced | Stress test, AIMD |
| 09-metrics-pipeline | OTel, Redpanda, veronex-consumer, ClickHouse |
| 10-image-storage | Image processing, S3 |
| 11-verify-liveness | Health checks, heartbeat |
| 12-mcp | MCP, embed, web search, ReAct |
| 13-frontend | Frontend smoke tests |
| 14-vespa-load-test | Vespa 벡터 부하 테스트 (수동 실행) |
| 15-vision-fallback | Vision fallback — non-vision model + image injection |
| 16-context-compression | Context compression lab settings + multi-turn eligibility |
| 17-mcp-analytics | MCP analytics pipeline + settings CRUD |
