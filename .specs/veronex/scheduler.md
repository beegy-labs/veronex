# Intelligence Serving — Scheduler SDD

> **Status**: Final Design | **Last Updated**: 2026-03-11
> **Target**: AMD Ryzen AI Max+ 395 (APU, 통합메모리) · k8s-worker-ai-01 (RAM 124GB)

---

## 목표

**Veronex = N개 Ollama 서버를 통합하는 Intelligence Gateway**

단순 리버스 프록시가 아니다. 서버 집합 전체를 하나의 컴퓨트 풀로 취급하고
자율적으로 최적 동작을 학습·결정한다.

```
클라이언트
    ▼
[Veronex Gateway]  ← K8s multi-replica
    ├──► Ollama A  (k8s-worker-ai-01, 124GB)
    ├──► Ollama B  (추후 확장)
    └──► Ollama N
```

**레벨 1 — 단일 서버 극한 활용**

각 Ollama 서버의 모델 할당과 동시 처리량을 극한까지 끌어올린다.

- 서버마다 VRAM·num_parallel이 다름 (비균등). AIMD가 서버×모델 쌍별 독립 학습.
- APU(AMD Ryzen AI Max+ 395): 통합 메모리 124GB, 대역폭 256GB/s가 병목.
  DRM VRAM 1GB 오보고 → node-exporter 실측값으로 대체.
- 모델을 VRAM에 상주시켜 재로드 없이 연속 처리. Lazy Eviction으로 필요할 때만 회수.
- 큐: FIFO + Locality(로드 모델 우선) + Age(오래 기다린 요청 역전) 복합 스케줄링.

**레벨 2 — 클러스터 처리율 최대화 + 전력 최적화**

서버 집합 전체의 처리율(Goodput)을 최대화하되, 자원 낭비를 최소화한다.

- 자원 최적 활용 1순위: 처리량 충분 → 최소 서버 수 운용. 부족 → Scale-Out. 남음 → 즉시 회수.
- 전력 효율화: 유휴 서버 모델을 Lazy Eviction으로 자연 회수 → Ollama 메모리 해제.
  서버 OS 종료 없이 idle 상태를 실질 저전력 수단으로 활용.
  (모델 미상주 ~5W vs 상주 ~70W)

**레벨 3 — 하드웨어 보호 (Thermal)**

하드웨어를 한계까지 쓰되, 온도 위험 구간 진입 시 자동 감쇄/차단으로 서버를 보호한다.

- perf_factor: 온도 비례 스케줄링 감쇄 → 과열 서버에서 모델 전환 억제.
- Soft Gate(82°C): 신규 요청 차단 + Preload/Scale-Out 금지.
- Hard Gate(90°C): 전면 차단 → Cooldown 300s → Ramp-up → Normal 점진 복귀.
- 임계값: 벤더별 기본값 + 어드민 provider별 오버라이드.

**추가 설계 목표**

- 크래시 복구: Lua atomic handoff로 job 유실 방지. at-least-once + idempotent.
- 모델 Pull 드레인: pull 중 자동 요청 차단 → drain → pull → 자동 재로드.
- 멀티인스턴스 안전: Lua ZREM 원자적 선점. 복수 인스턴스 중복 처리 방지.
- 단일 서버 환경(현재 실측: k8s-worker-ai-01 1대): Scale-Out은 no-op.
  멀티 서버 확장을 전제하되 단일 서버에서 안전하게 동작.

---

## 실측 환경 및 Ollama 설정

```
RAM 124GB · mem_available 120GB · DRM VRAM 1GB(BIOS) · GTT 128GB
모델: qwen3-coder-next 51GB · qwen3:30b 18GB · qwen3:8b 5GB · nomic-embed 274MB
```

**Ollama 권장 설정**:

```
OLLAMA_NUM_PARALLEL=<num_parallel>   ← 프로바이더 등록 시 설정값과 동일, AIMD 상한
OLLAMA_KEEP_ALIVE=-1                 ← Veronex Lazy Eviction 직접 제어
OLLAMA_MAX_LOADED_MODELS=0           ← VramPool이 VRAM 기준 제어
OLLAMA_FLASH_ATTENTION=1
OLLAMA_KV_CACHE_TYPE=q8_0
```

---

## N×N×N 구조 (서버별 비균등)

```
Gateway → N개 서버 (서버마다 VRAM · num_parallel · 지원 모델 상이)
              └─ 서버당 N개 모델 동시 상주 (VRAM 허용 범위)
                      └─ 모델당 N개 동시 요청 (num_parallel 이하, AIMD 학습)
```

AIMD는 `provider_id × model` 쌍 독립 학습. 서버 A의 qwen3:8b와 서버 B의 qwen3:8b는 별개.

**VramPool 구조**:

```
ProviderVramState (서버별)
  ├── total_mb         ← mem_available_mb × (1 - safety_permil/1000)
  │                      node-exporter 실측. APU drift 흡수용 safety_permil 적용.
  ├── reserved_kv_mb   ← 전체 KV 합산 (원자적 CAS)
  ├── safety_permil    ← 기본 100 (10%). OOM 감지 시 +50 증가, 회복 후 -10 점진 감소.
  │                      최대 500 (50%). OOM = try_reserve 실패 or Ollama 429 응답.
  └── is_standby       ← AtomicBool (Scale-In 시 라우팅 제외)

ModelState (모델별)
  ├── weight_mb / kv_per_request_mb
  ├── is_loaded / is_preloading    ← AtomicBool (중복 Preload 방지)
  ├── active_count / max_concurrent ← AIMD 학습값 (≤ num_parallel)
  ├── sample_count                 ← AtomicU32 (evict 시 0 리셋)
  ├── baseline_tps / baseline_p95_ms
  └── last_active_at               ← AtomicU64 (Lazy Eviction 기준)
```

**비용 규칙**: 로드됨 → KV only / 미로드 → weight+KV / 완료 → KV 반환, weight 상주.

**APU mem_available_mb 오차 처리**: Ollama 외 프로세스 메모리 소비로 인한 drift는
`safety_permil`(기본 10%)이 흡수. 30s sync 루프에서 node-exporter 재측정 후 갱신.

---

## 핵심 메커니즘

### 1. AIMD — 서버×모델별 최적 동시 요청 수 자율 학습

APU는 메모리 대역폭(256 GB/s)이 병목. AIMD가 포화점을 자동 탐색.

| 단계 | 조건 | max_concurrent |
|------|------|---------------|
| Cold Start | sample = 0 | `num_parallel` (상한에서 시작 → 하향 탐색) |
| AIMD | sample ≥ 3 | TPS ratio < 0.7 or p95 spike → max(1, current×3/4) 즉시 적용 / ratio ≥ 0.9 → +1 |
| LLM 보정 | sample ≥ 10, AIMD 적용 후 | **증가 방향에만** 최대 +2 후처리 |
| 재시작 | DB 복구 | 직전 학습값 즉시 적용 |

> **Cold Start 정책 변경** (기존 `capacity.md` cold_start=1과 다름):
> APU에서 메모리 안전은 try_reserve + safety_permil이 독립적으로 담당하므로
> 초기값을 1로 보수적으로 시작할 필요가 없다. num_parallel에서 시작해 AIMD가
> 빠르게 하향 조정하는 방식이 초기 처리량 확보에 유리하다.
> Phase 9에서 capacity.md를 이 정책으로 업데이트한다.

피드백: 30s마다 ClickHouse `inference_events` (`learning_epoch_started_at` 이후, 최대 1h) 기준. 결과 DB 저장.
  이유: evict→재로드 후 이전 환경의 측정치가 섞이면 Cold Start 재학습이 무효화됨.
        learning_epoch_started_at 이후 데이터만 집계해야 sample_count=0 리셋이 실제로 의미를 가짐.
  ClickHouse 쿼리 타임아웃: ClickHouse 응답 지연이 30s를 초과하면 AIMD 루프 전체가 블로킹됨.
    쿼리 타임아웃 = 10s. 초과 시 해당 사이클 AIMD 업데이트 스킵 (이전 max_concurrent 유지).
    연속 3회 타임아웃 시 경고 로그. ClickHouse 장애가 AIMD를 멈추게 하지 않음.
  외부 메모리 환경 급변 시: 30s sync 루프에서 mem_available_mb가 이전 값 대비 ≥15% 감소하면
    모든 ModelState의 sample_count=0 + learning_epoch_started_at=now_ms 리셋.
    이유: evict/pull 없이도 외부 프로세스가 메모리를 20GB+ 소비하면 이전 baseline이 무의미해짐.
    15% = 120GB × 15% = 18GB 임계값 (qwen3:8b 모델 1개 수준, 의미있는 환경 변화 기준).
LLM 보정은 AIMD 계산 완료 후 동일 30s 루프 내 후처리로 적용한다. 순서: AIMD → LLM 보정.
LLM 보정은 증가 방향에만 적용하며, 감소 방향 판단은 AIMD가 전담한다.
같은 루프에서 AIMD 감쇄(×3/4)가 발생한 경우 LLM 상향 보정은 금지한다.
즉, AIMD 감쇄 결과가 그 사이클의 최종값이다.

**baseline_tps / baseline_p95_ms 갱신 규칙**:

"첫 안정 측정값" 정의: `sample_count ≥ 3` 이 되는 시점 (AIMD 최초 활성화 기준과 동일)

| 이벤트 | baseline_tps | baseline_p95_ms |
|--------|-------------|-----------------|
| 초기화 (sample_count ≥ 3 첫 루프) | `current_tps` 로 설정 | `current_p95_ms` 로 설정 |
| 감쇄 발생한 사이클 | freeze (갱신 안 함) | freeze (갱신 안 함) |
| `ratio ≥ 0.9` 3회 연속 (안정 확인) | `current_tps` 로 상향 | `current_p95_ms` 로 하향 (latency 개선 반영) |
| evict → `sample_count = 0` | `0` 초기화 | `0` 초기화 |

p95 spike 감쇄 조건: `current_p95_ms > baseline_p95_ms × 2`
  → baseline_p95_ms = 0 이면 (초기화 전) p95 spike 조건 비활성화, TPS ratio만으로 판단.

**APU 특수**: DRM 1GB → 모델(5~51GB) 항상 초과 → VRAM 게이트 우회.
역할 분리: AIMD = 처리량 최적화 / try_reserve + safety_permil = 메모리 안전. 두 경로 독립 작동.
OOM (Ollama 429 or try_reserve 실패) 시:
  - safety_permil +50 → 가용 메모리 상한 즉시 축소
  - 해당 model×provider max_concurrent 즉시 max(1, current×3/4) (AIMD 감쇄 규칙 즉시 적용)
    **하한 = 1 필수**: u32 정수 반복 감쇄 시 4→3→2→1→0 도달 가능.
    max_concurrent=0이면 요청 수용 불가 → sample 미수집 → AIMD 증가 조건 불충족 → 복구 불가 deadlock.
  - 이후 30s AIMD 루프에서 정상 학습 재개
  **OOM 회복 불균형은 의도된 보수적 설계**:
    safety_permil 회복: -10/30s → +50 복구에 150s. max_concurrent 회복: +1/30s.
    두 경로 속도 불균형으로 150s간 저활용 구간 발생. 이는 의도됨.
    OOM은 서비스 전체 중단으로 이어지므로, 빠른 회복보다 안전 우선이 합리적.

### 2. 스케줄링 — FIFO + Locality + Age

**큐**: Valkey ZSET. enqueue score = `now_ms - tier_bonus` (고정). dispatch 시 보정.

**최대 큐 사이즈**: `MAX_QUEUE_SIZE = 10_000`. ZCARD 초과 시 enqueue 거부 → 429 Too Many Requests.
  이유: 무제한 ZSET 성장 방지. Valkey 메모리 보호 + K=100 scoring 비용 상한.
  **원자화 필수**: 비원자 ZCARD 체크 후 ZADD 사이에 다른 writer가 끼어들면 10,000 초과 가능(overshoot).
  **모델/티어별 독점 방지**: 전역 상한만 두면 hot model 또는 paid tier가 10,000 슬롯을 독점해
    다른 모델/티어의 신규 요청이 전면 차단됨.
    per-model 상한: `MAX_QUEUE_PER_MODEL = 2_000` (demand_counter가 이미 모델별 집계 제공).
    enqueue Lua에서 demand:{model} ≥ MAX_QUEUE_PER_MODEL 이면 해당 모델만 429 반환.
    (전역 ZCARD 체크 AND per-model demand 체크 — 둘 중 하나라도 초과 시 거부)
  구현: Lua 단일 스크립트 — ZCARD 체크 + ZADD + INCR demand를 원자 실행.
  ```lua
  -- Lua enqueue: 원자 ZCARD 체크 + ZADD + demand INCR + side hash 기록
  -- KEYS[1]=queue:zset  KEYS[2]=demand:{model}  KEYS[3]=queue:enqueue_at  KEYS[4]=queue:model
  -- ARGV[1]=job_id  ARGV[2]=score  ARGV[3]=max_size  ARGV[4]=now_ms  ARGV[5]=model
  if redis.call('ZCARD', KEYS[1]) >= tonumber(ARGV[3]) then return 0 end  -- 429
  redis.call('ZADD', KEYS[1], ARGV[2], ARGV[1])
  redis.call('INCR', KEYS[2])
  redis.call('HSET', KEYS[3], ARGV[1], ARGV[4])  -- enqueue_at 저장 (promote_overdue 역산용)
  redis.call('HSET', KEYS[4], ARGV[1], ARGV[5])  -- model 저장 (demand_resync 역산용)
  return 1
  ```
  dispatch Lua handoff, queued cancel Lua에도 `HDEL veronex:queue:enqueue_at {job_id}` 추가.

**job→model 매핑 side hash** (demand_resync 용):
  enqueue Lua 스크립트에 추가: `HSET veronex:queue:model {job_id} {model}`
  dispatch/cancel Lua에 추가: `HDEL veronex:queue:model {job_id}`
  이유: demand_resync_loop(60s)는 ZSET member(job_id)로부터 model을 역산해야 하나
        ZSET member에 model 정보가 없음. side hash로 해결.
        promote_overdue_loop도 이 hash에서 model 확인 가능.

**Tier 우선순위 (대기 250s 이내 절대적, 이후 공정 경쟁)**:
```
TIER_BONUS_PAID     = 300,000ms
TIER_BONUS_STANDARD = 100,000ms
TIER_BONUS_TEST     = 0ms
TIER_EXPIRE_SECS    = 250s   ← 이 시간 초과 시 tier_bonus 무효화
```
대기 250s 이내: paid가 standard보다 항상 먼저 처리 (200,000ms 격차, 역전 불가).
대기 250s 초과: tier_bonus 무효화 + EMERGENCY_BONUS 적용 → **장기 대기 요청이 신규 paid보다 우선**.
  보장 정밀도: promote_overdue 루프 주기 30s로 인해 실제 승격 시점은 250s~280s 구간 어딘가.
  "250s 이후 반드시 즉시 앞선다"가 아니라 "최대 280s 이내 반드시 앞서게 됨"이 정확한 계약.

