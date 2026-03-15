# Agent Data Optimization SDD

> **Status**: In Progress | **Last Updated**: 2026-03-15
> **Branch**: feat/ollama-compat-non-streaming
> **Scope**: Agent 코드만 수정 (Redpanda, OTel, ClickHouse 수정 금지)

---

## 목표

Agent에서 수집·전송하는 메트릭 수를 줄여
Redpanda와 ClickHouse의 신규 유입량을 최소화한다.

---

## 현황 (2026-03-15 측정)

### Redpanda (참고용 — 수정 금지)
| Topic | 용량 |
|-------|------|
| `otel.audit.metrics` | 2.1G |
| `otel.audit.logs` | 1.2G |
| `otel.audit.traces` | 97M |

### ClickHouse (참고용 — 수정 금지)
| 테이블 | 행 수 | 시간당 수집 |
|--------|-------|------------|
| `otel_metrics_gauge` | 16.3M | **806,420 rows/hr** |
| `otel_logs` | 1.0M | 2,714 rows/hr |

### Agent 수집 메트릭 분석 (시간당)

| Metric | rows/hr | 비중 | 판정 |
|--------|---------|------|------|
| `node_cpu_seconds_total` | 187,460 | 23% | **필터 필요** — mode 중 user/system/iowait/idle만 유용 |
| `node_hwmon_chip_names` | 4,066 | 0.5% | **제거** — static label, 분석 가치 없음 |
| `node_hwmon_sensor_label` | 6,226 | 0.8% | **제거** — static label, 분석 가치 없음 |
| `node_drm_*` | ~9,000 | 1.1% | 유지 — GPU 모니터링 핵심 |
| `node_hwmon_temp_celsius` | ~7,000 | 0.9% | 유지 — thermal 보호 핵심 |
| `node_hwmon_power_average_watt` | ~480 | 0.1% | 유지 — 전력 모니터링 |
| `node_memory_*` | ~2,000 | 0.3% | 유지 — 메모리 모니터링 핵심 |
| `ollama_*` | ~830 | 0.1% | 유지 — Ollama 상태 핵심 |

> `node_cpu_seconds_total`의 불필요 mode: `nice`, `irq`, `softirq`, `steal`, `guest`, `guest_nice`
> → 제거 시 187K → **~85K/hr** (55% 감소)

---

## 구현 계획

### Phase 1 — node_cpu_seconds_total mode 필터링

**대상**: `crates/veronex-agent/src/scraper.rs`

`node_cpu_seconds_total`은 CPU 코어 × mode 조합으로 폭발.
agent 레벨에서 유용한 mode만 통과시킴.

유지할 mode:
- `user` — 사용자 프로세스 CPU
- `system` — 커널 CPU
- `iowait` — I/O 대기
- `idle` — 유휴

제거할 mode: `nice`, `irq`, `softirq`, `steal`, `guest`, `guest_nice`

구현: `parse_node_exporter` 내에서 `node_cpu_seconds_total` 메트릭에 한해
`mode` label 값을 확인하여 allowlist 외 mode 제거.

**예상 효과**: 187K → 85K rows/hr (102K/hr 감소, 55% 감소)

### Phase 2 — Allowlist에서 static 메트릭 제거

**대상**: `crates/veronex-agent/src/scraper.rs`

`NODE_EXPORTER_ALLOWLIST`에서 제거:

| 항목 | 이유 |
|------|------|
| `node_hwmon_chip_names` | value 항상 0, chip 이름은 label에 포함 → 정보 없음 |
| `node_hwmon_sensor_label` | sensor 이름 매핑용 static metric → 분석 불필요 |

**예상 효과**: ~10K rows/hr 추가 감소

### Phase 3 — Scrape Interval 조정

**대상**: `crates/veronex-agent/src/main.rs` default 또는 `SCRAPE_INTERVAL_MS` 환경변수 기본값

현재 `15000ms` → `30000ms` (30초)

**트레이드오프**: GPU 온도 샘플링 30초 간격 → thermal 보호는 agent 내부 로직 아니므로 무관
**예상 효과**: 전체 수집량 2배 감소

---

## 총 예상 효과

| 현재 | → | 최적화 후 |
|------|---|----------|
| 806,420 rows/hr | | **~350,000 rows/hr (57% 감소)** |

| Phase | 감소량 |
|-------|--------|
| CPU mode 필터 | −102,000 rows/hr |
| Static 메트릭 제거 | −10,000 rows/hr |
| Scrape interval 2배 | 나머지 절반 |

---

## Tasks

| # | Task | 파일 | Status |
|---|------|------|--------|
| 1 | `node_cpu_seconds_total` mode 필터 구현 | `scraper.rs` | done |
| 2 | `node_hwmon_chip_names` allowlist 제거 | `scraper.rs` | done |
| 3 | `node_hwmon_sensor_label` allowlist 제거 | `scraper.rs` | done (never in list) |
| 4 | Scrape interval default 30초 조정 | `main.rs` | done |
| 5 | 테스트 업데이트 | `scraper.rs` | done (17/17 pass) |
