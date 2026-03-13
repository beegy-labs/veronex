# Intelligence Serving — Scheduler SDD

> **Status**: Implemented | **Last Updated**: 2026-03-12
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
> `docs/llm/inference/capacity.md`가 이 Cold Start 정책을 반영한다.

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

## 구현 상태

> **완료**: 전체 구현 + gap 보정 완료 (2026-03-13)

### 핵심 구현 현황

| 항목 | 구현 위치 | 비고 |
|------|----------|------|
| ZSET 큐 + Lua atomic scripts | `application/ports/outbound/valkey_port.rs` | enqueue/dispatch/cancel 3종 스크립트 |
| MAX_QUEUE_SIZE = 10,000 / PER_MODEL = 2,000 | `domain/constants.rs` | 원자 체크 (Lua 단일 스크립트) |
| Queue Full 시 DB 고아 방지 | `use_case.rs` | `Ok(false)` → jobs 정리 + DB status=Failed |
| AIMD (sample≥3, ratio<0.7/≥0.9, p95 spike) | `capacity/analyzer.rs` | baseline freeze on decay, 3-cycle stable update |
| LLM 보정 (stable_cycles≥3, **증가 전용**, +2 상한) | `capacity/analyzer.rs` | `change_floor = current` — 감소 불가 |
| OOM 대응 (safety_permil +50, max_concurrent ×3/4) | `capacity/vram_pool.rs` + `analyzer.rs` | -10 점진 회복, 최대 500 |
| Thermal 5-state machine | `capacity/thermal.rs` | Normal/Soft/Hard/Cooldown/RampUp |
| Soft Gate 해제 조건 | `capacity/thermal.rs` | `temp < normal_below AND active_count == 0` |
| Hard Gate 60s forced drain | `ThermalDrainPort` + `ThermalDrainAdapter` | `placement_planner.rs` 60s 경과 시 cancel |
| Hard Gate 90s watchdog | `placement_planner.rs` | 90s 초과 시 에러 로그 + 강제 타이머 설정 |
| Cooldown 300s + max 900s 상한 + 온도 분기 | `capacity/thermal.rs` | cooldown_secs × 3 절대 상한 |
| RampUp max_concurrent=1 + 요청 수용 | `inference/dispatcher.rs` | Soft와 달리 503 차단 없음 |
| **RampUp → Normal: Σmax_concurrent ≥ pre_hard_total** | `capacity/thermal.rs` | Hard 진입 시 스냅샷 저장, 복원 확인 후 전이 |
| Placement Planner 5s loop (Pass 0 + ④①②③⑤) | `placement_planner.rs` | provisional_free, Scale-Out 결정 락 |
| Preload 3-fail 300s 제외 | `capacity/vram_pool.rs` + dispatcher + planner | filter_candidates + ①② 모두 체크 |
| Lazy Eviction (idle 180s / standby 30s) | `placement_planner.rs` | ③ Evict |
| promote_overdue (30s) | `queue_maintenance.rs` | HSCAN + ZADD XX EMERGENCY_BONUS |
| demand_resync (60s) | `queue_maintenance.rs` | ZSCAN → HMGET → 재산정 |
| Multi-instance pub/sub + reaper | `pubsub/relay.rs`, `pubsub/reaper.rs` | 크래시 복구, 고아 job 재큐 |
| provider_vram_budget 영속화 | `provider_vram_budget_repository` | safety_permil DB 저장 |
| Startup recovery | `use_case.rs::recover_pending_jobs()` | LRANGE → DB 체크 → Lua 원자 ZADD + LREM |

### 설계 단순화 항목 (Accepted Simplifications)

다음 항목은 동작에 이상이 없으나 SDD 원문과 구현이 의도적으로 다르다.

#### G11 — eligible_capacity에 governor_cap 미반영

**현재 구현**: `placement_planner.rs`의 eligible_capacity 계산 시 governor가 활성 상태인 서버도 raw `max_concurrent` 사용.

**허용 이유**:
- governor_cap이 0 (dispatch_blocked)이면 `is_dispatch_blocked()` 필터에서 이미 제외됨 (line 191).
- governor_cap이 1~N (fair-share 제한)이면 실제 dispatch 처리량이 다소 낮게 평가되어 Scale-Out이 **조금 더 일찍 발동**될 수 있음.
- Scale-Out 조기 발동은 Scale-Out 억제보다 안전한 방향의 오차 (보수적). 단일 서버 환경에서는 no-op이므로 실질 영향 없음.

**향후 정밀화 조건**: 멀티 서버 환경에서 governor가 상시 활성화되어 Scale-Out 진동이 관측될 경우 `governor_cap`을 eligible_capacity에 반영한다.

```rust
// 현재 (단순화)
let mc = vram_pool.max_concurrent(p.id, &model);
// 정밀화 시
let cap = vram_pool.governor_cap(p.id, &model);
let mc  = if cap > 0 { cap } else { vram_pool.max_concurrent(p.id, &model) };
```

#### G12 — Thermal Soft/Hard 진입 시 Preload 태스크 정리 미구현

**현재 구현**: Soft/Hard 진입 시 진행 중인 Preloader 태스크를 명시적으로 취소하지 않음.