```
EMERGENCY_BONUS = TIER_BONUS_PAID = 300,000ms

final_score = zset_score                                ← raw ZSET score 그대로 사용
            - locality_bonus  (모델 로드됨: 20,000ms / 미로드: 0)
            - age_bonus       (wait_ms × 0.25 × perf_factor(temp_c))
낮을수록 먼저 처리.
```

**EMERGENCY_BONUS 적용 경로 — promote_overdue 단일 책임**:
  dispatch의 final_score 계산에서는 EMERGENCY_BONUS를 적용하지 않는다.
  EMERGENCY_BONUS는 `promote_overdue` 루프(30s)에서 ZADD XX로 ZSET score를 직접 갱신하는
  방식으로만 적용된다. dispatch는 갱신된 raw score를 신뢰한다.
  이유: dispatch에서도 EMERGENCY_BONUS를 빼면 promote_overdue와 이중 차감됨.
        단일 책임으로 분리해야 보정 로직이 한 곳에만 존재.

tier_bonus 제거만으로 부족한 이유: paid가 지속 유입되면 250s를 넘긴 standard도
  원래 score = T_old - 100,000 이고, 신규 paid score = T_new - 300,000.
  T_new = T_old + 250,000 이면 신규 paid score = T_old - 50,000.
  ZSET은 낮은 score가 우선이므로: standard(T_old - 100,000) < paid(T_old - 50,000) → standard가 이김.
  // ↑ 단순 tier_bonus 제거("overdue score = T_old") 방식에서는:
  //   overdue standard score = T_old, 신규 paid score = T_old - 50,000
  //   T_old - 50,000 < T_old → paid가 이김 — 기아가 해소되지 않음.
  // 따라서 tier_bonus를 단순 제거하는 것이 아닌 EMERGENCY_BONUS를 더해야 함:
  //   overdue standard score = T_old - 300,000 → paid(T_old - 50,000)보다 낮아 standard 우선.
promote_overdue가 overdue standard의 score를 enqueue_at - EMERGENCY_BONUS로 갱신하면:
  overdue standard score = T_old - 300,000 → 신규 paid(T_old - 50,000)보다 확실히 낮음 → standard 우선.

검증 예시 (T_old = standard enqueue 시각, T_now = T_old + 251,000):
  promote_overdue 후 standard score: T_old - 300,000
  신규 paid (wait=1s):     final = (T_now - 300,000) - locality - age ≈ T_now - 300,250
  standard (wait=251s):    final = (T_old - 300,000) - locality - age
                           = (T_now - 251,000 - 300,000) - 62,750  = T_now - 613,750  ✅ standard 우선
  paid (wait=251s):        promote_overdue 후 score = T_old - 300,000 → standard와 동일 score  ✅ fair race

**의도**: 250s 이상 대기한 요청은 tier와 무관하게 신규 요청보다 반드시 앞선다.
  paid 연속 유입 환경에서도 standard 기아가 발생하지 않는다.

**EMERGENCY_BONUS top-K 진입 보장**:
  dispatch의 final_score는 raw ZSET score를 사용하므로, overdue job의 ZSET score를
  직접 갱신해 top-K 안으로 끌어올려야 한다.
  과부하 환경에서 paid가 K=20~100 슬롯을 채우면 overdue standard는
  score가 높아 top-K 진입 자체가 불가하기 때문이다.

  해결: `promote_overdue` 패스 — 별도 30s 루프에서 **enqueue_at 기반 전체 커서 스캔**:
    1. HSCAN veronex:queue:enqueue_at CURSOR COUNT 200 → (job_id, enqueue_at_ms) 전체 순회
    2. wait_ms = now_ms - enqueue_at_ms > 250,000 인 job만 필터
    3. 해당 job의 ZSET score를 ZADD XX로 갱신:
         new_score = enqueue_at_ms - EMERGENCY_BONUS
    4. 이후 ZRANGE는 raw score 기준으로 overdue job을 자연히 top-K 안으로 선출.
  보장 범위 변화: "top-K 진입 후 standard 우선" + "top-K 진입 보장" 모두 성립.

  **ZSET score 상위 K*3 스캔이 부족한 이유**:
    ZRANGEBYSCORE LIMIT 0 {K*3}은 score가 낮은(=우선순위 높은) 상위만 본다.
    paid 수천 건이 쌓이면 overdue standard/test는 score가 높아 K*3 밖에 묻혀
    영구적으로 승격되지 않을 수 있다. enqueue_at side hash를 HSCAN으로 전체 순회하면
    score 순서와 무관하게 모든 overdue job을 감지할 수 있다.
    MAX_QUEUE_SIZE=10,000 × HSCAN COUNT 200 = 최대 50회 반복. 30s 주기에 충분히 가볍다.

  **tier 역산 문제 해결**:
  ZSET score = `now_ms - tier_bonus`이므로 score만으로는 원래 enqueue_at_ms를 알 수 없음.
  해결: enqueue 시 `HSET veronex:queue:enqueue_at {job_id} {now_ms}` side hash에 저장.
    enqueue Lua 스크립트에 HSET 추가 (원자 실행).
    promote_overdue: HSCAN veronex:queue:enqueue_at → enqueue_at_ms 획득.
    dispatch/cancel 시: `HDEL veronex:queue:enqueue_at {job_id}` 정리.
    new_score = enqueue_at_ms - EMERGENCY_BONUS  (tier_bonus 역산 불필요, enqueue_at 직접 사용)

**기아 방지**: age_bonus ≥ locality_bonus 역전 시점 → ≤75°C: 80s / 82°C: 114s.
max_queue_wait(300s) 이내에 역전 발생 보장. 미로드 모델도 ~2분 내 역전 → 모델 전환 강제.
SLA 정책: 대화형·배치 구분 없음. 모델별 차등 없음.

**perf_factor × age_bonus 설계 의도**: 온도가 높을수록 age_bonus가 줄어 모델 전환이
더 늦게 일어난다. 이는 의도된 동작이다. 과열 서버에서 모델 재로드(VRAM 재할당)는 추가
연산 부하를 유발하므로, thermal 보호 차원에서 기존 로드 모델에 더 오래 집중하는 것이 유리하다.

**멀티인스턴스 안전**: ZRANGE K=20 peek (read-only) → Rust scoring → Lua ZREM 원자적.
ZREM 반환값 0 = 다른 인스턴스가 선점 → 즉시 재시도.
K=20 윈도우 공정성: age_bonus는 top-K 후보 선정 이후 Rust scoring 단계에서 적용되므로
  K 밖의 job을 ZSET 상위로 끌어올리는 효과가 없다.
  K 밖 job의 공정성은 promote_overdue 루프(30s)가 ZSET score를 직접 갱신하는 방식으로만 보장된다.
  age_bonus 누적이 K 진입을 보장한다는 설명은 틀렸다 — promote_overdue가 단일 책임자다.
큐 적체 시 보완: ZSET 크기가 K×3(60) 초과 시 K를 동적으로 min(ZSET_size/3, 100)으로
확장. dispatcher 루프마다 ZCARD로 확인. 상한 100은 scoring 비용 제한.

**perf_factor(temp_c)**: ≤75°C → 1.0 / 82°C → 0.70 / ≥90°C → 0.0 (선형 보간, thermal.rs).

**demand_counter**: `veronex:demand:{model}` (Valkey). 의미 = **ZSET 대기열 길이(queued only)**.
- INCR: job이 ZSET에 진입할 때 (enqueue)
- DECR: dispatch 시 Lua handoff 스크립트 내 원자적 처리 (ZREM + LPUSH + DECR 단일 스크립트)
- DECR: cancel/timeout 경로 — queued cancel Lua 스크립트(ZREM + DECR)로 원자 처리 (§7 참고)
- inflight(processing) 중인 job은 카운트하지 않음
- resync: 60s마다 ZSET member 기준으로 집계 후 덮어씀 → INCR/DECR drift 자동 보정
          (ZSET이 단일 진실 소스: ZSCAN → HMGET queue:model → 집계. side hash stale entry 자동 제외)

**원자성 범위 명세**:
- enqueue: `ZCARD` + `ZADD queue:zset` + `INCR demand:{model}` — **Lua 단일 스크립트 (원자)**
  이유: 비원자 ZCARD → ZADD 사이에 다른 writer가 끼어들면 MAX_QUEUE_SIZE overshoot 가능.
        Lua로 묶어 hard cap 보장 (반환 0 = 큐 포화 → 429).
- dispatch: `ZREM + LPUSH + DECR` — **Lua 단일 스크립트 (원자)**
- cancel/timeout: `ZREM + DECR` — **Lua 단일 스크립트 (원자)**

**drift 안전성 근거**:
- DECR 단독 실패 불가: DECR은 항상 Lua 스크립트 내에서 ZREM과 함께 실행됨
- enqueue drift 제거됨: ZCARD + ZADD + INCR이 Lua 단일 스크립트이므로
  "ZADD 성공 후 INCR 전 크래시" 시나리오가 원천 차단됨
- resync 존재 이유: Lua 외부 예외(Valkey 재시작, 운영자 수동 ZSET 조작 등) 방어 차원.
  60s resync가 ZSCAN(ZSET 단일 진실 소스) 기반으로 demand_counter를 재산정해 어떠한 예외적 drift도 자동 보정.
  side hash(queue:model) 단독 HSCAN 사용 금지 — stale entry로 인한 과대복구 발생.
- 결론: enqueue drift 경로 없음. dispatch/cancel은 Lua 원자. 60s resync가 최종 방어선.
  영구 drift 불가.

### 3. Thermal 보호 — 요청 차단 및 복구

**임계값 기준**: AMD Ryzen AI 395+ APU 공식 junction temp 한계(105°C) 기준으로
운영 안전 마진 확보. 75°C(정상)/82°C(경고)/90°C(위험) 3구간.
어드민이 provider별로 오버라이드 가능. 벤더별 기본값:
- AMD APU (Ryzen AI Max+ 395): normal_below=75 / soft_at=82 / hard_at=90 / cooldown_secs=300
- NVIDIA GPU: normal_below=80 / soft_at=88 / hard_at=93 / cooldown_secs=300
- unknown: AMD APU 기본값 적용

**cooldown_secs=300 근거**: GPU/APU thermal throttling은 소프트웨어가 부하를 멈춰도 하드웨어
클럭 복구 + 센서 안정화에 수분이 필요하다. 60s로 재개하면 "부하 재개 → 즉시 throttling"
무한 루프 확률이 높다. 300s(5분)이 APU 실제 쿨다운에 충분한 최소값. 어드민 오버라이드 가능.

health_checker 30s 루프 → node-exporter 스크랩 → `thermal.update(temp_c)` → 상태 갱신.
dispatcher `score_and_claim()` 호출 시 현재 thermal 상태 읽기 (atomic load).

**Soft Gate (≥ soft_at, 82°C)**:
```
신규 요청: 차단 (503)
진행 중인 요청: 완료까지 허용
해제 조건(Hysteresis): temp < 80°C AND provider_total_active == 0 일 때 Normal 복귀
  // provider_total_active = Σ active_count(model, provider) for all loaded models on this provider
  // active_count는 ModelState 단위(model+provider 쌍)이므로, provider-wide 합산이 해제 조건이다.
  // model-wide active_count 단독이 아님 — 해당 provider의 모든 모델 in-flight 종료가 조건.
의도: 82°C 경계에서 요청 1개 단위로 Gate가 여닫히는 진동(Oscillation) 방지.
      provider_total_active == 0 단독으로는 해제되지 않음 — 온도가 80°C 이하로 내려가야 재개.
해제 체크 주기: health_checker 30s 루프에서만 수행. 이벤트 드리븐 즉시 해제 없음.
  (provider_total_active==0 이벤트 시점에도 온도 재체크는 다음 30s 루프까지 대기 — 보수적 의도)
장기 스트림 고착 방지: SSE_HARD_TIMEOUT_SECS = 600 (상수).
  이 값은 dispatcher.rs 및 runner.rs에서 강제 종료 기준으로 사용됨.
  Soft Gate 진입 후 600s 이내에 모든 in-flight 스트림이 종료됨 — 무기한 고착 불가.
  // 이 보장은 scheduler.md가 단독으로 완결한다. distributed.md 참조 없이도 성립.
  // SSE_HARD_TIMEOUT_SECS 변경 시 이 보장이 깨지므로 두 문서 동기화 필수.
```

**Hard Gate (≥ hard_at, 90°C)**:
```
신규 요청: 모두 차단 (503)
진행 중인 요청: 완료까지 허용 (단, 최대 60s drain 상한)
  이유: 무기한 drain은 실질 쿨다운 시간을 보장하지 못함.
        긴 SSE가 200초 더 돌면 cooldown 300s 중 100초만 실제 냉각됨 → 300s 근거 무효.
// 용어 정의 — 혼동 방지:
//   forced_drain_timeout = 60s  : Hard 진입 후 in-flight을 강제 종료하기까지의 최대 대기 시간.
//                                  cooldown 기간(300s)과 무관. drain이 빠르면 0s에 완료될 수도 있음.
//   cooldown_secs = 300s        : Cooldown 상태 지속 시간. 실제 하드웨어 냉각 시간.
//                                  기존 코드 7,200s → 300s로 변경 (근거: L322-324).
Cooldown timer 시작 — 단일 정의:
  timer_start_at = first_time_provider_total_active_reaches_0
  // = Hard 진입 후 provider_total_active(모든 모델 active_count 합)가 0이 되는 시점.
  // VramPermit drop(단계 5 완료)이 active_count를 감소시키므로, 실제 하드웨어 부하 종료 후 시작.
  // forced drain(60s 상한)으로 인해 Hard 진입 후 최대 60s + cancel→VramPermit drop 지연(수초) 이내.
  // "min(hard_entered_at+60s, active==0)" 방식은 VramPermit drop 전(단계 5 이전)에 timer를 시작해
  // 하드웨어가 아직 부하 중인 상태에서 cooldown이 카운트다운됨 — 사용 금지.
  watchdog: Hard 진입 후 90s(=60s drain + 30s 버퍼)가 지나도 active>0이면 오류 로그 기록 후
            timer_start_at = hard_entered_at + 90s 강제 설정 (블로킹 방지).
  이유: cancel() 완료를 무기한 대기하면 안 되므로 90s watchdog이 최종 보장.
추가 dispatch: Hard 진입 후 없음. drain 중 완료된 요청은 후속 dispatch 없음.

[Hard Gate forced drain cancel — 60s 상한 초과 시 강제 중단 계약]
  트리거: Hard 진입 후 60s 경과 && active_count > 0 (drain 미완료)
  처리 (§7 processing cancel 경로와 동일):
    1. 각 in-flight job의 Job Runner에 cancel 신호 전송
    2. SSE error event 전송 (스트림이 아직 열려 있을 때):
         data: {"error":{"type":"thermal_hard_gate","message":"server temperature critical"}}\n\n
    3. LREM processing {job_id}
    4. DB job status = "failed", failure_reason = "thermal_hard_gate"
    5. VramPermit drop → KV 반환, active_count 감소
  순서 보장: 1→2→3→4→5.
  // Cooldown timer 시작점: 단계 5 완료(VramPermit drop → active_count 감소) 후
  // provider_total_active == 0이 되는 시점. 90s watchdog이 최종 보장 (위 단일 정의 참고).
  // 강제 drain cancel 발동(60s) 후 단계 5가 완료되어야 active==0이 확정됨.
  // "cancel 발동 시점(60s)"으로 timer를 시작하면 하드웨어가 아직 연산 중 — 사용 금지.
  중복 terminal 방지: CancelOnDrop과 동일 메커니즘 — cancel() 후 runner가 DB 상태를 이미 썼으면 스킵.
  VramPermit drop 타이밍 계약:
    cancel() 신호 발송 → Job Runner SSE 루프 중단 → 단계 2~4 완료 → 단계 5 VramPermit drop.
    Ollama는 RST_STREAM(HTTP/2) 또는 연결 끊김 이벤트로 KV 슬롯 해제를 시작.
    Veronex의 VramPermit drop은 Ollama 내부 해제 완료를 확인하지 않는다.
    (try_reserve 기반 소프트 예약이므로 Ollama 429 재발 시 AIMD/OOM 경로로 자연 보정됨)
```

