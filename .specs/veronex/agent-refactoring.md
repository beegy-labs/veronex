# Agent Refactoring SDD

> **Status**: Pending | **Last Updated**: 2026-03-15
> **Branch**: feat/agent-refactoring (미생성)
> **Scope**: `crates/veronex-agent/src/` 만 수정. Redpanda, OTel Collector, ClickHouse 수정 금지.

---

## 설계 원칙: Graceful Degradation

**서버 정보(node-exporter) 없음 → 기본 기능만 지원**
**서버 정보(node-exporter) 있음 → 핵심 기능 전체 지원**

```
┌─────────────────────────────────────────────────────────┐
│              서버 정보 있음 (node-exporter 정상)          │
│                                                         │
│  ✅ VRAM-aware dispatch (실시간 잔여 VRAM 기반 라우팅)   │
│  ✅ Thermal gate (85°C 초과 시 자동 차단)               │
│  ✅ AIMD 안정화 (부하 기반 요청 속도 조정)               │
│  ✅ Capacity learning (용량 학습 및 예측)                │
│  ✅ ClickHouse 메트릭 분석                              │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│           서버 정보 없음 (node-exporter / agent 장애)    │
│                                                         │
│  ✅ 추론 요청 처리 (기본 dispatch 계속)                  │
│  ✅ 정적 등록 VRAM 기반 라우팅 (stale fallback)          │
│  ⚠️  Thermal gate 비활성 (온도 데이터 없음)              │
│  ⚠️  AIMD/capacity 기능 저하 (메트릭 없음)              │
│  ❌ 실시간 VRAM 분석 불가                               │
└─────────────────────────────────────────────────────────┘
```

> 에이전트와 node-exporter는 **서포트 역할** — 장애 시 기본 추론은 반드시 유지.
> 핵심 기능(thermal, VRAM-aware)은 서버 정보가 있을 때만 활성화.
> 이 동작은 코드상 이미 구현되어 있으나 **명시적으로 보장/테스트되지 않음** → 이번 리팩토링에서 명문화.

---

## 목표

현재 기능을 유지하면서:
1. **Graceful Degradation 명문화** — 서버 정보 없을 때 기본 기능 보장을 명시적으로 테스트
2. **에이전트 장애 격리** (K8s 3종 probe로 hung 상태 자동 복구)
3. 에이전트 자체 관측성 추가 (자가 메트릭)
4. OTLP push 신뢰성 개선 (재시도)
5. 에러 처리 일관성 개선

---

## 장애 격리 분석

### node-exporter 장애 시 동작

```
node-exporter DOWN
  ↓
health_checker: fetch_node_metrics() → Err → return (Valkey 업데이트 안 함)
  ↓
Valkey 캐시 60s TTL 만료 → hw 데이터 없음
  ↓
get_ollama_available_vram_mb() 캐시 미스
  ├── total_vram_mb == 0 → i64::MAX (무제한 취급, provider_router.rs:520)
  └── total_vram_mb  > 0 → 등록된 정적 VRAM 값 (provider_router.rs:523)
  ↓
dispatcher: 계속 dispatch ✅
```

**이미 동작함** — 추론은 node-exporter 장애와 무관하게 계속됨.

| 항목 | node-exporter 정상 | 장애 (60s TTL 만료 후) |
|------|-------------------|----------------------|
| VRAM 정확도 | 실시간 | 정적 등록값 (stale) |
| Thermal gate | 85°C 초과 시 차단 | **비활성화** (0°C 가정) |
| 추론 가능 여부 | ✅ | ✅ |

> **트레이드오프**: node-exporter 장애 시 thermal 보호가 비활성화됨.
> GPU 온도를 알 수 없으면 추론을 막을 수 없으므로 이 동작은 의도적.
> Ollama 자체도 GPU 과열 시 자체 보호 기능이 있음.

---

### 에이전트 장애 시 veronex 기본 기능 영향 범위

```
┌─────────────────────┐        ┌──────────────────────────────┐
│   veronex-agent     │        │   veronex (main service)     │
│                     │        │                              │
│ scrape → OTLP push  │──────► │  ClickHouse (분석용)         │
│                     │        │                              │
│ GET /v1/metrics/    │──────► │  target discovery API        │
│   targets           │        │  (에이전트가 뭘 긁을지 조회) │
└─────────────────────┘        │                              │
                                │  health_checker ─────────►  │
                                │  node-exporter 직접 폴링    │
                                │    ↓                         │
                                │  Valkey (60s TTL)            │
                                │    ↓                         │
                                │  dispatcher (라우팅 결정)    │
                                └──────────────────────────────┘
```

