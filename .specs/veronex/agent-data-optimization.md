# Agent Data Optimization SDD

> **Status**: Planned | **Last Updated**: 2026-03-15
> **Branch**: feat/ollama-compat-non-streaming (same feature branch)

---

## 목표

Agent → OTel Collector → Redpanda → ClickHouse 파이프라인에서
저장 용량을 최소화한다.

---

## 현황 (2026-03-15 측정)

### Redpanda (50G PVC, 3.9G 사용)

| Topic | 용량 | 원인 |
|-------|------|------|
| `otel.audit.metrics` | 2.1G | retention 없음 → 무한 축적 |
| `otel.audit.logs` | 1.2G | retention 없음 → 무한 축적 |
| `otel.audit.traces` | 97M | retention 없음 |

> Kafka/Redpanda는 consumer가 소비해도 retention 설정 없으면 삭제 안 함.
> ClickHouse가 거의 실시간으로 소비하지만 메시지는 계속 남아있음.

### ClickHouse (veronex DB)

| 테이블 | 행 수 | 용량 | 시간당 수집 |
|--------|-------|------|------------|
| `otel_metrics_gauge` | 16.3M | 121 MiB | 806,420 rows |
| `otel_logs` | 1.0M | 70.6 MiB | 2,714 rows |
| `audit_events` | 9 | 2.16 KiB | — |

> TTL은 init migration에 placeholder로 설정됨 → Helm values에서 metrics 30일, analytics 90일, audit 365일로 배포 시 치환.

### Agent 수집 메트릭 분석 (시간당, 상위)

| Metric | rows/hr | 비중 | 필요성 |
|--------|---------|------|--------|
| `node_cpu_seconds_total` | 187,460 | 23% | Partial — user/system/iowait만 필요 |
| `node_network_*` (25종) | ~250,000 | 31% | 불필요 — 현재 allowlist에 없으나 ClickHouse에 과거 데이터 존재 |
| `node_cpu_scaling_governor` | 31,633 | 4% | 불필요 |
| `node_cpu_guest_seconds_total` | 31,633 | 4% | 불필요 |
| `node_cpu_scaling_frequency_*` (4종) | 63,000 | 8% | 불필요 |
| `node_hwmon_chip_names` | 4,066 | 0.5% | 불필요 (static label) |
| `node_hwmon_sensor_label` | 6,226 | 0.8% | 불필요 (static label) |
| `node_drm_*`, `node_hwmon_temp_*` | ~15,000 | 2% | 필요 (GPU 모니터링 핵심) |
| `node_memory_*`, `ollama_*` | ~8,000 | 1% | 필요 |

---

## 최적화 계획

### Phase 1 — Redpanda Retention 설정 (최고 효과)

**대상**: platform-gitops `clusters/home/values/redpanda-values.yaml`

모든 otel topic에 retention 설정:

| Topic | Retention | 근거 |
|-------|-----------|------|
| `otel.audit.metrics` | 2시간 | ClickHouse 실시간 소비, buffer 여유만 필요 |
| `otel.audit.logs` | 2시간 | 동일 |
| `otel.audit.traces` | 2시간 | 동일 |

**예상 효과**: 3.4G → ~200MB (98% 감소)

### Phase 2 — Agent Allowlist 정밀화

**대상**: `crates/veronex-agent/src/scraper.rs`

현재 `NODE_EXPORTER_ALLOWLIST`에서 제거/수정:

| 변경 | 항목 | 이유 |
|------|------|------|
| 제거 | `node_cpu_scaling_governor` | 클럭 거버너 — 분석 불필요 |
| 제거 | `node_cpu_guest_seconds_total` | VM guest CPU — 베어메탈 불필요 |
| 제거 | `node_hwmon_chip_names` | static label, value 무의미 |
| 제거 | `node_hwmon_sensor_label` | static label, value 무의미 |
| 유지 | `node_cpu_seconds_total` | 전체 mode 유지 (mode 필터는 OTel에서) |
| 유지 | `node_drm_*` | GPU 모니터링 핵심 |
| 유지 | `node_hwmon_temp_celsius` | 열 보호 핵심 |
| 유지 | `node_hwmon_power_average_watt` | 전력 모니터링 |
| 유지 | `node_memory_*` | 메모리 모니터링 핵심 |
| 유지 | `ollama_*` | Ollama 상태 핵심 |

**예상 효과**: metrics 수집량 ~12% 감소 (67K rows/hr 감소)

### Phase 3 — ClickHouse TTL 추가

**대상**: `migrations/clickhouse/000001_init.up.sql` + 4개 SQL 파일

```sql
-- otel_metrics_gauge
ALTER TABLE veronex.otel_metrics_gauge
  MODIFY TTL toDateTime(ts) + INTERVAL 30 DAY;

-- otel_logs
ALTER TABLE veronex.otel_logs
  MODIFY TTL toDateTime(Timestamp) + INTERVAL 7 DAY;
```

| 테이블 | TTL | 근거 |
|--------|-----|------|
| `otel_metrics_gauge` | 30일 | 월별 트렌드 분석 필요 |
| `otel_logs` | 7일 | 최근 로그만 디버깅에 유용 |

**예상 효과**: 장기 무한 증가 방지, 현재 데이터는 30일 후 정리 시작

### Phase 4 — Scrape Interval 조정 (선택)

**대상**: `clusters/home/values/veronex-dev-values.yaml` + prod values

현재 `scrapeIntervalMs: 15000` → `60000` (15초 → 60초)

**효과**: metrics 4배 감소 (806K → ~200K rows/hr)
**트레이드오프**: GPU 온도 모니터링 실시간성 1분 지연 → thermal 보호는 agent 내부 로직이므로 무관

---

## 구현 순서

| 순서 | Phase | 예상 감소 | 비고 |
|------|-------|----------|------|
| 1 | Redpanda Retention | Redpanda 98% | GitOps → Terraform, 즉시 효과 |
| 2 | ClickHouse TTL | 장기 무한 증가 방지 | DB migration |
| 3 | Agent Allowlist | metrics 12% 감소 | 코드 변경 + 빌드/배포 |
| 4 | Scrape Interval | metrics 75% 감소 | values.yaml 변경 |

## Tasks

| # | Task | 대상 | Status |
|---|------|------|--------|
| 1 | Redpanda topic retention 설정 | platform-gitops | pending |
| 2 | ClickHouse `otel_metrics_gauge` TTL 30일 | migration SQL | **done** (init migration TTL placeholder + Helm values) |
| 3 | ClickHouse `otel_logs` TTL 90일 | migration SQL | **done** (init migration TTL placeholder + Helm values) |
| 4 | Agent allowlist 4개 항목 제거 | scraper.rs | **done** (allowlist에 미포함 상태) |
| 5 | Scrape interval 60초로 조정 | values.yaml | **done** |