**Thermal 상태 머신**:
```
Normal ──[≥soft_at]──► Soft ──[≥hard_at]──► Hard ──[active==0 OR 60s drain]──► Cooldown
  ▲                     │                     ▲                                       │
  └─────[<80°C/hyst]────┘                     │               cooldown_secs 경과      │
                                               │                    → RampUp 진입 ────┘
RampUp (별도 상태):
  - **신규 요청 수용** (차단 없음). max_concurrent=1 상한만 적용.
    Soft(503 차단)와 다름. Normal 복귀 전 점진적 서빙 재개 단계.
  - max_concurrent = 1 강제
  - 30s마다 온도 체크 후 분기:
      temp < soft_at              → AIMD +1 재개 (계속 RampUp)
      soft_at ≤ temp < hard_at   → Soft 전이 (신규 요청 차단 재개)
      temp ≥ hard_at             → Hard 전이 (즉시 차단 + Cooldown 재진입)
  - provider 전체 복귀 조건 → Normal 완전 복귀:
      temp < normal_below
      AND Σ max_concurrent(model, provider) for all loaded models ≥ provider_pre_hard_total
    // provider_pre_hard_total: Hard 진입 직전 provider_total_committed_parallel 스냅샷 (ProviderVramState에 저장).
    // per-model pre_hard_max_concurrent 기준이 아닌 provider-wide 합산 기준.
    // 이유: thermal은 provider 전체 온도이므로 복귀 조건도 provider-wide여야 일관됨.
    //   model 1개가 pre_hard에 도달해도 다른 모델이 아직 1이면 실제 부하는 pre_hard 수준 미도달.
    // RampUp → Hard 재전이: 이미 저장된 provider_pre_hard_total 유지 (재정의 안 함).
    //   RampUp 중 Hard 재진입 시 RampUp의 reduced 상태가 덮어써지지 않아야 함.
    // pre_hard_max_concurrent(per-model): RampUp 진행 표시용으로 유지. provider-wide 조건의 보조.

Cooldown 중 온도 재상승:
  temp ≥ hard_at → Cooldown timer 리셋 (cooldown_secs 재시작)
  Cooldown 종료 조건: cooldown_secs 경과 AND temp < soft_at
    (종료 시 온도가 여전히 soft_at 이상이면 Cooldown 유지)
  **Cooldown 최대 대기 상한**: 최초 Cooldown 진입 시각으로부터 cooldown_secs × 3 경과 (기본 900s = 15분)
    timer reset과 무관한 절대 상한. cooldown_entered_at는 Hard→Cooldown 전이 시 1회 기록, reset 시 갱신 안 함.
    상한 도달 시: 전이 직전 온도 체크 후 분기
      temp ≥ hard_at   → Hard 재진입 (Cooldown timer 리셋, RampUp 진입 안 함)
      soft_at ≤ temp < hard_at → Soft 전이 (신규 요청 차단 재개)
      temp < soft_at   → RampUp 전이
    이유: 외부 열원(다른 워크로드)으로 temp가 82~89°C에 고정되면 Cooldown이 무기한 연장됨.
          상한 도달 시 무조건 RampUp 진입하면 hard_at 이상에서 최대 30s간 신규 요청이
          수용되는 안전 공백이 발생. 온도 체크 후 분기로 이를 차단.
          어드민 알림 로그 기록.
```

**전이 완전성 보장** (모든 경로 정의):
```
Normal    → Soft      : temp ≥ soft_at
Soft      → Hard      : temp ≥ hard_at
Soft      → Normal    : temp < 80°C AND provider_total_active == 0 (hysteresis)
Hard      → Cooldown  : provider_total_active == 0 (Hard 진입 후 최대 60s forced drain, 90s watchdog 최종 보장)
Cooldown  → Cooldown  : temp ≥ hard_at (timer 리셋) 또는 아직 temp ≥ soft_at (대기)
Cooldown  → RampUp    : cooldown_secs 경과 AND temp < soft_at
Cooldown  → Hard      : cooldown_secs × 3 경과 AND temp ≥ hard_at (Hard 재진입)
Cooldown  → Soft      : cooldown_secs × 3 경과 AND soft_at ≤ temp < hard_at
Cooldown  → RampUp    : cooldown_secs × 3 경과 AND temp < soft_at
RampUp    → RampUp    : temp < soft_at, AIMD 학습 진행 중
RampUp    → Soft      : soft_at ≤ temp < hard_at
RampUp    → Hard      : temp ≥ hard_at
RampUp    → Normal    : AIMD current ≥ pre_hard_max_concurrent AND temp < normal_below
```

**Circuit Breaker vs Thermal Gate 우선순위**:
```
Circuit Breaker: provider 응답 없음/타임아웃 → 해당 provider 전체 차단
Thermal Gate:    온도 초과 → 해당 provider 신규 요청 차단
동시 활성화: Circuit Breaker 우선 (더 강한 차단). CB 해제 후 Thermal 상태 재평가.
해제 순서: Thermal cooldown 완료 → CB 반개(half-open) → 탐색 요청 성공 시 CB 해제.
```

**전 provider 불능 시 동작 — 두 경로 단일 정의**:
```
// 경로 A (pre-handoff): ZRANGE peek 단계에서 filter_candidates()=0 감지
//   → dispatch 사이클 전체 스킵. ZREM 미수행. 모든 job 큐에 보존.
//   → 다음 루프까지 대기 (QUEUE_POLL_INTERVAL).
//   이유: HOL blocking 방지. 큐 front를 소모하면 다음 사이클도 동일 상태에서 연속 소모·실패.
//   클라이언트 처리: max_queue_wait(300s) 이내 provider 복구 → 정상 dispatch.
//                    복구 안 됨 → max_queue_wait 초과 → queued cancel 경로(§7) → SSE error event.

// 경로 B (post-handoff 예외): score_and_claim() 내부에서 provider 상태 변경으로 eligible=0
//   → job은 이미 processing 상태. 즉시 아래 처리 시퀀스 실행.
//   이유: ZREM 완료 후 큐에 돌려놓을 수 없음. processing 상태 job은 즉시 종결해야 함.

dispatch 흐름:
  1. ZRANGE peek → top-K 후보 수집
  2. filter_candidates() 호출 — eligible provider가 0개이면:
       경로 A: 사이클 전체 스킵 (ZREM 미수행). QUEUE_POLL_INTERVAL 후 재시도.
  3. eligible provider가 존재하면 Rust window scoring → best job 선정 → Lua handoff
  4. score_and_claim() — 이 단계에서 eligible=0 감지 시 경로 B 실행

경로 B 처리 시퀀스 (케이스별 failure_reason만 다름):
  케이스 A: 전부 Hard gate / Circuit Breaker / is_pulling → failure_reason="no_eligible_provider"
  케이스 B: 전부 Soft gate (또는 혼합) → failure_reason="all_providers_soft_gated"

  1. LREM processing {job_id}   ← processing 리스트 정리 (ZREM 완료 상태이므로 필수)
  2. DB status="failed", failure_reason 기록
  3. 클라이언트 응답 (HTTP 응답 시작 여부에 따라 분기):
       [아직 200 OK 미전송] → 503 + Retry-After 헤더 반환
       [이미 SSE heartbeat 전송됨 (200 OK + SSE 헤더)] → SSE error event 전송 후 스트림 종료:
         data: {"error":{"type":"no_eligible_provider","message":"...","retry_after_secs":N}}\n\n
       이유: HTTP 응답이 시작된 후에는 상태 코드를 변경할 수 없음.

Retry-After 계산 규칙 (경로 B에만 적용, 구현 단일 정의):
  Hard gate: max(0, cooldown_secs - elapsed_cooldown_secs). cooldown 미진입이면 cooldown_secs.
  Circuit Breaker: CB half-open 대기 시간 (CB 구현이 제공하는 next_attempt_at - now).
  is_pulling: max_pull_secs 기본값 사용 (남은 시간 미예측). 기본 300s.
  Soft gate: health_checker 30s 루프 주기 기준. 기본 30s.
  복수 상태 혼합: 위 값 중 최솟값.
  알 수 없는 경우: 기본 60s.
```

### 4. 3-State 모델 생명주기

```
IDLE ──[demand>0 + Preloader]──► COLD START ──[로드 완료]──► STEADY STATE
 ▲                                                                 │
 └──────────────── Lazy Eviction ─────────────────────────────────┘
      ① 다른 모델 VRAM 필요 (APU: ②③만으로 발화)
      ② active_requests == 0
      ③ idle ≥ 180s  (is_standby=true 서버: idle ≥ 30s로 단축 — 전력 최적화 우선)
      → evict 시: sample_count = 0, learning_epoch_started_at = now_ms
               (재로드 시 Cold Start 재시작 + 새 epoch 기준으로 ClickHouse 집계 시작)
```

Preloader: POST `/api/generate` `num_predict=0`. is_preloading 플래그로 중복 방지.

**Preload 실패 처리**:
```
120s 타임아웃 또는 오류 → is_preloading=false → 다음 5s 루프 재시도
클라이언트 타임아웃 방어:
  - 큐에 쌓인 요청은 대기 중 SSE heartbeat("data: \n\n") 30s마다 전송 (연결 유지)
  - max_queue_wait = 300s. 초과 시 job → failed, 클라이언트 응답:
      SSE heartbeat 전송 전: 503 반환
      SSE heartbeat 전송 후: SSE error event 전송 후 스트림 종료
        data: {"error":{"type":"timeout","message":"queue wait exceeded"}}\n\n
  - Preload 3회 연속 실패 시: 해당 **model+provider 조합**만 300s 제외
      다른 healthy provider가 있으면 → 해당 provider로 계속 라우팅
      모든 provider가 제외될 때만 → 해당 모델 요청에 503 반환
      이유: "해당 모델 요청 전체 503"은 멀티 provider 환경에서 healthy provider까지 막아 가용성을 깨뜨림.
  - 3회 실패 후 장기 복구: 300s 대기 후 preload_fail_count=0 리셋 → 자동 재시도 재개
    300s 동안 해당 model+provider 조합은 dispatcher filter_candidates()와 Planner ①② 루프에서 제외
```

### 5. 모델 Pull — 요청 드레인

Ollama 모델 추가/교체 시 자동으로 요청을 차단하고 완료 후 재개한다.

**권한**: 모든 Pull/Drain API는 JWT admin 이상 필요. 일반 API 키로는 접근 불가.
  이유: 50GB+ 모델 pull은 서버를 최대 4시간 점유 — 일반 사용자 트리거 허용 시 DoS 가능.

```
POST   /v1/ollama/models/pull {model, provider_id}   ← RequireAdmin
DELETE /v1/ollama/models/pull/{provider_id}/{model}  ← RequireAdmin

POST /v1/ollama/models/pull {model, provider_id}
  → is_pulling=true 설정
  → 신규 요청: 해당 model+provider → 503 (pull in progress), is_pulling 해제까지 유지

  [1단계 — Drain] active_count==0 될 때까지 대기
    drain timeout 60s 초과 시: 강제 진행 (§7 processing cancel 경로 전체 수행)
      → in-flight SSE 처리 (이미 200 OK + SSE 헤더 전송됨이므로 503 불가):
          1. Job Runner에 cancel 신호 전송
          2. SSE error event 전송 후 스트림 종료:
               data: {"error":{"type":"service_update","message":"model pull in progress"}}\n\n
          3. LREM processing {job_id}     ← 누락 시 zombie job 잔존
          4. DB job status = "failed", failure_reason = "drain_forced"
          5. VramPermit drop → KV 반환, active_count 감소
          순서 보장: 1→2→3→4→5 (§7과 동일)
      → pull 강행 후 Ollama가 모델 교체 중이므로 해당 model+provider는 is_pulling=true 유지

  [2단계 — Pull] ollama pull 실행
    추적 필드:
      started_at:    pull 시작 시각
      heartbeat_at:  ollama pull progress 마지막 수신 시각 (30s마다 갱신)
      max_pull_secs: 기본 14400 (4h), 어드민 오버라이드 가능
    timeout 판정: (now - started_at) > max_pull_secs OR (now - heartbeat_at) > 300s
    timeout 초과 시: is_pulling=false 강제 해제, 에러 로그, Planner 재개
    어드민 강제 취소: DELETE /v1/ollama/models/pull/{provider_id}/{model}
    이유: pull hang으로 인한 특정 모델 서빙의 영구적 마비 방지.

  [3단계 — 재개] 완료:
    is_pulling=false, is_loaded=false
    sample_count=0, learning_epoch_started_at=now_ms
    baseline_tps=0, baseline_p95_ms=0
    preload_fail_count=0, preload_failed_at=0   ← 명시적 초기화 (stale 제외 방지)
    이유: pull은 모델 가중치 교체 이벤트 — evict보다 큰 환경 변화.
          epoch를 갱신하지 않으면 과거 1h 데이터가 새 재학습에 섞임 → Cold Start 무효화.
          baseline도 리셋하지 않으면 구 모델 기준값으로 AIMD가 잘못 수렴.
    → Placement Planner 다음 5s 루프에서 Preloader 자동 재로드 (Cold Start 재시작)
```

**수동 차단**: `PATCH /v1/providers/{id}/selected-models/{model}` ← RequireAdmin → `is_enabled=false`
로 pull 전 직접 차단 가능. 완료 후 `is_enabled=true` 재활성화.

### 6. Hard Gate Forced Drain Cancel 계약

Hard Gate 60s forced drain cancel 처리 계약은 §3 Hard Gate 항목에 정의됨 (§3 참고).
§7과 동일한 processing cancel 경로를 따름. failure_reason = "thermal_hard_gate".

### 7. 취소·타임아웃 계약 (Cancellation Contract)