| 에이전트 장애 시나리오 | 추론 라우팅 영향 | 분석 데이터 영향 |
|----------------------|----------------|----------------|
| 에이전트 crash/재시작 | **없음** | ClickHouse 수집 일시 중단 |
| OTLP push 실패 | **없음** | 해당 사이클 데이터 소실 |
| target discovery 실패 | **없음** (빈 타겟 → 스킵) | 해당 사이클 스킵 |
| 에이전트 hung (무한 대기) | **없음** (별도 프로세스) | K8s가 감지 못함 ← 문제 |

**결론**: 에이전트는 veronex 추론 라우팅과 **완전히 분리됨**.
- 라우팅(thermal gate, VRAM 체크)은 veronex 내부 `health_checker`가 node-exporter를 직접 폴링
- 에이전트는 ClickHouse 분석용 데이터만 담당

**단, 현재 문제**: K8s liveness probe 없음 → 에이전트가 hung 상태가 되면 영원히 재시작 안 됨.

---

## 현재 상태 분석

### 잘 동작하는 부분 (변경 불필요)

| 항목 | 파일 | 상태 |
|------|------|------|
| Shard 해싱 로직 | `shard.rs` | 정확함 — proptest 17/17 통과 |
| Scrape loop 구조 | `main.rs` | `biased select` + graceful shutdown 올바름 |
| DOS 보호 | `scraper.rs` | body 크기, 라벨 개수, 모델 개수 모두 제한됨 |
| CPU mode 필터 | `scraper.rs` | user/system/iowait/idle만 통과 (55% 볼륨 감소) |
| 메트릭 allowlist | `scraper.rs` | GPU 서버 모니터링 목적에 적합 |
| **Graceful Degradation** | `thermal.rs:260`, `provider_router.rs:520` | 서버 정보 없으면 `ThrottleLevel::Normal` + 정적 VRAM — 기본 dispatch 유지 ✅ |

### 개선 필요 항목

#### 1. 에이전트 자가 관측성 없음 (MEDIUM)

**현재**: 에이전트 자체의 동작 상태를 나타내는 메트릭이 없음.
Scrape 사이클이 느려지거나 OTLP push가 실패해도 ClickHouse에서 볼 수 없음.

**추가할 자가 메트릭** (node-exporter/Ollama 메트릭과 함께 OTLP push):

| 메트릭 이름 | 타입 | 설명 |
|------------|------|------|
| `veronex_agent_scrape_duration_seconds` | gauge | 마지막 scrape 사이클 소요 시간 |
| `veronex_agent_scrape_targets_total` | gauge | 이번 사이클에서 수집한 타겟 수 |
| `veronex_agent_gauges_collected_total` | gauge | 이번 사이클에서 수집한 gauge 개수 |
| `veronex_agent_otlp_push_errors_total` | gauge | 누적 OTLP push 실패 횟수 |
| `veronex_agent_uptime_seconds` | gauge | 에이전트 시작 후 경과 시간 |

> Redpanda/OTel 수정 없음 — 기존 OTLP 파이프라인으로 같이 전송됨.

#### 2. OTLP push 재시도 없음 (LOW)

**`otlp.rs:77`**: 단일 POST 시도, 실패 시 데이터 소실.

**현재**:
```rust
let resp = client.post(endpoint).json(&payload).send().await?;
// 실패 시 에러 로그만 남기고 사이클 계속 진행
```

**개선**: 1회 재시도 + 5초 대기 (총 2회 시도). 과도한 재시도는 backpressure 유발이므로 최소화.

```rust
for attempt in 0..2 {
    match push_once(client, endpoint, &payload).await {
        Ok(_) => return Ok(()),
        Err(e) if attempt == 0 => {
            tracing::warn!("otlp push failed, retrying in 5s: {e}");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        Err(e) => {
            tracing::error!("otlp push failed after retry: {e}");
            // 에러 카운터 증가
        }
    }
}
```

#### 3. Target discovery 실패 시 silent (LOW)

**`main.rs:80`**:
```rust
.json::<Vec<SdTarget>>().await.unwrap_or_default()
```

JSON 파싱 실패 시 빈 벡터 반환 — 로그도 없음.

**개선**: 파싱 실패 시 `tracing::warn!` 추가:
```rust
match resp.json::<Vec<SdTarget>>().await {
    Ok(targets) => targets,
    Err(e) => {
        tracing::warn!(url = sd_url, "sd target parse failed: {e}");
        vec![]
    }
}
```

#### 4. K8s probe 전무 (HIGH)

**`deploy/helm/veronex/templates/veronex-agent-statefulset.yaml`**: `startupProbe`, `livenessProbe`, `readinessProbe` 모두 미정의.