**허용 이유**:
- Placement Planner가 5s 루프마다 thermal 상태를 확인하고 Soft/Hard/Cooldown인 서버에 대해 ①② (Scale-Out, Preload) 단계를 **완전히 스킵**함 (thermal 연동 규칙 참고).
- 실행 중인 Preloader 태스크는 자체 타임아웃(120s) 또는 성공/실패로 자연 종료됨. 종료 시 `is_preloading=false` + NX 락 DEL이 항상 수행됨.
- 최대 120s 동안 Ollama에 preload HTTP 요청이 남아 있을 수 있으나, thermal Hard 상태에서 Ollama가 부하를 받는 것은 in-flight 요청이 이미 있는 상황과 동일 — VramPermit은 preload 중 발급되지 않으므로 추가 VRAM 오용 없음.

**향후 구현 조건**: Preloader가 Soft/Hard 진입에도 Ollama 부하를 유발하는 것이 실측으로 확인될 경우, placement_planner에 `active_preload_tokens: HashMap<(Uuid, String), CancellationToken>` 구조를 추가해 즉시 취소 구현.

---

## TDD — 테스트 전략

> 정책 원문: `docs/llm/policies/testing-strategy.md`
> 방법론: Testing Trophy + Contract Testing. **Integration 중심, 중복 배제, 레이어별 책임 분리.**

### 레이어별 테스트 책임

| Layer | 검증 대상 | 도구 | Anti-Pattern |
|-------|----------|------|-------------|
| **Static** | Types, lint | Rust 타입 시스템, Clippy | 타입으로 잡히는 건 테스트 안 씀 |
| **Unit** | 순수 함수 로직 | `cargo nextest`, `proptest` | HTTP/DB 검증 금지 |
| **Integration** | API 계약 (schema), port 계약 | mock port, OpenAPI 검증 | E2E와 같은 경로 중복 금지 |
| **E2E** | 사용자 흐름 | bash e2e | 개별 함수 검증 금지 |

**테스트 순수성 원칙**: 내부 함수 수정 → unit만 깨짐 → E2E 불변.
E2E가 내부 함수 변경에 깨지면 → **테스트 설계 결함** (레이어 침범).

### 테스트 작성 결정 체크리스트

```
1. 타입으로 잡히나?             → Yes → 테스트 불필요 (Rust 타입 시스템 신뢰)
2. 순수 함수인가?               → Yes → Unit (proptest 우선)
   ex. window_score(), perf_factor(), thermal 상태 전이 로직
3. 외부 의존성? (Valkey/DB)     → Yes → Integration (mock port 사용)
   ex. Lua atomic 스크립트, Placement Planner port 계약, ThermalDrainPort
4. 사용자 흐름?                 → Yes → E2E (최소한만)
   ex. end-to-end inference, cancel 흐름, queue wait timeout
5. 다른 레이어에서 검증?        → Yes → 작성하지 않음
```

### Scheduler 컴포넌트별 테스트 범위

**Unit (`cargo nextest` + `proptest`)**:

| 컴포넌트 | 테스트 대상 | 도구 |
|----------|------------|------|
| `thermal.rs` | 상태 머신 전이 (Normal→Soft→Hard→Cooldown→RampUp), 경계값 | proptest |
| `dispatcher.rs` | `window_score()`, `filter_candidates()` 후보 필터 | proptest |
| `valkey_keys.rs` | key 생성 함수 정확성 | cargo nextest |
| AIMD 계산 | TPS ratio, p95 spike 감쇄 조건, LLM 보정 방향 제약 | proptest |
| `perf_factor()` | 온도 구간별 선형 보간 (0.0~1.0) | proptest |

**Integration (mock port)**:

| 컴포넌트 | 테스트 대상 |
|----------|------------|
| `ThermalDrainPort` | Hard Gate 60s 초과 시 `cancel_jobs_for_provider()` 호출 검증 |
| `VramPool` | `try_reserve()` / `release()` 원자성, standby 플래그 격리 |
| Placement Planner | `ThermalDrainPort` mock으로 drain 계약 검증 |
| Lua 스크립트 | enqueue/dispatch/cancel atomic handoff 계약 (Valkey testcontainer) |

**E2E (bash scripts)**:

| 스크립트 | 검증 흐름 |
|----------|----------|
| `02-inference.sh` | queued → processing → completed 기본 흐름 |
| `04-security.sh` | admin-only pull drain API 권한 |
| `06-lifecycle.sh` | job cancel, queue wait timeout 흐름 |

**cargo-mutants**: 릴리스 전 1회. Thermal 상태 머신 + AIMD 핵심 로직 우선 감사.

### 테스트 금지 패턴

```
✗ Thermal 상태 변경에 E2E가 깨짐 → 레이어 침범 (unit 책임)
✗ is_loaded / active_count DB 직접 검증 → unit/integration 책임
✗ Lua 스크립트 로직을 Rust mock으로 대체 → 계약 공동화 (Valkey testcontainer 사용)
✗ window_score()를 E2E에서 검증 → 순수 함수이므로 unit 책임
```