**배경**: 현재 코드(`cancel_guard.rs`, `runner.rs`, `use_case.rs`)에는 processing 상태 job의
cancel 경로(CancelOnDrop → cancel() → DB cancelled + publish)가 구현돼 있다.
그러나 queued 상태(Valkey ZSET에 있는) job의 ZREM + demand DECR 경로는 코드상 없다.
이를 SDD에서 명시적으로 정의한다.

**두 상태의 cancel 경로 분리**:

```
[queued cancel]  job이 ZSET에 있는 상태 (아직 dispatch 전)
  공통 처리 (트리거 무관):
    1. Lua 원자 스크립트: ZREM queue:zset {job_id} + DECR demand:{model}
                          + HDEL queue:model {job_id} + HDEL queue:enqueue_at {job_id}
       (ZREM 반환 0 = 이미 dispatch됨 → processing cancel로 전환)
    2. DB job status / 클라이언트 응답은 트리거별로 분기:

  [client disconnect]   DB status = "cancelled"
                        SSE 응답 전송 없음 (연결이 이미 끊김)

  [max_queue_wait 초과] DB status = "failed", failure_reason = "queue_wait_exceeded"
                        SSE heartbeat 연결에 에러 이벤트 후 종료:
                          data: {"error":{"type":"timeout","message":"queue wait exceeded"}}\n\n

  [수동 취소 API]       DB status = "cancelled"
                        SSE heartbeat 연결에 취소 이벤트 후 종료:
                          data: {"error":{"type":"cancelled","message":"request cancelled"}}\n\n

[processing cancel]  job이 processing 리스트에 있는 상태 (runner 실행 중)
  트리거: 클라이언트 연결 끊김(CancelOnDrop) / runner 내부 오류 / pull drain 강제 중단
  처리:
    1. cancel() 호출 → runner SSE loop 중단
    2. SSE error event 전송 (스트림이 아직 열려 있을 때): drain_forced 또는 client_disconnect
    3. LREM processing {job_id}
    4. DB job status:
         클라이언트 연결 끊김 → "cancelled"
         pull drain 강제 / runner 오류 / timeout → "failed" (failure_reason 상황별)
    5. VramPermit drop → KV 반환

[timeout cancel]  max_queue_wait(300s) 초과
  queued 상태면 → queued cancel 경로
  processing 상태면 → Ollama response timeout → runner error → processing cancel 경로
```

**통합 Cancellation Contract**:

```
취소 사유              | 상태       | Valkey 처리               | DB 상태   | 클라이언트
-----------------------|------------|--------------------------|-----------|------------------
client disconnect      | queued     | ZREM + DECR (Lua atomic) | cancelled | SSE heartbeat 종료
client disconnect      | processing | cancel() + LREM          | cancelled | 연결 끊김
max_queue_wait         | queued     | ZREM + DECR (Lua atomic) | failed    | SSE error event
pull drain forced      | processing | cancel() + LREM          | failed    | SSE error event
Ollama timeout         | processing | cancel() + LREM          | failed    | SSE error event
thermal hard forced    | processing | cancel() + LREM          | failed    | SSE error event
수동 취소 API          | queued     | ZREM + DECR (Lua atomic) | cancelled | SSE error event
수동 취소 API          | processing | cancel() + LREM          | cancelled | SSE error event
no_eligible_provider   | processing | LREM                     | failed    | 503 또는 SSE error (§3 참고)
```

**LREM 보장 원칙**: Job Runner 종료 경로(정상 완료·오류·timeout 포함) 모두에서
`LREM processing {job_id}`를 반드시 수행한다.
cancel() 호출 여부와 무관하게 LREM은 최종 cleanup의 일부다.
미수행 시 processing 리스트에 zombie job 잔존 → startup recover 시 재실행 오염.

**멀티인스턴스 안전**: queued cancel Lua 스크립트의 ZREM 반환값이 0이면
  다른 인스턴스가 이미 dispatch한 것 → processing cancel로 전환.
  k8s 환경에서 인스턴스 A가 queued cancel을, 인스턴스 B가 dispatch를 동시에 시도해도
  Lua 원자성으로 둘 중 하나만 성공한다.

**단일 서버 환경**: 동일하게 동작. Lua ZREM 원자성은 인스턴스 수와 무관.

### 8. Gateway Intelligence — 서버 할당 자동화

**Scale-Out** (수평 확장, per-model 기준):
```
demand_counter(model) > eligible_capacity(model) × 0.80
  eligible_capacity = Σ max_concurrent(model, S)
                      for loaded S where:
                        !S.thermal_soft_gated && !S.thermal_hard_gated
                        && !S.circuit_open && !S.is_standby
                        && !ModelState(model, S).is_pulling
                        && !ModelState(model, S).dispatch_blocked
  // dispatch_blocked==true인 모델은 governor에 의해 실제 dispatch 불가.
  // capacity 계산에 포함하면 Scale-Out 조건이 충족되지 않아 확장이 억제됨.
  // governor가 cap을 적용 중일 때는 effective max_concurrent = governor_cap(model, S) 사용.
  //   eligible_capacity = Σ governor_cap(model, S)  (governor 활성 서버)
  //                     + Σ max_concurrent(model, S) (governor 비활성 서버)
  // total_capacity(모든 loaded 포함) 사용 금지: soft/hard gate·pull·CB open 상태인 provider는
  // 실제 신규 요청 수용 불가이므로 capacity에서 제외해야 Scale-Out 오발동 방지.
  // 예: Provider A loaded max=8 but Soft Gate → eligible=0.
  //     demand=6, eligible_capacity=0 → 0.80×0=0 → Scale-Out 발동 ✅
→ target = argmax(free_vram, servers where !ModelState(model, S).is_loaded && Pass 0 candidates)
    // !is_loaded는 "해당 모델이 해당 서버에 로드되지 않음" — model+provider 쌍 기준.
    // 동일 모델이 이미 로드된 서버는 Scale-Out 대상 제외 (이미 serving 중).
    // is_standby 서버도 Pass 0 candidates에 포함되므로 ④ 복귀 후 선택 가능.
    동점 처리: free_vram 동일 시 provider_id ASC (결정적 순서 → 멀티 인스턴스 split-brain 방지)
    후보 0개 (단일 서버 환경): no-op (Preloader 호출 없이 조용히 스킵)
→ Preloader(target, model) 전 Valkey atomic 선점:
    Lua: SET preloading:{model}:{provider_id} 1 NX EX 180
    반환 nil = 다른 인스턴스가 이미 선점 → 스킵
    이유: 멀티 인스턴스에서 동일 모델+서버 중복 Preload 방지

**선점 락 lifecycle**:
  획득 성공 → Preloader 실행
    정상 완료: DEL preloading:{model}:{provider_id} 즉시 해제
              (is_loaded=true가 되면 다음 Scale-Out은 !is_loaded 조건 불충족 → 자동 스킵)
    즉시 실패 (VramPool has_room 미충족): DEL 즉시 해제 → 다음 5s 루프 재평가
    timeout/오류 (3회 이내): DEL 즉시 해제 → preload_fail_count++ → 재시도 가능
    3회 실패: DEL 즉시 해제 + preload_failed_at 설정 → 300s 제외 (§4 규칙)
    Veronex 크래시: TTL 180s 자연 만료 → 다른 인스턴스 재시도 허용

Scale-Out 후 hold-down: 해당 server를 60s 동안 Scale-In 후보에서 제외.
(Preload 완료 → 큐 소진 → 즉시 Scale-In 과확장-과수축 진동 방지)
```

**Scale-In** (전력 절감, per-server 기준):
```
server_idle(S): demand==0 for all loaded models AND active_requests==0 AND !last_server
과도 상태 보호: is_preloading==true OR standby 복귀 후 30s 이내 → Scale-In 스킵
→ is_standby = true → Lazy Eviction 자연 발화 → Ollama 메모리 해제
```

**STANDBY → ACTIVE 복귀**: demand > 0 감지 → is_standby=false → 즉시 라우팅 재개.
모델이 아직 로드된 상태면 즉시 서빙 가능. 언로드된 경우 Preloader가 재로드.

**Placement Planner 루프 (5s)**:

2-pass 구조:
  [Pass 0 — 사전 계산 (루프 시작 시 1회)]
    scale_out_candidates = candidate_servers_for_scale_out()
      = {server | !server.thermal_soft_gated && !server.thermal_hard_gated
                  && !server.circuit_open
                  && free_vram(server) > 0}
      // standby 서버도 포함 (④에서 복귀 후 ①이 즉시 사용 가능하도록)
      // preload_failed_at은 model+provider 쌍 속성 → server 집합 필터 사용 금지.
      //   모델별 preload_failed_at 체크는 ①②에서 per-model로 수행한다.
      // is_pulling도 model+provider 쌍 속성 → 동일하게 ①②에서 per-model 처리.
      // thermal/CB를 Pass 0에서 제외해야 ④가 unusable 서버를 STANDBY 복귀 후보로 선정하지 않음.
    scale_out_needed = {model | demand(model) > total_capacity_excl_standby(model) × 0.80}
    // total_capacity 계산 시 governor_cap / dispatch_blocked 반영 (eligible_capacity §8 정의 참고)

  Pass 0은 상태 변경 없음 (read-only). ④①②③⑤ 모두 이 스냅샷을 기준으로 동작.

  **서버 단위 provisional VRAM reservation** (같은 사이클 다모델 충돌 방지):
    Pass 0에서 각 서버의 free_vram 스냅샷을 `provisional_free: HashMap<ProviderId, u32>` 로 복사.
    ①② 단계에서 Preloader 대상으로 선정될 때마다:
      provisional_free[server] -= model.weight_mb + model.kv_per_request_mb
    이후 같은 사이클의 ①②에서 다른 모델이 동일 서버를 선택할 때:
      provisional_free[server] < model.weight_mb → 해당 서버 후보에서 제외
    이유: Pass 0의 free_vram 스냅샷을 공유하면 여러 모델이 같은 서버를 동시에 선택.
          실제 has_room 체크는 Preloader 실행 시점에서 수행되지만,
          planner가 인위적 충돌을 유발하면 preload 실패 카운트 증가 → 300s 제외로 이어짐.
          provisional reservation으로 planner 단계에서 충돌을 선제 차단.
  **멀티 replica provisional_free 비결정성**:
    provisional_free는 각 replica의 in-memory 상태이므로 replica 간 공유되지 않음.
    두 replica가 동시에 다른 서버를 best_server로 선정하면 같은 모델을 두 서버에 동시 preload.
    (NX 락은 {model}:{provider_id} 단위이므로 서로 다른 서버 = 서로 다른 락 = 둘 다 성공)
    결과: 과도한 preload. 부하 경감보다 낭비.
    대응: Valkey에 `Scale-Out 결정 락`: `SET scaleout:{model} {replica_id} NX EX 30`
      반환 nil = 다른 replica가 이 모델의 Scale-Out 결정 중 → 스킵.
      30s TTL = 다음 Planner 사이클 전 자동 만료. 결정 완료(preload NX 락 획득) 후 DEL.

```
④ STANDBY 복귀: standby_server가 다음 중 하나를 만족할 때 → is_standby=false
     조건 A: 해당 서버에 is_loaded==true인 모델 중 demand>0인 것이 있음 (즉시 서빙 가능)
     조건 B: Pass 0의 scale_out_needed × scale_out_candidates 교차에서 해당 서버가 best_server로 선정됨
   (이유: ①보다 먼저 실행하되 ①의 계산 결과를 참조하려면 Pass 0에서 선행 계산해야 함.
          `any demand>0` 대신 실제로 서빙하거나 Scale-Out에 쓸 서버만 선별 복귀.)
① Scale-Out:   scale_out_needed 모델에 대해 Pass 0 candidate 중 best_server 선정
               (이미 ④에서 is_standby=false 처리됐으므로 total_capacity에 포함됨)
               && now_ms - preload_failed_at(model, best_server) >= 300_000
               → Preloader(best_server, model)
② Preload:     demand>0 && !is_loaded && !is_preloading && has_room
               && now_ms - preload_failed_at(model, provider) >= 300_000
               → Preloader
   (이유: §4의 3회 실패 300s 제외를 filter_candidates()뿐 아니라 Planner 루프에도 직접 적용.
          미적용 시 Planner가 5s마다 같은 preload를 반복 시도 — 300s 제외가 무효화됨)
③ Evict:       demand==0 && is_loaded && active==0 &&
               (idle ≥ 180s  OR  (is_standby && idle ≥ 30s))
               → evict; sample_count=0
⑤ Scale-In:    server_idle && !last_server && !in_transition → is_standby=true

충돌 방지: ①에서 Scale-Out 후보로 선정된 서버는 같은 사이클 ⑤에서 Scale-In 제외.
           ④에서 복귀한 서버는 30s transition guard → ⑤ 스킵.
           ②와 ⑤: ②에서 Preload 대상이 된 서버는 같은 사이클 ⑤에서 Scale-In 제외.
           ③와 ④: ④에서 복귀한 서버의 모델은 같은 사이클 ③ Evict 후보에서 제외
                   (복귀 직후 idle 조건 불충족으로 자연 제외됨).
Thermal 연동: thermal hard_gate 또는 soft_gate 활성 시 ①② 스킵.
             (soft gate: 추가 모델 로드 시 I/O 부하가 온도를 hard_gate로 밀 수 있음)
Pull 연동:   is_pulling은 ModelState 단위 (model+provider 쌍)이며 ProviderVramState 전체가 아님.
             단계별 제외 규칙:
             ① Scale-Out:  해당 model+provider 조합만 후보 제외. 같은 provider의 다른 model은 영향 없음.
             ② Preload:    해당 model+provider 조합만 후보 제외 (pull 중 동일 모델 재로드 금지).
             ③ Evict:      pull 중인 model 자체는 evict 제외. 같은 provider의 다른 model은 evict 허용.
             ④/⑤ STANDBY/Scale-In: provider의 해당 model capacity를 0으로 계산. 다른 model capacity는 정상 포함.
Scale-Out 중복 방지 (Dedup):
  - 이미 is_preloading==true && Thermal Normal인 서버 수 ≥ needed_servers이면 ① 스킵.
    needed_servers = ceil(demand_counter(model) / avg_max_concurrent(model))
    // 단순 "1개라도 preloading이면 스킵"은 급증 수요에서 확장을 직렬화함 (수요 2배인데 서버 1개씩 추가).
    // needed_servers 기준으로 동시 Scale-Out을 허용해 급증 수요에 병렬 대응한다.
    // 단일 서버 환경: needed_servers 계산 무의미, 그냥 no-op.
  - preloading 서버가 Soft/Hard 상태 진입 시 cleanup 절차:
      1. 해당 Preloader task에 cancel 신호 전송 (tokio CancellationToken)
      2. DEL preloading:{model}:{provider_id} (Valkey NX 락 즉시 해제)
      3. is_preloading=false (VramPool ModelState 원자적 리셋)
      순서 보장: 1→2→3. cancel 후 락 해제해야 다른 인스턴스가 즉시 재선점 가능.
      이후 다른 서버 Scale-Out 허용 (다음 5s 루프에서 재평가).
```

---

## 전체 데이터 흐름