- **startupProbe 없음** → K8s가 컨테이너 시작 즉시 liveness 체크 → 첫 scrape 전에 재시작 가능
- **livenessProbe 없음** → scrape loop hung 감지 불가, 영구 비정상 상태
- **readinessProbe 없음** → 첫 scrape 완료 전에 트래픽 수신 가능 (target discovery 미완료 상태)

**개선**: Health HTTP server 추가 + 3종 probe 설정.

**`main.rs`** — 별도 tokio task로 health server 실행 (port `HEALTH_PORT`, default `9091`):

```rust
struct HealthState {
    start_time: Instant,
    first_scrape_done: bool,          // readiness
    last_scrape_at: Instant,          // liveness
}

// GET /startup → 200 (process alive), 503 (pre-init)
// GET /ready   → 200 (first scrape done), 503 (not yet)
// GET /health  → 200 (last scrape < 3min ago), 503 (hung)
```

**K8s probe 설계**:

| probe | 목적 | endpoint | 판단 기준 |
|-------|------|----------|-----------|
| `startupProbe` | 초기화 완료 대기 (첫 scrape) | `GET /startup` | 프로세스 살아있으면 200 |
| `readinessProbe` | 첫 scrape 완료 확인 | `GET /ready` | `first_scrape_done = true` |
| `livenessProbe` | hung 감지 + 재시작 | `GET /health` | 마지막 scrape < 180s |

```yaml
# veronex-agent-statefulset.yaml
startupProbe:
  httpGet:
    path: /startup
    port: 9091
  failureThreshold: 12   # 12 × 5s = 60초 내 초기화 완료 못하면 재시작
  periodSeconds: 5

readinessProbe:
  httpGet:
    path: /ready
    port: 9091
  initialDelaySeconds: 5
  periodSeconds: 15
  failureThreshold: 2    # 30초 내 첫 scrape 없으면 NotReady

livenessProbe:
  httpGet:
    path: /health
    port: 9091
  initialDelaySeconds: 60  # startupProbe 이후 시작
  periodSeconds: 30
  failureThreshold: 3      # 90초 무응답 시 재시작
```

환경변수 `HEALTH_PORT` (default: `9091`) 로 포트 설정.

#### 5. OTLP 에러 body 마스킹 (LOW)

**`otlp.rs:78`**: `resp.text().await.unwrap_or_default()` — response body 읽기 실패 시 에러 원인 불명확.

**개선**: 에러 body 로깅 수준을 `debug`로 내리고 실패 시 메시지 명시:
```rust
let body = resp.text().await.unwrap_or_else(|_| "<unreadable>".into());
tracing::warn!(status = status.as_u16(), body = %body, "otlp push rejected");
```

---

## 구현 계획

### Phase 1 — 자가 메트릭 구조체 추가 (`main.rs`)

```rust
/// 에이전트 자가 관측 상태 — scrape 사이클마다 갱신.
struct AgentStats {
    start_time: std::time::Instant,
    otlp_push_errors: u64,
}
```

### Phase 2 — scrape_cycle() 반환값으로 사이클 통계 수집 (`main.rs`)

```rust
struct CycleResult {
    duration_secs: f64,
    targets_scraped: usize,
    gauges_collected: usize,
}
```

`scrape_cycle()`이 `CycleResult`를 반환하도록 변경.

### Phase 3 — 자가 메트릭을 Gauge 벡터로 변환 후 기존 OTLP push에 포함 (`main.rs`)

```rust
fn agent_self_gauges(stats: &AgentStats, cycle: &CycleResult) -> Vec<Gauge> {
    vec![
        Gauge { name: "veronex_agent_uptime_seconds".into(),
                value: stats.start_time.elapsed().as_secs_f64(), labels: vec![] },
        Gauge { name: "veronex_agent_scrape_duration_seconds".into(),
                value: cycle.duration_secs, labels: vec![] },
        Gauge { name: "veronex_agent_scrape_targets_total".into(),
                value: cycle.targets_scraped as f64, labels: vec![] },
        Gauge { name: "veronex_agent_gauges_collected_total".into(),
                value: cycle.gauges_collected as f64, labels: vec![] },
        Gauge { name: "veronex_agent_otlp_push_errors_total".into(),
                value: stats.otlp_push_errors as f64, labels: vec![] },
    ]
}
```

기존 `gauges` 벡터에 `extend`로 추가 → 기존 OTLP push 코드 그대로 사용.

### Phase 4 — OTLP 재시도 (`otlp.rs`)

`push()` 내부에 1회 재시도 추가. 재시도 간격: 5초.
`AgentStats.otlp_push_errors` 증가는 콜백 또는 반환값으로 처리.