```
클라이언트  POST /v1/chat/completions
    ▼
Veronex API  auth + rate limit → Job DB INSERT (status="queued")
    │  Lua enqueue_atomic: ZCARD < MAX_QUEUE_SIZE → ZADD + INCR demand  (원자, 큐 포화 시 429)
    │    enqueue 실패(429) 시: DB job status → "failed", failure_reason = "queue_full" (orphan 방지)
    │    (비원자 ZCARD+ZADD 금지 — overshoot 가능. §2 MAX_QUEUE_SIZE 원자화 명세 참고)
    │  SSE heartbeat 30s마다 (모델 로드 대기 중 연결 유지)
    ▼
Valkey ZSET
    │  Dispatcher 루프 (인스턴스마다)  ※ 마이그레이션 중: ZSET(1순위) + LIST drain(2순위) 병행 — Phase -1 참고
    │  ZRANGE K=20 → Rust window scoring
    │  Lua atomic handoff: ZREM queue:zset + LPUSH processing + DECR demand:{model}
    │    단일 스크립트로 원자 실행. 스크립트 실패(ZREM=0) = 선점 → 재시도.
    │    gap 없음: ZSET에서 사라진 job은 반드시 processing에 존재.
    ▼
Dispatcher
    │  filter_candidates()  active + type + model + !is_standby
    │  score_and_claim()    CB → thermal_gate → AIMD gate → try_reserve()
    │                       CB 우선, thermal hard_gate는 CB 해제 후 재평가
    │  LREM processing (ACK) — demand DECR은 위 handoff에서 완료됨
    ▼
Job Runner → Ollama SSE → 클라이언트
    │  응답 헤더: X-Job-Id: {job_id}  (최초 200 OK 응답 시 포함)
    │  이유: at-least-once 재실행 시 클라이언트가 중복 응답 여부를 job_id로 판단 가능
    ▼
VramPermit drop  → KV 반환 · last_active_at 갱신 · PostgreSQL+ClickHouse 기록

[Startup]
  sync_providers_once()       ← /api/ps → is_loaded 갱신 (dispatcher 전에)
  recover_processing_queue()  ← processing 잔존 job → ZADD 복원 + side hash 재구성 + LREM 정리
                                 1. LRANGE processing 0 -1 → job_id 목록 수집
                                 2. 각 job_id에 대해 DB 조회 (status, model, enqueued_at):
                                    - status=completed/failed: LREM processing {job_id} → 스킵
                                    - status=queued/processing: 복구 대상
                                 3. 복구 대상 job을 Lua 원자 스크립트로 처리:
                                    score = enqueued_at_ms - tier_bonus(job.tier)
                                    // tier는 DB job 레코드에서 조회 (job.tier 또는 api_key.tier).
                                    // 원래 enqueue score = now_ms - tier_bonus였으므로
                                    // 복구 시에는 enqueued_at_ms(DB)를 now_ms 대신 사용해 원래 우선순위 복원.
                                    ZADD queue:zset {score} {job_id}
                                    HSET queue:model {job_id} {model}         ← side hash 재구성
                                    HSET queue:enqueue_at {job_id} {enqueued_at_ms}  ← side hash 재구성
                                    LREM processing 0 {job_id}                ← 처리 완료 즉시 제거
                                 // 이유: ZADD만 하고 LREM 생략 시 동일 job이 다음 재시작에도 재처리 → 중복 큐잉.
                                 //       side hash 누락 시 demand_resync·promote_overdue에서 해당 job 누락.
                                 //       Lua 원자화로 ZADD↔LREM 사이 크래시로 인한 중복 처리 방지.
                                 중복 응답 방지: DB에서 job status=completed 확인 후
                                 이미 완료된 job은 ZADD 제외 (at-least-once → idempotent)
                                 **TPM 이중 차감 방지**: 재실행 job은 TPM을 재차감하지 않음.
                                   DB job의 reserved_tokens가 이미 존재하면 차감 스킵.
                                 **API 계약**: at-least-once. 크래시 후 재시도 시 중복 응답 가능.
                                   응답 헤더 X-Job-Id로 job_id 노출 → 클라이언트 멱등성 판단 가능.
  resync_demand_counters()    ← ZSET 실측 기반 demand_counter 재산정
  spawn dispatcher · placement_planner · sync_loop · demand_resync_loop(60s)
```

---

## Phase 구현 계획

### Phase -1 — 큐 마이그레이션 (LIST → ZSET)

기존 코드: `veronex:queue:paid`, `veronex:queue:standard`, `veronex:queue:test` 3개 LIST.
SDD 목표: `veronex:queue:zset` 단일 ZSET.

**배포 전략 (단일 배포, dual-read, 진정한 무중단)**:

핵심: 신 버전 dispatcher가 ZSET(primary) + LIST 3개(drain-only)를 동시에 읽는다.
이 방식으로 rolling 중 어떤 순간에도 모든 job이 즉시 처리된다.

```
─── 단일 배포 ──────────────────────────────────────────────────────────────

신 버전 코드:
  enqueue: ZSET에만 추가
  dispatcher: 매 루프마다 두 소스를 순서대로 확인 (non-blocking)
    1순위: ZSET window scoring (ZRANGE K=20 → Rust scoring → Lua ZREM)
           → 후보 있으면 즉시 처리, 없으면 2순위로
    2순위: LIST drain — RPOP paid (non-blocking) → RPOP standard → RPOP test
           (반환 nil이면 루프 sleep QUEUE_POLL_INTERVAL 후 1순위부터 재시작)
           tier_bonus 복원 불필요 — drain 대상은 이미 오래 대기한 job, 즉시 처리
  이유: BRPOP 0s(무기한 블로킹) 사용 시 LIST drain 중 새 ZSET job을 볼 수 없음
       → RPOP(non-blocking)으로 매 루프 ZSET을 반드시 먼저 확인

rolling 기간 (분 단위):
  구 버전 pod: LIST enqueue, LIST dispatch → 정상 처리
  신 버전 pod: ZSET enqueue, ZSET dispatch (+ LIST drain 병행)
  → 어떤 pod가 처리하든 job은 즉시 dispatched. 대기 적체 없음.

rolling 완료: 모든 replica 신 버전
  이 시점부터 LIST enqueue 없음 → LIST 잔존 job만 drain 대기

─── 자연 소진 대기 (배포 후 운영 중 자동 처리) ──────────────────────────────

LIST 잔존 job: dispatcher drain-only 경로가 계속 처리
신규 job: 전부 ZSET 경로
LLEN paid=0, standard=0, test=0 → 소진 완료

─── 정리 (소진 확인 후) ──────────────────────────────────────────────────────

LIST drain 코드 제거 (선택적 후속 배포)
DEL veronex:queue:paid veronex:queue:standard veronex:queue:test
```

**무중단 보장 근거**:
- rolling 중: 구 pod(LIST), 신 pod(ZSET+drain) 모두 즉시 처리 — 적체 없음
- 신 pod가 LIST job을 drain으로 처리할 수 있으므로 구 pod 종료 타이밍 무관
- **max_queue_wait=300s**: LIST backlog가 클 경우 drain 속도(~3-5 jobs/s)에 따라
  LIST 후미 job은 300s를 초과할 수 있음. "초과 위험 없음"은 정확하지 않다.
  rolling 전 `LLEN paid/standard/test` 확인 후 backlog가 클 때는 트래픽 낮은 시간대 rolling 필수.

**알려진 의미적 제약 (semantic limitation) — rolling 중**:
- LIST 잔존 job은 demand_counter에 포함되지 않음 → Placement Planner·Scale-Out이 실제 대기량을
  과소 추정 → under-scale 발생 가능 (backlog가 클수록 심각).
- LIST 잔존 job은 promote_overdue 대상 아님 → EMERGENCY_BONUS 미적용 → 250s 대기해도
  기아 방지 보장 없음. FIFO(tier 고정 우선순위)로만 처리됨.
- LIST 잔존 job은 side hash 없음 → ZSET window scoring 미적용.
- 신규 ZSET job이 오래된 LIST job보다 먼저 처리될 수 있어 기존 FIFO/tier SLA 계약이 일시적으로 깨짐.
  예: LIST standard 대기 200s → 신규 ZSET paid → ZSET 먼저 처리. 이는 의도된 동작이 아님.
- **운영 필수 절차** (enforcement):
  1. rolling 전: `LLEN veronex:queue:paid`, `standard`, `test` 확인.
  2. 합산 LLEN > 500건이면 반드시 트래픽 낮은 시간대(off-peak) rolling.
  3. rolling 완료 후 최소 15분간 error rate, queue_wait_exceeded, p95 latency 모니터링.
  4. paid tier SLA 역전 감지 시 롤백 절차 즉시 실행.

**롤백**:
enqueue를 다시 LIST로 되돌릴 수 있다. 단, ZSET 잔존 job 처리가 보장되어야 한다.

롤백 규칙:
1. 최소 1개 이상의 신 버전 pod(ZSET dispatcher)를 유지한 채 enqueue만 LIST로 전환한다.
2. 신 버전 pod가 ZSET 잔존 job을 모두 drain한다.
3. `ZCARD veronex:queue:zset = 0` 확인 후 나머지 신 버전 pod를 종료하고 구 버전으로 완전 복귀한다.

이유: 구 버전 pod는 ZSET를 읽지 못하므로, 신 버전 pod를 모두 먼저 내리면 ZSET 잔존 job이 고아가 된다.

대안: 별도 one-shot 복구 스크립트로 ZSET job을 LIST로 재삽입한 뒤 구 버전으로 완전 롤백할 수 있다.
**정리 배포**: LIST drain 코드 제거는 선택적. 제거 전 반드시 LLEN=0 확인.

### Phase 0 — DB 마이그레이션
**파일**: `migrations/postgres/000002_intelligence_serving.up.sql`

```sql
CREATE UNIQUE INDEX uq_llm_providers_ollama_url
    ON llm_providers (url) WHERE url <> '' AND provider_type = 'ollama' AND deleted_at IS NULL;

ALTER TABLE llm_providers ADD COLUMN num_parallel SMALLINT NOT NULL DEFAULT 4;

CREATE TABLE model_registries (
    id UUID PRIMARY KEY DEFAULT uuidv7(), name VARCHAR(255) NOT NULL,
    url TEXT NOT NULL UNIQUE, description TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### Phase 1 — APU VRAM 수정
**파일**: `hw_metrics.rs`, `health_checker.rs`, `analyzer.rs`

`HwMetrics`에 `mem_available_mb: u32` 추가. `health_checker.rs`: NodeMetrics 매핑.
`analyzer.rs:454`: APU 경로 → `hw.mem_available_mb × (1 - safety_permil/1000)` 사용.
fallback: `weight * 115/100`.

### Phase 2 — Thermal 보호 (perf_factor + gates + admin)
**파일**: `capacity/thermal.rs`, `infrastructure/inbound/http/capacity_handlers.rs`

```rust
pub struct ThermalThresholds {
    pub normal_below: f32,  // default AMD APU: 75.0
    pub soft_at: f32,       // default AMD APU: 82.0
    pub hard_at: f32,       // default AMD APU: 90.0
    pub cooldown_secs: u64, // default: 300
}

pub enum ThermalState {
    Normal,
    Soft,
    Hard,
    Cooldown,
    RampUp,  // Cooldown 후 점진 복귀 단계. max_concurrent=1 강제, 30s마다 +1
}

impl ThermalThresholds {
    pub fn amd_apu() -> Self { Self { normal_below: 75.0, soft_at: 82.0, hard_at: 90.0, cooldown_secs: 300 } }
    pub fn nvidia_gpu() -> Self { Self { normal_below: 80.0, soft_at: 88.0, hard_at: 93.0, cooldown_secs: 300 } }
}

pub fn perf_factor(&self, temp_c: f32) -> f64 {
    if temp_c <= self.normal_below { 1.0 }
    else if temp_c <= self.soft_at {
        let r = (temp_c - self.normal_below) / (self.soft_at - self.normal_below);
        (1.0 - 0.30 * r as f64).max(0.0)   // 75°C→1.0, 82°C→0.70
    } else if temp_c <= self.hard_at {
        let r = (temp_c - self.soft_at) / (self.hard_at - self.soft_at);
        (0.70 - 0.70 * r as f64).max(0.0)  // 82°C→0.70, 90°C→0.0
    } else { 0.0 }
}
```

어드민 API: `PATCH /v1/providers/{id}/thermal-thresholds` → `ThermalThresholds` 저장.
  **권한**: JWT admin 이상 필요 (RequireAdmin extractor).
  **입력 검증**:
    - `normal_below < soft_at < hard_at` 순서 보장 (위반 시 422)
    - `normal_below ≥ 50.0`, `hard_at ≤ 100.0` (범위 이탈 시 422)
    - `60 ≤ cooldown_secs ≤ 3600` (최소 1분, 최대 1시간)
      이유: cooldown_secs=0 허용 시 Cooldown이 즉시 종료 → "부하 재개 → 즉시 throttling" 무한 루프.
            normal_below·soft_at·hard_at 최소 간격: 각 인접 값 차이 ≥ 3.0°C (위반 시 422)
      이유: 간격이 너무 좁으면 perf_factor 선형 보간 구간이 사실상 없어 감쇄 효과 무효화.
health_checker 30s 루프에서 `thermal.update(temp_c)` 호출 → `ThermalState` 갱신.
Cooldown 후 ramp-up: `max_concurrent` 강제 1 설정, 이후 AIMD 재개.

### Phase 3 — ZSET 큐 + Window Scoring + Demand Counter
**파일**: `valkey_keys.rs`, `domain/constants.rs`, `valkey_port.rs`, `valkey_adapter.rs`, `use_case.rs`, `dispatcher.rs`

```rust
// constants.rs
pub const QUEUE_JOBS_ZSET: &str    = "veronex:queue:zset";
pub const QUEUE_WINDOW_SIZE: usize = 20;
pub const LOCALITY_BONUS_MS: f64   = 20_000.0;
pub const TIER_BONUS_PAID: f64     = 300_000.0;
pub const TIER_BONUS_STANDARD: f64 = 100_000.0;
pub const TIER_EXPIRE_SECS: u64    = 250;
pub const EMERGENCY_BONUS_MS: f64  = 300_000.0;  // = TIER_BONUS_PAID; wait>250s 기아 방지
pub const OVERDUE_PROMOTE_SECS: u64 = 30;         // promote_overdue_loop 주기 (top-K 진입 보장)
pub const MAX_QUEUE_WAIT_SECS: u64 = 300;
pub fn demand_counter(model: &str) -> String { format!("veronex:demand:{model}") }

// 추가 Valkey 메서드
async fn zrange_withscores(&self, key: &str, limit: usize) -> Result<Vec<(String, f64)>>;
async fn zrem(&self, key: &str, member: &str) -> Result<i64>;
async fn incr_floor(&self, key: &str, delta: i64) -> Result<i64>;  // DECR + max(0)
async fn enqueue_atomic(&self, job_id: &str, score: f64, model: &str) -> Result<bool>; // Lua 원자
```

**enqueue Lua 스크립트** (§2 MAX_QUEUE_SIZE 원자화 명세 참고):
ZCARD 체크 + ZADD + demand INCR을 Lua 단일 스크립트로 원자 실행.
반환 0 = 큐 포화(429). 비원자 ZCARD+ZADD 사용 금지 — overshoot 발생.

### Phase 4 — Atomic Handoff + Demand Resync
**파일**: `dispatcher.rs`, `use_cases/demand_resync.rs` (신규)

```lua
-- Lua atomic handoff: ZSET → processing + demand DECR + side hash 정리 (단일 스크립트)
-- KEYS[1]=queue:zset  KEYS[2]=processing  KEYS[3]=demand:{model}
-- KEYS[4]=queue:enqueue_at  KEYS[5]=queue:model
-- ARGV[1]=job_id
local removed = redis.call('ZREM', KEYS[1], ARGV[1])
if removed == 0 then return 0 end  -- 다른 인스턴스가 선점
redis.call('LPUSH', KEYS[2], ARGV[1])
local v = redis.call('decr', KEYS[3])
if v < 0 then redis.call('set', KEYS[3], 0) end
redis.call('HDEL', KEYS[4], ARGV[1])  -- enqueue_at side hash 정리
redis.call('HDEL', KEYS[5], ARGV[1])  -- model side hash 정리
return 1

-- Lua queued cancel: ZSET 제거 + demand DECR + side hash 정리 (cancel/timeout 경로, §7)
-- KEYS[1]=queue:zset  KEYS[2]=demand:{model}
-- KEYS[3]=queue:enqueue_at  KEYS[4]=queue:model
-- ARGV[1]=job_id
-- 반환값 0 = 이미 dispatch됨 → processing cancel로 전환
local removed = redis.call('ZREM', KEYS[1], ARGV[1])
if removed == 0 then return 0 end
local v = redis.call('decr', KEYS[2])
if v < 0 then redis.call('set', KEYS[2], 0) end
redis.call('HDEL', KEYS[3], ARGV[1])  -- enqueue_at side hash 정리
redis.call('HDEL', KEYS[4], ARGV[1])  -- model side hash 정리
return 1
```

```rust
// demand_resync_loop: 60s마다 ZSET 실측 기반 재산정 (drift 보정)
// 1. ZSCAN veronex:queue:zset 0 COUNT 200 → 전체 job_id 수집 (ZSET = 단일 진실 소스)
// 2. 수집된 job_id 배치로 HMGET veronex:queue:model {job_ids...} → model 일괄 조회
// 3. model별 집계 → SET demand:{model} count
// 4. [stale GC] HSCAN veronex:queue:model → ZSET에 없는 job_id 발견 시 HDEL 즉시 제거
//              HSCAN veronex:queue:enqueue_at → 동일하게 stale entry 제거
//              이유: dispatch·cancel Lua는 HDEL을 원자 처리하지만, 크래시 등 예외 경로에서
//                   stale hash entry가 누적될 수 있음. 60s GC가 최종 방어선.
//              비용: MAX_QUEUE_SIZE=10,000 × HSCAN COUNT 200 = 최대 50 왕복. 60s 주기에 무시 가능.
// 이유: HSCAN hash 단독 사용 시 stale entry → demand 과대복구 → Planner Scale-Out/Preload 오발동.
//       ZSCAN 기반 교집합만 집계 → stale 자동 제외. GC로 hash 무한 증가 방지.
// 주의: HMGET nil(model 조회 실패) job_id는 집계 제외 (정상: dispatch 직후 HDEL 타이밍 차이).
```

### Phase 4b — Queued Cancel + Promote Overdue
**파일**: `dispatcher.rs`, `use_cases/inference/use_case.rs`, `use_cases/promote_overdue.rs` (신규)

queued cancel Lua 스크립트 구현 (§7 계약). cancel/timeout 경로에서 호출.
반환값 0 = 이미 dispatch됨 → processing cancel 경로로 전환.
SSE heartbeat 연결에서 에러 이벤트 전송 후 종료.

**promote_overdue_loop** (30s 주기, §2 top-K 진입 보장):
```rust
// 의존성: queue:enqueue_at hash의 정합성에 직접 의존.
//   이 hash에 없는 job은 overdue 승격 대상 누락 → fairness 보장 붕괴.
//   정합성 보장 경로 (모두 충족해야 함):
//     enqueue:  Lua enqueue_atomic에서 HSET queue:enqueue_at 원자 기록
//     dispatch: Lua handoff에서 HDEL queue:enqueue_at 원자 정리
//     cancel:   Lua queued_cancel에서 HDEL queue:enqueue_at 원자 정리
//     startup:  recover_processing_queue()에서 HSET queue:enqueue_at 재구성 (Fix 7)
//               → 크래시 복구 후에도 overdue 승격 보장
//
// HSCAN veronex:queue:enqueue_at CURSOR COUNT 200 → 전체 순회 (score 무관)
// 각 (job_id, enqueue_at_ms) 에 대해:
//   wait_ms = now_ms - enqueue_at_ms
//   if wait_ms > TIER_EXPIRE_SECS*1000:
//     new_score = enqueue_at_ms - EMERGENCY_BONUS_MS
//     ZADD queue:zset XX {new_score} {job_id}   (이미 존재하는 경우만 갱신, 중복 방지)
// 이후 ZRANGE는 raw score 순서로 overdue job을 top-K 안으로 자연 선출
// side hash 정리: dispatch/cancel 시 HDEL veronex:queue:enqueue_at {job_id}
// 비용: MAX_QUEUE_SIZE=10,000 × COUNT 200 = 최대 50 HSCAN 왕복. 30s 주기에 무시 가능.
// stale GC: demand_resync_loop가 60s마다 queue:enqueue_at stale entry 제거 (Fix 2).
```

### Phase 5 — Placement Planner + Startup
**파일**: `application/use_cases/placement_planner.rs` (신규), `main.rs`

5초 루프. 위 핵심 메커니즘 §8 루프 ①~⑤ 구현 (충돌 방지 + thermal 연동 포함).
`main.rs` 원자성 유지: `sync_providers_once` → `recover_processing_queue` → `resync_demand_counters` → spawn.

### Phase 6 — Preloader + SSE Heartbeat
**파일**: `infrastructure/outbound/ollama/preloader.rs` (신규), `dispatcher.rs`

```rust
// is_preloading=true 설정 후 호출. 완료/실패 시 false 복원.
//
// preload 성공 시:
//   is_preloading = false, is_loaded = true
//   preload_fail_count = 0   ← 연속 실패 카운터 리셋 (성공 1회로 "3회 연속" 계약 초기화)
//   preload_failed_at = 0    ← 제외 타이머 해제
//   이유: "3회 연속"은 성공이 끼면 카운트가 끊겨야 함. 리셋 없으면 과거 실패가 누적돼
//         건강한 provider가 예기치 않게 300s 제외되는 false positive 발생.
//
// preload 실패 시 (120s timeout 또는 오류):
//   is_preloading = false
//   preload_fail_count += 1
//   if preload_fail_count >= 3:
//     preload_failed_at = now_ms   ← 300s 제외 시작
//     preload_fail_count = 0       ← 다음 300s 후 재시도 사이클을 위해 리셋
//
// 3회 연속 실패 → 해당 model+provider 조합 300s 제외
//   다른 healthy provider가 있으면 라우팅 계속.
//   모든 provider 제외될 때만 해당 모델 요청에 503 반환. (§4 기준 통일)
client.post(format!("{url}/api/generate"))
    .json(&json!({"model": model, "prompt": "", "num_predict": 0, "keep_alive": -1}))
    .timeout(Duration::from_secs(120)).send().await?;

// SSE heartbeat: 클라이언트 연결 유지 (모델 로드 대기 중)
// 30s마다 "data: \n\n" 전송. MAX_QUEUE_WAIT_SECS 초과 시 SSE error event 후 종료.
// (200 OK + SSE 헤더 전송 후에는 503 불가 — SSE error event로 통일)
```

### Phase 7 — VramPool 상태 필드 추가
**파일**: `capacity/vram_pool.rs`

`ProviderVramState`: `is_standby: AtomicBool`, `transition_until: AtomicU64`.

// ModelState 범위: model+provider 쌍 (provider_id × model_name). ProviderVramState의 HashMap 값.
// 모든 AtomicXxx 필드는 동시성 안전. 저장 위치를 명시하지 않은 상태는 이 목록이 단일 정의.
`ModelState`:
  `last_active_at: AtomicU64`              ← VramPermit::drop() 시 now_ms로 갱신
  `is_preloading: AtomicBool`             ← Preloader 실행 중. 완료/실패 시 false 복원.
                                              scope: model+provider 쌍
  `is_pulling: AtomicBool`               ← Pull 진행 중. 범위 = model+provider 쌍 (provider-wide 아님).
                                              같은 provider의 다른 model은 영향 없음.
                                              저장 위치: ModelState (ProviderVramState.models HashMap 값).
  `sample_count: AtomicU32`
  `preload_fail_count: AtomicU32`         ← 연속 실패 횟수. preload 성공 시 즉시 0으로 리셋 (§17).
  `preload_failed_at: AtomicU64`          ← 3회 연속 실패 시각 (Unix ms). 0 = 정상.
                                              filter_candidates()에서 `now - preload_failed_at < 300_000ms`
                                              이면 해당 model+provider 제외.
  `learning_epoch_started_at: AtomicU64`  ← evict 시 now_ms로 갱신. ClickHouse 집계 기준점.
  `dispatch_blocked: AtomicBool`          ← governor share=0 시 설정. max_concurrent 필드는 변경하지 않음.
                                              근거: max_concurrent=0 → deadlock (L164). 플래그로 대체해 보호.
                                              AIMD 루프 시작 시 전체 모델 false로 초기화 후 재평가.
  `pre_hard_max_concurrent: AtomicU32`    ← Hard 진입 직전 max_concurrent snapshot (1회 기록).
                                              저장 시점: Normal/Soft/RampUp → Hard 전이 시 즉시 기록.
                                              용도: RampUp 종료 조건 = AIMD current ≥ pre_hard_max_concurrent.
                                              초기값: 0 (Hard 미진입 상태). 0이면 RampUp 종료 조건 불사용.
`VramPermit::drop()`: `last_active_at` 갱신. evict 시: `is_loaded=false`, `sample_count=0`, `is_preloading=false`.
추가 메서드: `idle_since_secs()`, `set_standby()`, `set_active()`, `in_transition()`.

safety_permil 규칙:
- OOM (try_reserve 실패 or Ollama 429): `safety_permil = min(safety_permil + 50, 500)`
- 30s 루프에서 OOM 없이 안정적: `safety_permil = max(safety_permil - 10, 100)`

### Phase 8 — AIMD num_parallel 상한 + Cooldown Ramp-up
**파일**: `vram_pool.rs`, `analyzer.rs`

**파괴적 변경**: 기존 `initial_max_concurrent()` / `weight_based_max_concurrent()` 함수 제거.
Cold start 초기값을 weight 기반 휴리스틱(`>50GB→1` 등)에서 `provider.num_parallel`로 교체.

Cold Start: `initial = provider.num_parallel as u32`.
  **Cold Start 동시 폭주 방어**: governor는 30s AIMD 루프에서만 발동하므로
  여러 모델이 동시에 num_parallel로 Cold Start하면 초기 30s 동안 governor가 개입하지 못함.
  대응: Preloader 완료 시점(is_loaded=true 설정 직전) max_concurrent 초기화:
    initial = min(provider.num_parallel, provider.num_parallel - provider_committed_parallel)
    provider_committed_parallel = Σ max_concurrent(loaded_model, provider) for all already-loaded models
    // 이미 로드된 모델이 점유한 parallel 슬롯을 제외하고 남은 슬롯만 초기값으로 사용.
    // 예: num_parallel=8, 기존 모델이 max=4 점유 → 신규 Cold Start initial = min(8, 8-4) = 4.
    // 단일 모델만 있으면 initial = num_parallel 그대로 (폭주 없음).
    // max(1, ...) 하한 적용 — 0이면 dispatch 불가.

AIMD 증가: `(current + 1).min(provider.num_parallel as u32)`.
Cooldown 복귀 후: `max_concurrent = 1` 강제. 이후 AIMD 30s 루프에서 +1씩 정상 재개.

**Provider-wide pressure governor** (APU 대역폭 포화 방어):
  AIMD는 model×provider 쌍 독립 학습이지만 병목은 provider 전체 대역폭.
  Cold Start 초기화 때 committed_parallel 차감으로 1차 방어. governor는 2차 방어.
  여러 모델이 동시에 Cold Start하면 초기 30s 과포화 발생 가능 → governor가 다음 루프에서 보정.
  각 model의 TPS 하락 원인 귀속 불가 → governor가 fair-share로 정리.

  30s AIMD 루프 시작 시:
    provider_total_active = Σ active_count(model, provider) for all loaded models
    if provider_total_active > provider.num_parallel:   ← provider 전체 상한 초과
      → **fair-share 분배**: Σ model_cap = num_parallel 수학적 보장
        budget = provider.num_parallel
        // 필터: active_count > 0 OR demand_counter > 0 (queued_or_active)
        // 이유: active_count > 0 단독 필터는 share=0 → max_concurrent=0 → active_count=0 →
        //       다음 사이클 제외 → demand 있는데 영구 dispatch 불가 deadlock 발생.
        //       demand_counter > 0 포함으로 대기 job이 있는 model은 항상 재포함.
        candidates = loaded models where (active_count > 0 OR demand_counter > 0)
        n = candidates.len()

        if n ≤ budget:
          base = budget / n    // ≥ 1 보장 (n ≤ budget이므로)
          rem  = budget % n
          각 model에 base 배정 후, oldest_queued_ms 오름차순 상위 rem개에 +1
          // (oldest_queued_ms = 해당 model의 ZSET 내 가장 오래된 job의 enqueue_at_ms, 없으면 u64::MAX)
          // 모든 candidate share ≥ 1 → deadlock 없음

        if n > budget:
          // budget보다 많은 model이 경합.
          // 정렬 기준: oldest_queued_ms 오름차순 (가장 오래 기다린 model 우선)
          // 이유: demand_counter 내림차순은 고수요 model이 매 사이클 상위를 독점
          //       → 저수요 model이 age 아무리 커져도 model-gate에서 영구 차단 (starvation).
          //       oldest_queued_ms 기준은 EMERGENCY_BONUS(250s)·promote_overdue와 일관된 공정성 축.
          //       고수요 model은 많은 job을 빠르게 소화 → oldest_queued_ms가 빠르게 갱신
          //       → 저수요 model의 old job이 자연스럽게 상위 진입.
          상위 budget개 (oldest_queued_ms 오름차순): share = 1  (Σ = budget ✅)
          나머지 (n-budget)개: share = 0

        // governor_cap(model): 이 사이클의 임시 dispatch 상한 (max_concurrent와 별개 필드).
        //   ModelState에 `governor_cap: AtomicU32` 추가. 0 = cap 없음(governor 비활성).
        //
        // share > 0: governor_cap = min(max_concurrent, share). max_concurrent는 변경하지 않음.
        //   이유: max_concurrent에 governor 값을 덮어쓰면 governor 해제 후 원래 AIMD 학습값이
        //         복구되지 않음 → AIMD가 cap 수준(예: 2)에서 재시작 → 학습값(예: 8) 영구 소실.
        //   governor_cap은 "이 사이클의 dispatch 허용 상한"으로만 사용.
        //   dispatch: active_count < min(max_concurrent, governor_cap) 일 때만 수용.
        //             governor_cap=0이면 max_concurrent만 적용 (cap 없음).
        //
        // share = 0: dispatch_blocked = true. governor_cap 설정 불필요.
        //            max_concurrent, governor_cap 필드 변경 없음.
        //            근거: max_concurrent=0 → deadlock (§1 AIMD 하한=1 참고).
        //            dispatch 진입 시: dispatch_blocked==true이면 즉시 skip.
        //            in-flight 요청은 계속 진행 (graceful drain).
        //
        // 다음 AIMD 루프 시작 시:
        //   dispatch_blocked = false (모든 모델) 초기화
        //   governor_cap = 0 (모든 모델) 초기화
        //   → governor 재평가. 비활성이면 max_concurrent 그대로 AIMD 재개.
        //
        // governor 비활성 사이클(total_active ≤ num_parallel):
        //   baseline_tps 갱신도 정상 재개.
        //   governor 활성 사이클에서는 baseline_tps 갱신 동결:
        //   governor가 강제한 cap 수준에서 측정된 TPS는 "모델 본래 처리량"이 아님.
        //   동결하지 않으면 낮은 baseline이 다음 AIMD ratio 계산에 영향을 줌.
        for each model in candidates:
          if share(model) > 0:
            model.governor_cap = min(model.max_concurrent, share(model))
            model.dispatch_blocked = false
          else:
            model.governor_cap = 0
            model.dispatch_blocked = true
      → 이 사이클의 AIMD 증가(+1) 금지
      → 이 사이클의 AIMD 감쇄(×3/4) suppress (적용 안 함)
      → governor의 fair-share cap만 최종값으로 적용
      → 다음 사이클에서 provider_total_active ≤ num_parallel 확인 후 governor 비활성, AIMD 정상 재개
  이유: 기존 per-model `num_parallel/2` cap은 3모델 시 총합 1.5×num_parallel → 상한 초과.
       floor + rem 분배 (n ≤ budget) 또는 top-budget=1 분배 (n > budget)는 Σ = budget 수학적 보장.
       queued_or_active 필터가 share=0 deadlock을 원천 차단.
       model-local AIMD를 동시 적용하면 이중 감쇄이므로 suppress.
  **우선순위**: governor 활성 → provider-global fair-share가 최종값. AIMD 증가·감쇄 모두 suppress.
  다음 사이클에서 governor 비활성 → AIMD 증가·감쇄 정상 재개.

### Phase 9 — CDD 문서
`capacity.md`: APU VRAM, safety_permil 규칙, Cold Start, sample_count 리셋.
`distributed.md`: ZSET+Lua ZREM, 크래시 복구, demand counter, resync loop,
  EMERGENCY_BONUS(250s 기아 방지), cancellation contract(queued vs processing).
`providers/hardware.md`: thermal thresholds 어드민 API, 벤더별 기본값.
Circuit Breaker: 기존 구현 유지 (`score_and_claim()` 내부). CB+Thermal 우선순위 문서화.

---

## 파일별 변경 요약

| 파일 | 유형 | Phase |
|------|------|-------|
| `migrations/postgres/000002_*.up.sql` | 신규 | 0 |
| `infrastructure/outbound/hw_metrics.rs` | 수정 | 1 |
| `infrastructure/outbound/health_checker.rs` | 수정 | 1 |
| `capacity/analyzer.rs` | 수정 | 1, 8 |
| `capacity/thermal.rs` | 추가 | 2 |
| `inbound/http/capacity_handlers.rs` | 수정 | 2 |
| `valkey_keys.rs` + `domain/constants.rs` | 추가 | 3 |
| `ports/outbound/valkey_port.rs` + `valkey_adapter.rs` | 추가/구현 | 3 |
| `use_cases/inference/use_case.rs` | 수정 | 3 |
| `use_cases/inference/dispatcher.rs` | 수정 | 3, 4, 4b |
| `use_cases/inference/use_case.rs` | 수정 | 3, 4b |
| `use_cases/demand_resync.rs` | 신규 | 4 |
| `use_cases/promote_overdue.rs` | 신규 | 4b |
| `use_cases/model_pull.rs` | 신규 | 5 |
| `use_cases/placement_planner.rs` | 신규 | 5 |
| `main.rs` | 수정 | 5 |
| `outbound/ollama/preloader.rs` | 신규 | 6 |
| `capacity/vram_pool.rs` | 수정 | 7, 8 |
| `docs/llm/inference/capacity.md` + `infra/distributed.md` | 수정 | 9 |
| `docs/llm/providers/hardware.md` | 수정 | 9 |

**의존 관계**: 0·1·2 독립 → 3 (2 후) → 4·4b (3 후) → 6·7 독립 → 5 (3·4·4b·6·7 후) → 8 (3·2 후) → 9 (전체 후)

---

## 범위 외

| 항목 | 이유 |
|------|------|
| MinIO 모델 배포 | 소수 서버 ROI 없음 |
| NVIDIA 지원 | 추후 별도 |
| 서버 OS 전원 제어 (ACPI) | Ollama 유휴 = 실질 저전력으로 충분 |
| 서버 간 모델 가중치 복사 | 각 Ollama 서버 독립 설치 가정 |
| Thermal 임계값 자율 학습 | 안전 한계는 하드웨어 스펙 기반 고정값이 원칙 |

---

## Design Review Guide

> **이 섹션 전체를 별도 리뷰 세션의 프롬프트로 사용하세요.**
> 목표: 구현 시작 전 설계의 논리적 완결성을 검증합니다.
> 각 항목을 ✅ sound / ⚠️ needs improvement / ❌ design flaw 로 평가하세요.

---

## Part 1 — 시스템 사전 정보

> 이 SDD를 처음 보는 리뷰어를 위한 기존 시스템 컨텍스트입니다.
> SDD는 기존 시스템 위에 새 기능을 추가/변경하는 문서입니다.

### 전체 시스템 구조

```
클라이언트 (OpenAI SDK, curl, 웹 대시보드)
    │ POST /v1/chat/completions
    ▼