### Phase 5 — 에러 처리 개선 (`main.rs`, `otlp.rs`)

- Target discovery JSON 파싱 실패 → `tracing::warn!` 추가
- OTLP response body 읽기 실패 → `"<unreadable>"` 폴백 명시

### Phase 6 — K8s 3종 probe + health endpoint (`main.rs`, `statefulset.yaml`)

`main.rs`에 tokio spawn으로 health HTTP server 추가 (axum minimal, port `HEALTH_PORT` default 9091).

공유 상태:
```rust
struct HealthState {
    first_scrape_done: AtomicBool,
    last_scrape_at: Mutex<Instant>,
}
```

- `first_scrape_done` → scrape cycle 첫 완료 시 `true` 설정
- `last_scrape_at` → 매 cycle 완료 시 갱신

엔드포인트:
- `GET /startup` → 프로세스 살아있으면 200 (startupProbe용)
- `GET /ready` → `first_scrape_done` 이면 200, 아니면 503
- `GET /health` → `last_scrape_at` 경과 < 180s 이면 200, 이상이면 503

`veronex-agent-statefulset.yaml`에 3종 probe 추가.
`values.yaml`에 `veronexAgent.healthPort: 9091` 추가.

변경 파일:
- `crates/veronex-agent/src/main.rs`
- `deploy/helm/veronex/templates/veronex-agent-statefulset.yaml`
- `deploy/helm/veronex/values.yaml`

### Phase 7 — 테스트 업데이트

- `scrape_cycle()`이 `CycleResult` 반환 → 기존 테스트 시그니처 업데이트
- `agent_self_gauges()` 단위 테스트: 반환 메트릭 이름/수 검증
- OTLP 재시도: mock server로 첫 요청 실패 → 두 번째 성공 검증
- `/health`: 정상 시 200, 180초 초과 시 503 검증

---

## 변경 파일 목록

| 파일 | 변경 내용 |
|------|-----------|
| `main.rs` | health server, `AgentStats`, `CycleResult`, `agent_self_gauges()`, target discovery warn |
| `otlp.rs` | 1회 재시도, response body 에러 명시 |
| `scraper.rs` | 변경 없음 |
| `shard.rs` | 변경 없음 |
| `veronex-agent-statefulset.yaml` | `livenessProbe` 추가 |
| `values.yaml` | `agent.healthPort: 9091` 추가 |

---

## 변경하지 않는 것

- Redpanda 설정, 토픽, retention
- OTel Collector 설정
- ClickHouse 스키마
- 기존 메트릭 allowlist (node-exporter, Ollama)
- CPU mode 필터 (user/system/iowait/idle)
- Scrape interval default (30초)

---

## Tasks

| # | Task | 파일 | Status |
|---|------|------|--------|
| 1 | `HealthState` 공유 구조체 + health HTTP server (`/startup`, `/ready`, `/health`) | `main.rs` | pending |
| 2 | `startupProbe` (GET /startup, 12×5s=60s) | `veronex-agent-statefulset.yaml` | pending |
| 3 | `readinessProbe` (GET /ready, 15s period, 2× fail) | `veronex-agent-statefulset.yaml` | pending |
| 4 | `livenessProbe` (GET /health, 30s period, 3× fail) | `veronex-agent-statefulset.yaml` | pending |
| 5 | `veronexAgent.healthPort: 9091` values 추가 | `values.yaml` | pending |
| 4 | `AgentStats`, `CycleResult` 구조체 추가 | `main.rs` | pending |
| 5 | `scrape_cycle()` → `CycleResult` 반환하도록 변경 | `main.rs` | pending |
| 6 | `agent_self_gauges()` 구현 + gauges에 extend | `main.rs` | pending |
| 7 | OTLP push 1회 재시도 추가 | `otlp.rs` | pending |
| 8 | OTLP response body 에러 처리 개선 | `otlp.rs` | pending |
| 9 | Target discovery JSON 파싱 실패 시 warn 추가 | `main.rs` | pending |
| 10 | `/startup`, `/ready`, `/health` 단위 테스트 | `main.rs` | pending |
| 11 | `agent_self_gauges()` 단위 테스트 | `main.rs` | pending |
| 12 | 기존 테스트 시그니처 업데이트 | `main.rs` | pending |
| 13 | **Graceful Degradation regression test**: node-exporter 없을 때 `ThrottleLevel::Normal` 반환 검증 | `thermal.rs` | pending |
| 14 | **Graceful Degradation regression test**: Valkey cache miss 시 static VRAM 폴백 검증 | `provider_router.rs` | pending |