Veronex Gateway (Rust/Axum)
    ├── Auth 미들웨어  (JWT + API Key, BLAKE2b hash)
    ├── Rate Limiter   (RPM sliding window + TPM counter, Valkey)
    ├── Inference Use Case
    │       ├── Dispatcher (큐에서 job 꺼내 provider 선택)
    │       ├── Job Runner (Ollama SSE 중계)
    │       └── VramPool   (메모리 예약·반환)
    └── Dashboard API  (사용량, 성능 분석)
         │
         ├──► PostgreSQL  (job, api_keys, providers, model_vram_profiles)
         ├──► Valkey      (큐, VRAM 조정, rate limit)
         ├──► Ollama A/B/N (실제 LLM 추론)
         └──► ClickHouse  (analytics, AIMD 학습 소스)
                  ▲
         veronex-agent → OTel Collector → Redpanda → ClickHouse
         (node-exporter + Ollama /api/ps 스크랩 → OTLP push)
```

### 구성 요소별 역할

| 구성 요소 | 역할 | SDD와의 관계 |
|-----------|------|--------------|
| **Veronex Gateway** | OpenAI 호환 HTTP API, 요청 라우팅, SSE 중계 | SDD 변경 대상 (큐·AIMD·Thermal 개선) |
| **veronex-agent** | node-exporter + Ollama `/api/ps` 스크랩 → OTLP push | `mem_available_mb`·`temp_c` 소스 (Phase 1·3) |
| **PostgreSQL** | job 이력, API 키, provider 등록, VRAM 학습값 영구 저장 | Phase 0 마이그레이션 대상 |
| **Valkey** | 요청 큐, VRAM 임대, rate limit, pub/sub | 큐 구조 변경 (LIST → ZSET) |
| **ClickHouse** | inference_logs, audit_events 분석 저장소 | AIMD 30s 학습 루프의 데이터 소스 |
| **OTel Pipeline** | agent → Collector → Redpanda → ClickHouse | 변경 없음, 읽기 소스로만 사용 |
| **Ollama** | 실제 LLM 추론 엔진 | Veronex가 API 제어 (`/api/generate`, `/api/ps`, `/api/pull`) |

### 현재 코드 vs SDD가 바꾸는 것

**큐 구조**:
```
현재: paid/standard/test 3개 LIST → Lua priority pop (tier 고정 우선순위)
SDD:  단일 ZSET → Rust window scoring → Lua ZREM (age·locality·tier 복합)
```

**AIMD Cold Start**:
```
현재: weight 기반 (51GB→1, 18GB→2, 5GB→4, <5GB→8)  ← initial_max_concurrent()
SDD:  provider.num_parallel (파괴적 변경, 함수 삭제)
```

**Thermal 상태**:
```
현재: Normal / Soft / Hard (3개), Hard cooldown 기본 7,200s
SDD:  Normal / Soft / Hard / Cooldown / RampUp (5개), Hard cooldown 300s
```

**신규 컴포넌트** (현재 코드에 없음):
```
placement_planner.rs  — 5s 루프: Scale-Out·Preload·Evict·STANDBY·Scale-In
preloader.rs          — POST /api/generate num_predict=0 으로 모델 선로드
model_pull.rs         — Pull drain → Pull → 재로드 워크플로우
demand_resync.rs      — 60s마다 demand_counter 재산정
```

### 핵심 용어

| 용어 | 의미 |
|------|------|
| Provider | Ollama 서버 1개 |
| Job | 요청 1건 (DB 저장 + Valkey 큐 대기) |
| ZSET | Valkey Sorted Set — score 기준 정렬 큐 |
| AIMD | 동시 요청 수 자율 학습 (TPS 피드백 기반) |
| num_parallel | Ollama에 설정하는 모델당 최대 동시 요청 수 |
| safety_permil | 메모리 안전 마진 (permil 단위, 기본 100 = 10%) |
| perf_factor | 온도 비례 성능 계수 (75°C→1.0, 90°C→0.0) |
| Lazy Eviction | 수요 없는 모델을 idle 후 메모리에서 제거 |
| Preloader | 수요 발생 시 모델을 미리 메모리에 올리는 컴포넌트 |
| EMERGENCY_BONUS | 250s 초과 대기 요청에 부여하는 우선순위 가산점 (300,000ms) |
| is_standby | Scale-In된 서버 플래그 (라우팅 제외, 모델은 아직 로드될 수 있음) |
| demand_counter | ZSET 대기열 길이 (queued only, processing 미포함) |
| sample_count | AIMD 학습에 사용된 측정 횟수 (evict 시 0 리셋) |

---

## Part 2 — 전체 검수 항목

> 각 항목을 ✅ / ⚠️ / ❌ 로 평가하고 이유를 기록하세요.

### A. 목표 및 구조

**A-1** 3단계 목표(극한 활용 → 처리율 최대화 → 하드웨어 보호)의 충돌 시 우선순위가 명시되어 있는가?
> 레벨3(하드웨어 보호) > 레벨2(처리율) > 레벨1(극한 활용) 순이어야 한다.

**A-2** 단일 서버 환경에서 Scale-Out이 no-op일 때 Placement Planner가 에러 없이 동작하는가?
> 후보 서버가 0개이면 Preloader를 호출하지 않고 조용히 스킵해야 한다.

**A-3** AIMD의 max_concurrent 상한이 `provider.num_parallel`인데, 서버마다 num_parallel이 다를 수 있다. 이것이 명시적으로 서술되어 있는가?

---

### B. AIMD — 동시 요청 수 자율 학습

**B-1** Cold Start = num_parallel 정책 변경이 파괴적 변경임이 Phase 8에 명시되어 있는가?
> `initial_max_concurrent()` / `weight_based_max_concurrent()` 함수 **삭제** 명시 확인.

**B-2** AIMD 증가/감소 임계값이 명확한가?
```
증가: TPS ratio ≥ 0.9  → max_concurrent + 1
감소: TPS ratio < 0.7  → max_concurrent × 3/4
감소: p95 > baseline_p95 × 2 (spike) → max_concurrent × 3/4
상한: provider.num_parallel
```

**B-3** LLM 보정(증가 방향에만 최대 +2)이 AIMD **이후** 같은 30s 루프에서 실행된다. AIMD 감쇄가 발생한 사이클에서 LLM 상향이 금지되어 경계값 진동이 방지되는가?

**B-4** OOM 이중 보정(`safety_permil +50` AND `max_concurrent ×3/4` 동시 적용)의 회복 속도가 불균형하다:
- safety_permil 회복: `-10/30s` → 5 사이클 = 2.5분
- max_concurrent 회복: `+1/30s` AIMD 재개
> 이 불균형으로 인한 저활용 기간이 SDD에서 허용된 설계인가?

**B-5** `baseline_tps`는 첫 샘플에서 설정되고, ratio ≥ 0.9일 때만 상향 갱신된다. 이 동작이 ClickHouse `inference_events` 기반으로 올바르게 계산되는가?

---

### C. 스케줄링 — FIFO + Locality + Age + Tier

**C-1** enqueue score 공식:
```
paid:     score = now_ms - 300,000
standard: score = now_ms - 100,000
test:     score = now_ms - 0
낮을수록 먼저 처리 (ZRANGEBYSCORE ASC)
```
> paid가 가장 낮은 score → 가장 먼저 처리됨을 확인하라.

**C-2** EMERGENCY_BONUS 적용 후 검증:
```
신규 paid (wait=1s):       final ≈ T_now - 300,000 - ~250
standard (wait=251s):      final = T_old - 300,000 - 62,750  ← standard가 더 낮음 = 우선
```
> 250s 넘긴 standard가 신규 paid보다 반드시 앞선다는 것을 수식으로 직접 확인하라.

**C-3** K=20 윈도우 동적 확장 조건:
```
ZSET 크기 > 60 → K = min(ZSET_size / 3, 100)
```
> 윈도우 밖의 job은 다음 사이클에서 age_bonus 누적으로 자연히 진입한다.

**C-4** demand_counter DECR이 두 경로에서 모두 원자적으로 호출되는가?
```
dispatch 경로:  Lua handoff 스크립트 (ZREM + LPUSH + DECR)
cancel 경로:    Lua queued cancel 스크립트 (ZREM + DECR)
```
> ZREM 반환 0 = 이미 dispatch됨 → DECR 스킵 (이중 차감 방지).

**C-5** MAX_QUEUE_SIZE = 10,000 초과 시 429 반환. enqueue 시 ZCARD로 체크하는가?

---

### D. Thermal — 5상태 머신

**D-1** 다음 전이가 모두 정의되어 있는가:
```
Normal  →[≥82°C]→ Soft
Soft    →[≥90°C]→ Hard
Hard    →[즉시]→  Cooldown
Cooldown→[300s]→ RampUp
RampUp  →[학습값 도달]→ Normal
RampUp  →[≥90°C]→ Hard  (재진입)
Soft    →[<80°C AND active==0]→ Normal
```
> **미정의 전이 확인**: Cooldown 중 온도 재상승 시 Hard 재진입 전이가 있는가?
> **미정의 전이 확인**: RampUp에서 82~89°C 구간 진입 시 Soft 전이가 있는가?

**D-2** Soft Gate 해제 복합 조건:
```
temp < 80°C  AND  active_count == 0
```
> 조건 하나만으로는 불충분한 이유를 이해했는가?
> (temp < 80°C만이면: 82°C→79°C→82°C 진동. active==0만이면: 82°C 상태에서 요청 없으면 해제됨)

**D-3** Hard Gate cooldown이 기존 코드(7,200s)에서 300s로 변경되었다. Phase 2 구현 시 기존 cooldown 값을 덮어쓰는가?
> 300s 근거: 하드웨어 클럭 복구 + 센서 안정화에 최소 수분 필요. 60s 재개 시 throttling 무한 루프 위험.

**D-4** Circuit Breaker와 Thermal 동시 활성화 시 해제 순서:
```
Thermal cooldown 완료 → CB half-open → 탐색 요청 성공 → CB 완전 해제
```
> 이 순서가 반대이면 (CB 먼저 해제 시도) Thermal이 아직 Hot인 상태에서 CB 탐색 요청이 나가 온도를 더 올릴 수 있다.

**D-5** Soft Gate 장기 스트림 우회 방지:
> `distributed.md`의 SSE 600s hard timeout이 in-flight 스트림을 강제 종료한다. 이 연동이 SDD에 명시되어 있는가?

---

### E. 3-State 모델 생명주기

**E-1** Lazy Eviction 발화 조건:
```
APU: ② active_requests == 0  AND  ③ idle ≥ 180s (standby: idle ≥ 30s)
일반: ① 다른 모델 VRAM 필요  AND  ②③
```
> APU에서 ①이 없어도 ②③만으로 발화하는 이유: APU는 VRAM이 시스템 RAM과 통합이므로 모델 간 VRAM 경쟁이 희박하다.

**E-2** Preloader 3회 실패 후 300s 제외 타이머:
> `preload_failed_at: AtomicU64` 필드가 Phase 7 ModelState에 추가됨.
> `now_ms - preload_failed_at < 300_000` 조건으로 `filter_candidates()`에서 제외.
> 이 필드가 VramPool 구조체 정의에 포함되어 있는가?

---

### F. 모델 Pull — 드레인

**F-1** Pull 3단계 흐름 확인:
```
1. Drain:  active_count == 0 대기 (max 60s, timeout 시 강제 진행)
2. Pull:   ollama pull (max 4h, heartbeat 300s)
3. 재개:   is_pulling=false, is_loaded=false
           sample_count=0, learning_epoch_started_at=now_ms, baseline_tps=0, baseline_p95_ms=0
           → Preloader 자동 재로드 (Cold Start 재시작 + 새 epoch 기준 재학습)
```

**F-2** Drain 강제 진행 시 in-flight SSE 종료 시퀀스:
```
Job Runner cancel 신호
→ data: {"error":{"type":"service_update","message":"model pull in progress"}}\n\n
→ DB: status="failed", failure_reason="drain_forced"
```
> 이미 200 OK + SSE 헤더가 전송됐으므로 503 반환 불가. 이 제약이 SDD에 명시되어 있는가?

**F-3** Pull API 권한 — 두 엔드포인트 모두 JWT admin 이상 필요:
```
POST   /v1/ollama/models/pull              → RequireAdmin
DELETE /v1/ollama/models/pull/{pid}/{model} → RequireAdmin
```
> 일반 API 키로 50GB 모델 pull 트리거 시 서버 4시간 마비 가능 → 권한 검증 필수.

---

### G. 취소·타임아웃 계약

**G-1** queued vs processing 두 경로가 완전히 분리되어 있는가:
```
queued:     Lua ZREM + DECR demand  (ZREM=0이면 processing 경로로 전환)
processing: cancel() + LREM processing + VramPermit drop
```

**G-2** 취소 사유별 DB status 매핑 확인:
```
client disconnect  → "cancelled"
max_queue_wait 초과 → "failed"
pull drain 강제    → "failed", failure_reason="drain_forced"
수동 취소 API      → "cancelled"
```

**G-3** k8s 멀티인스턴스 환경에서 인스턴스 A(cancel)와 인스턴스 B(dispatch)가 동시에 같은 job을 처리하려 할 때 Lua ZREM 원자성으로 하나만 성공한다. 이 경쟁 조건이 명시되어 있는가?

---

### H. Gateway Intelligence — Placement Planner

**H-1** Planner 5단계 루프 순서 (2-pass 구조):
```
[Pass 0] candidate_servers_for_scale_out() 사전 계산 (read-only, 상태 변경 없음)
         scale_out_needed 모델 집합 계산 (standby 서버 제외한 현재 total_capacity 기준)
④ STANDBY 복귀  조건 A: 해당 서버의 is_loaded 모델 중 demand>0 존재 (즉시 서빙)
                조건 B: Pass 0 결과에서 해당 서버가 scale_out_candidates에 포함됨
                → 만족 시 is_standby=false
① Scale-Out    scale_out_needed × Pass 0 candidates → best_server 선정
               (④에서 복귀 처리 완료 → total_capacity에 반영됨)
② Preload      demand > 0, !is_loaded, !is_preloading, has_room, preload_failed_at 300s 경과 → Preloader
③ Evict        demand == 0, is_loaded, active == 0, idle ≥ 180s → evict
⑤ Scale-In     서버 전체 idle, !last_server → is_standby=true
```
> Pass 0의 핵심: ④가 ①의 계산 결과를 참조하려면 선행 계산이 필요. 2-pass로 자기참조 제거.

**H-2** 같은 사이클 내 충돌 방지 규칙 확인:
```
①에서 선정된 서버 → ⑤ Scale-In 제외 (같은 사이클)
④에서 복귀한 서버 → ⑤ 스킵 (30s transition guard)
Thermal soft/hard → ①② 스킵
is_pulling 모델   → ①②③④⑤ 전체 제외 (단, ③은 다른 모델에는 허용)
```

**H-3** Scale-Out 후 hold-down 60s의 목적:
> Preload 완료 → 큐 소진 → Scale-In → Scale-Out → ... 진동 방지.

**H-4** `best_server = argmax(free_vram, !is_loaded && !is_standby)` 동점 처리 방법이 정의되어 있는가?

---

### I. 전체 데이터 흐름

**I-1** startup 순서가 반드시 이 순서여야 하는 이유:
```
① sync_providers_once()       ← is_loaded 상태 먼저 확보
② recover_processing_queue()  ← ①이 완료된 후 ZSET 재투입
③ resync_demand_counters()    ← ②가 완료된 후 ZSET 실측 집계
④ spawn 루프들
```
> ③이 ② 전에 실행되면 recover된 job들이 demand에 누락됨 → Scale-Out 미트리거.

**I-2** Lua atomic handoff의 원자성 보장:
```
ZREM queue:zset {job_id}   ← 실패(0)이면 즉시 재시도
LPUSH processing {job_id}
DECR demand:{model}
```
> gap 없음: ZSET에서 사라진 job은 반드시 processing에 존재한다.

---

### J. 보안

**J-1** 신규 Admin API 권한 수준 전체 확인:
```
PATCH  /v1/providers/{id}/thermal-thresholds   → RequireAdmin ✓
POST   /v1/ollama/models/pull                  → RequireAdmin ✓
DELETE /v1/ollama/models/pull/{pid}/{model}    → RequireAdmin ✓
PATCH  /v1/providers/{id}/selected-models/{m}  → RequireAdmin ✓
```

**J-2** ThermalThresholds 입력값 검증 범위 확인:
```
normal_below < soft_at < hard_at  (순서 위반 시 422)
normal_below ≥ 50.0, hard_at ≤ 100.0
cooldown_secs ≤ 3600
```

**J-3** MAX_QUEUE_SIZE = 10,000. 없으면 Valkey ZSET 무제한 성장 → OOM.

**J-4** at-least-once 재실행 시 TPM 이중 차감 방지:
> `reserved_tokens`가 DB에 이미 있으면 TPM 재차감 스킵. 명시되어 있는가?

**J-5** `X-Job-Id` 응답 헤더로 job_id 노출:
> 클라이언트가 중복 응답 여부를 판단할 수 있도록. 명시되어 있는가?

---

### K. 파괴적 변경 확인

**K-1** Phase 8에서 제거되는 함수 명시 확인:
```rust
// 삭제 대상 — Phase 8에서 provider.num_parallel로 교체
fn initial_max_concurrent(weight_mb: u32) -> u32
fn weight_based_max_concurrent(weight_mb: u32) -> u32
```

**K-2** 기존 3-queue LIST에서 단일 ZSET으로 교체 시 마이그레이션 계획이 있는가?
> 전환 시점에 기존 큐에 잔류한 job 처리 방법.

---

### L. 최종 검증 질문

이 질문에 모두 답할 수 있으면 SDD를 충분히 이해한 것입니다:

1. qwen3:30b(18GB) 모델 요청이 갑자기 폭주했다. Veronex가 자동으로 무엇을 하는가?
2. 서버 온도가 30초 만에 75°C → 91°C로 급등했다. 어떤 순서로 무슨 일이 발생하는가?
3. Veronex 인스턴스 2개가 동시에 같은 job을 처리하려 할 때 어떻게 중복을 막는가?
4. Veronex가 크래시 후 재시작됐다. in-flight SSE job은 어떻게 되는가?
5. 어드민이 `POST /v1/ollama/models/pull`을 호출했다. 현재 streaming 중인 사용자에게 무슨 일이 생기는가?

---

## Part 3 — 리뷰 요청 프롬프트

> **아래 텍스트를 새 AI 세션에 그대로 붙여넣어 리뷰를 요청하세요.**

---

```
당신은 소프트웨어 아키텍처 전문가입니다.
아래 SDD(Software Design Document)를 처음부터 끝까지 읽고 전체를 검수해 주세요.

## 검수 기준
각 항목을 다음 중 하나로 평가하세요:
- ✅ sound         — 논리적으로 완결, 구현 가능
- ⚠️ needs improvement — 동작하지만 개선 여지 있음, 이유와 수정 방향 제시
- ❌ design flaw   — 구현 시 실제 결함 발생, 반드시 수정 필요

## 검수 영역
아래 8개 영역을 빠짐없이 검토하세요.

**1. AIMD — 동시 요청 수 학습**
- Cold Start = num_parallel (top-down): APU에서 OOM 보호가 try_reserve + safety_permil로 독립 처리되므로 안전한가?
- evict 시 sample_count=0 리셋: 재로드 후 환경이 달라질 수 있어 합리적인가?
- AIMD → LLM 보정(증가 방향에만 최대 +2) 동일 루프 적용: AIMD 감쇄 사이클에서 LLM 상향 금지가 적용되는가?
- TPS ratio < 0.7 → ×3/4 감쇄: APU 대역폭 포화 대응으로 충분한가?
- OOM 이중 보정(safety_permil +50 AND max_concurrent ×3/4): 과보수화로 저활용 가능성은?

**2. 스케줄링 — FIFO + Locality + Age + Tier**
- EMERGENCY_BONUS(250s 이후 적용): paid 연속 유입 시 standard 기아를 실제로 방지하는가? 수식으로 검증하라.
- perf_factor × age_bonus: 과열 서버에서 모델 전환 억제 — tier와의 상호작용은 sound한가?
- K=20 동적 확장(최대 100): 고부하 혼합 tier 환경에서 공정성이 충분한가?
- demand_counter: cancel/timeout 경로에서 DECR이 누락되면 영구 drift 가능한가?
- MAX_QUEUE_SIZE=10,000: 초과 시 429 반환이 필요한가?

**3. Thermal — 5상태 머신**
- 모든 상태 전이가 정의되어 있는가? (Cooldown→Hard 재진입, RampUp→Soft 미정의 여부 확인)
- Soft Gate 히스테리시스(진입 82°C / 해제 <80°C AND active==0): 30s 샘플링 주기에 충분한가?
- Soft Gate 장기 스트림 우회: SSE 600s timeout 연동이 명시되어 있는가?
- Hard Gate cooldown 기본값이 기존 코드(7,200s)에서 300s로 변경됨 — 근거가 명시되어 있는가?
- Hard Gate cooldown timer 시작점: `active_count==0` 또는 60s drain 상한 경과 중 더 이른 시점. 즉시 카운트 시작은 실질 냉각 보장 불가.
- RampUp +1/30s: 이전 학습값 8 기준 3.5분 복구 — 생산 트래픽 영향은?

**4. Placement Planner — Gateway 지능**
- ①~⑤ 루프 순서: 같은 사이클 내 충돌(①↔⑤, ④→⑤) 방지 규칙이 완전한가?
- Scale-Out Dedup: preloading 서버가 Soft/Hard 진입 시 다른 서버 Scale-Out 허용이 올바른가?
- Scale-Out argmax(free_vram) 동점 처리: 정의되어 있는가?
- 단일 서버 환경(no-op): Scale-Out 후보 0개일 때 에러 없이 스킵하는가?

**5. 모델 Pull 드레인**
- Drain timeout 60s 강제 진행 시: in-flight SSE 종료 시퀀스가 명시되어 있는가?
- 강제 진행 후 DB job status = "failed", failure_reason = "drain_forced"가 명시되어 있는가?
- Pull 완료 후 VRAM 부족 시 Preloader 실패 → 3회 재시도 → 300s 제외가 올바르게 동작하는가?

**6. APU 환경**
- safety_permil 기본 10%(12GB/120GB): k8s 환경에서 타이트하지 않은가?
- node-exporter 실측값으로 APU VRAM을 추정하는 방식이 30s 갱신 주기에 충분한가?

**7. 취소·타임아웃 계약**
- queued cancel Lua(ZREM+DECR)와 processing cancel(cancel()+LREM)이 완전히 분리되어 있는가?
- k8s 멀티인스턴스에서 cancel vs dispatch 경쟁 조건이 Lua 원자성으로 해결되는가?
- startup 순서(sync→recover→resync→spawn)가 반드시 이 순서여야 하는 이유가 명시되어 있는가?

**8. 보안**
- 신규 Admin API 4개 모두 RequireAdmin이 명시되어 있는가?
- ThermalThresholds 입력값 검증(범위 + 순서)이 있는가?
- at-least-once 재실행 시 TPM 이중 차감이 방지되는가?
- Phase 8 파괴적 변경(함수 삭제)이 명시되어 있는가?

## 추가 검토
위 8개 영역 외에 설계상 누락되거나 모순된 부분이 있으면 자유롭게 지적해 주세요.

## SDD 전문
[아래에 scheduler.md 전문을 붙여넣으세요]
```
