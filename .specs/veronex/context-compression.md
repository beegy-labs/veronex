# Context Compression SDD

> **Status**: Draft | **Last Updated**: 2026-04-02
> **Scope**: S14 — Per-turn context compression for resource-constrained deployments
> **Branch**: TBD

---

## Premise

This system targets **individuals and small teams running Ollama on personal hardware** — not cloud LLM providers. Resources are finite:

- Single GPU or shared VRAM across models
- Small models (3B–13B) with 4K–16K context windows
- No budget for dedicated infrastructure per feature

The goal is **maximum token efficiency without compromise**: every completed turn is compressed immediately so future turns always read compact context, regardless of conversation length.

This is not optional behavior. Long conversations on small models degrade or fail entirely without it.

---

## Problem Statement

```
Without compression (8K model, avg 500 tokens/turn):

Turn 10 context assembly:
  system:       200 tokens
  turns 1–9:  4,500 tokens  (9 × 500)
  current:      800 tokens
  ─────────────────────────
  total:      5,500 tokens → exceeds 60% budget (4,915) → quality degrades

With per-turn compression (avg 100 tokens/turn compressed):

Turn 10 context assembly:
  system:       200 tokens
  turns 1–9:    900 tokens  (9 × 100)
  current:      800 tokens
  ─────────────────────────
  total:      1,900 tokens → 38% of budget → fast inference, full quality
```

---

## Multi-Turn Eligibility Gate

Multi-turn conversation (`messages.len() > 1` or `conversation_id` present) is gated
on **two hard conditions** evaluated at handler entry. Both must pass.

### Condition 1 — Model parameter count

```
multiturn_min_params (lab_settings, default: 7)

Sources (research-backed):
  - 7B: basic multi-turn + compression reliable
  - 13B: recommended for compression quality
  - <7B: 25%p accuracy drop in multi-turn benchmarks, compression output unreliable

Derivation: parse model name tag  →  "qwen2.5:7b" → 7.0B
Unknown size (no tag): FAIL OPEN — allow multi-turn
```

### Condition 2 — Context window (max_ctx)

```
multiturn_min_ctx (lab_settings, default: 16_384)

Sources (research-backed):
  8K  (8,192):  핸드오프 과다. 압축 후에도 20턴마다 리셋 필요. 비실용적.
  16K (16,384): 압축 적용 시 40~60턴 지원. 현실적 최소.
  32K (32,768): 프로덕션 권장. 20~30턴 raw, 80+턴 압축.
  128K+:        이상적. 압축 없이도 수백 턴 가능.

Critical: Ollama 기본 num_ctx = 2048 (전 모델 동일).
          configured_ctx가 아닌 max_ctx (모델 아키텍처 최대값) 기준.
          max_ctx는 Valkey veronex:ollama:ctx:{provider_id}:{model} 에서 읽음.
          (방금 구현된 Valkey 컨텍스트 캐시 활용)

Unknown max_ctx (Valkey 미스): FAIL OPEN — allow multi-turn
```

### Condition 3 — Model allowlist (optional)

```
multiturn_allowed_models (lab_settings, default: [] = 모든 모델 허용)

관리자가 특정 모델만 multi-turn 허용하도록 명시적 화이트리스트 설정.
비어 있으면 Condition 1+2만 적용.
설정 시 allowlist에 없으면 Condition 1+2 통과해도 차단.

예시: ["qwen2.5:7b", "qwen2.5:14b", "mistral:7b"]
```

### 판단 로직

```rust
fn check_multiturn_eligibility(
    model_name: &str,
    max_ctx: Option<u32>,          // Valkey에서 읽은 값
    lab: &LabSettings,
) -> Result<(), MultiTurnError> {

    // 1. 모델 사이즈 체크
    if let Some(params) = model_param_billions(model_name) {
        if params < lab.multiturn_min_params as f32 {
            return Err(MultiTurnError::ModelTooSmall {
                actual: params,
                required: lab.multiturn_min_params,
            });
        }
    }
    // unknown size → pass (fail open)

    // 2. 컨텍스트 윈도우 체크
    if let Some(ctx) = max_ctx {
        if ctx < lab.multiturn_min_ctx as u32 {
            return Err(MultiTurnError::ContextTooSmall {
                actual: ctx,
                required: lab.multiturn_min_ctx,
            });
        }
    }
    // unknown max_ctx → pass (fail open)

    // 3. 화이트리스트 체크
    if !lab.multiturn_allowed_models.is_empty() {
        if !lab.multiturn_allowed_models.contains(&model_name.to_string()) {
            return Err(MultiTurnError::ModelNotAllowed {
                model: model_name.to_string(),
            });
        }
    }

    Ok(())
}
```

### API 에러 응답

```json
// Condition 1 실패
{
  "error": {
    "message": "multi-turn conversation requires a 7B+ model (this model: 3B).",
    "type": "invalid_request_error",
    "code": "model_too_small"
  }
}

// Condition 2 실패
{
  "error": {
    "message": "multi-turn conversation requires 16K+ context window (this model: 8192 tokens).",
    "type": "invalid_request_error",
    "code": "context_too_small"
  }
}

// Condition 3 실패
{
  "error": {
    "message": "model 'llama3.2:3b' is not in the multi-turn allowlist.",
    "type": "invalid_request_error",
    "code": "model_not_allowed"
  }
}
```

---

## Design Principles

1. **Multi-turn is gated at 7B** — models below threshold receive 400 at handler entry. No fallback, no silent degradation.
2. **Compress every turn eagerly** — not threshold-based. Every completed Q&A pair is compressed immediately after it is saved to S3.
3. **Read compressed, write raw** — S3 stores both. Raw is preserved for audit/training. Inference always prefers compressed.
4. **No new infrastructure** — no Kafka, no dedicated service. `tokio::spawn` is sufficient for N≤3 instances; Kafka becomes an option only at larger scale.
5. **Graceful fallback always** — if compression is unavailable, not ready, or times out, inference proceeds with raw context. Never block the hot path.
6. **CompressionRouter owns routing** — N=1 vs N≥2 policy is encapsulated; caller does not branch.
7. **Context budget is enforced** — assembly rejects context exceeding `configured_ctx × BUDGET_RATIO` regardless of source.
8. **Long input is also compressed** — if the current turn's input itself exceeds the input budget, it is compressed before being sent to the model.

---

## Context Budget

Budget ratios are derived from `configured_ctx` (real Ollama value from Valkey cache).
Manual override available via `lab_settings.context_budget_override`.

```rust
fn budget_ratios(configured_ctx: u32) -> BudgetRatios {
    match configured_ctx {
        // 소형 (≤8K): 컨텍스트 품질 민감 — 보수적
        ..=8_192 => BudgetRatios {
            compression_start:  0.50,  // 50% 넘으면 eager 압축 활성
            inference_budget:   0.60,  // 추론 입력 하드 한계
            handoff_threshold:  0.70,  // 마스터 요약 + 새 세션 트리거
        },
        // 중형 (≤32K)
        ..=32_768 => BudgetRatios {
            compression_start:  0.55,
            inference_budget:   0.65,
            handoff_threshold:  0.75,
        },
        // 대형 (>32K)
        _ => BudgetRatios {
            compression_start:  0.60,
            inference_budget:   0.70,
            handoff_threshold:  0.80,
        },
    }
}
```

```
max_ctx 구간별 역할:

0% ──── 50% ──── 65% ──── 75% ──── 85% ──── 100%
│       │        │         │         │
안전     압축시작  입력한계   핸드오프   절대금지
구간     트리거    (BUDGET)  트리거
```

| 모델 | ctx | 압축 시작 | 입력 한계 | 핸드오프 |
|------|-----|----------|----------|---------|
| 3B (4K) | 4,096 | 2,048t | 2,457t | 2,867t |
| 7B (8K) | 8,192 | 4,096t | 4,915t | 5,734t |
| 13B (16K) | 16,384 | 9,011t | 10,650t | 12,288t |
| 70B (128K) | 131,072 | 78,643t | 91,750t | 104,858t |

```
BUDGET_TOKENS = configured_ctx × inference_budget
OUTPUT_RESERVE = configured_ctx × (1.0 - inference_budget)   // 최소 생성 공간

예산 배분 (Turn N+1):
  system_prompt:        고정 (요청에서 읽음)
  compressed_history:   BUDGET - system - current_input
  current_input:        압축 전 측정, input_budget 초과 시 압축
```

---

## Conceptual Model

```
Task (InferenceJob)
─────────────────────────────────────────────────────────
추론의 원자 단위. 모든 API 경로는 단일 Task를 생성한다.
  - /v1/inference, /api/generate, /api/chat, /v1/chat/completions
  - 이미지 포함 가능, 결과는 TurnRecord로 S3에 저장
  - 항상 단일 요청 → 단일 응답

Multi-turn (Conversation)
─────────────────────────────────────────────────────────
Task의 집합. conversation_id로 연결된 Task 시퀀스.
  - Multi-turn 자체는 Task가 아님 — Task들의 이력 조회 + 컨텍스트 조립 레이어
  - conversation_id 없으면 Multi-turn 아님 (독립 Task)
  - 압축/핸드오프는 이 레이어에서 발생

계층:
  Task (atomic)  ←  Vision pipeline, VisionAnalysis 저장
  Multi-turn     ←  Context assembly, Compression, Handoff
                     (Task들이 쌓인 후에 작동)
```

Vision 파이프라인과 VisionAnalysis 저장은 **Task 레벨** 동작 — conversation_id 유무와 무관.
압축/핸드오프는 **Multi-turn 레벨** 동작 — `conversation_id` 있을 때만 활성.

---

## Architecture

```
┌────────────────────────────────────────────────────────────┐
│                      Turn N completes                       │
│                    (runner.rs finalize)                     │
└──────────────────────────┬─────────────────────────────────┘
                           │
              ┌────────────▼─────────────┐
              │     S3 ConversationRecord │
              │  TurnRecord appended (raw)│
              └────────────┬─────────────┘
                           │
              ┌────────────▼─────────────┐
              │      CompressionRouter    │
              │   .route(providers, lab)  │
              └────────────┬─────────────┘
                           │
           ┌───────────────┼───────────────┐
           ▼               ▼               ▼
        N=1             N≥2 idle       N≥2 dedicated
     SyncInline      AsyncIdle        AsyncDedicated
           │         (spawn to       (lab: compression
           │          idle prov)      _model provider)
           │               │               │
           └───────────────┴───────────────┘
                           │
              ┌────────────▼─────────────┐
              │   compress_turn(turn)     │
              │   Ollama /api/chat call   │
              │   model: compression_model│
              │         OR infer model    │
              └────────────┬─────────────┘
                           │
              ┌────────────▼─────────────┐
              │  S3 put_conversation()    │
              │  TurnRecord.compressed    │
              │  = Some(CompressedTurn)   │
              └────────────┬─────────────┘
                           │
                    Valkey conv cache
                    invalidated (DEL)

─────────────────────────────────────────────────

Turn N+1 request arrives
           │
┌──────────▼───────────┐
│  Context assembler    │
│  load ConversationRec │
│  (Valkey → S3)        │
└──────────┬───────────┘
           │
┌──────────▼───────────────────────────────────┐
│  For each past turn:                          │
│    turn.compressed.is_some()                  │
│      → use compressed.summary (~100 tokens)   │
│    else                                       │
│      → use raw prompt+result (fallback)       │
│                                               │
│  current_input > input_budget?                │
│    → compress input inline (sync)             │
│                                               │
│  total > BUDGET_TOKENS?                       │
│    → drop oldest turns (already compressed)   │
└──────────┬───────────────────────────────────┘
           │
    Inference proceeds
```

---

## 3-Layer Compression Strategy

### Layer 1 — Per-turn eager compression (micro)
Every completed turn is compressed immediately after S3 write.
Target: ~100 tokens per turn. Stored in `TurnRecord.compressed`.

Timing optimization: compression runs via `tokio::spawn` right after Turn N response
is sent to the user. By the time Turn N+1 arrives (user read time), compression is
already done → **Call 1 is free in the normal case**.

If compression not yet ready when Turn N+1 arrives:
- N=1: run sync before inference (SyncInline)
- N≥2: run on idle provider (AsyncIdle), or skip + fallback to raw

### Layer 2 — Context assembly with budget enforcement (meso)
At Turn N+1, assembler builds messages array from compressed turns.
Hard limit: `inference_budget` ratio of `configured_ctx`.
Overflow: drop oldest turns first (already compressed, minimal loss).

### Layer 3 — Session handoff with master summary (macro)
Trigger: sum of `compressed.compressed_tokens` across all turns > `handoff_threshold × configured_ctx`.

```
[Call 1: master summary]
  입력: 모든 TurnRecord.compressed.summary 연결
  출력: 전체 대화 요약 (~300 tokens)
  → 새 conversation_id 생성
  → S3 새 ConversationRecord, turns[0] = HandoffTurn(summary)

[Call 2: 추론]
  입력: [system] + [master_summary 300t] + [current input]
  → 컨텍스트 완전 초기화, 이력은 요약에 보존

응답:
  Header: X-Conversation-ID: {new_conv_id}
  Body:   { ..., "conversation_renewed": true }
```

Lab settings:
```
handoff_enabled:    bool  (default: true)
handoff_threshold:  f32   (위 BudgetRatios.handoff_threshold 사용)
```

---

## CompressionRouter Policy

```rust
pub enum CompressionRoute {
    /// N=1, or all providers busy. Compression runs synchronously at Turn N+1 start.
    SyncInline,
    /// N≥2. Compression is spawned async to the least-loaded idle provider.
    AsyncIdle { provider_id: Uuid },
    /// N≥2 with lab.compression_model set. Always routes to designated provider.
    AsyncDedicated { provider_id: Uuid },
    /// All providers saturated (active_requests > 0) and N≥2.
    /// Turn N is marked compressed=None; retry deferred to Turn N+1 finalize.
    Skip,
}
```

Decision logic (in priority order):
1. `lab.compression_model` is set AND a provider with that model exists → `AsyncDedicated`
2. `providers.len() == 1` → `SyncInline`
3. Any provider with `active_requests == 0` → `AsyncIdle` (pick lowest VRAM usage)
4. All providers busy → `Skip`

**SyncInline timing**: runs at **Turn N+1 request arrival**, before context assembly. Never at Turn N completion (that would delay the current response).

**Skip recovery**: on every `finalize_turn`, if prior turns have `compressed == None`, retry compression for them in addition to the current turn.

---

## Data Model

### S3: `ConversationRecord` extension

```rust
// application/ports/outbound/message_store.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedTurn {
    /// Compressed Q&A summary. Target: ~100 tokens.
    pub summary: String,
    /// Token count of (prompt + result) before compression.
    pub original_tokens: u32,
    /// Token count of summary after compression.
    pub compressed_tokens: u32,
    /// Model used for compression (e.g. "qwen2.5:3b").
    pub compression_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    pub job_id: Uuid,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    pub created_at: String,
    // NEW
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compressed: Option<CompressedTurn>,
}
```

`ConversationRecord` itself is unchanged — compression state is per-turn.

### S3: `HandoffTurn`

세션 핸드오프 발생 시 새 `ConversationRecord`의 첫 번째 턴으로 저장.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffTurn {
    /// 이전 대화 전체를 요약한 마스터 요약 (~300 tokens).
    pub master_summary: String,
    /// 요약 생성에 사용된 모델.
    pub summary_model: String,
    /// 이전 conversation_id (감사/연결 추적용).
    pub previous_conversation_id: Uuid,
    /// 이전 대화의 총 턴 수.
    pub previous_turn_count: u32,
    pub created_at: String,
}

// ConversationRecord.turns 의 첫 번째 항목으로 포함 가능하도록
// TurnRecord와 동일한 직렬화 레이아웃 or 별도 enum 으로 처리:
pub enum ConversationTurn {
    Regular(TurnRecord),
    Handoff(HandoffTurn),
}
```

### DB: `lab_settings` additions

```sql
-- Migration: 000014_lab_context_compression.up.sql
ALTER TABLE lab_settings
  -- 압축
  ADD COLUMN context_compression_enabled  BOOLEAN  NOT NULL DEFAULT false,
  ADD COLUMN compression_model            TEXT,               -- NULL = reuse infer model
  ADD COLUMN context_budget_ratio         REAL     NOT NULL DEFAULT 0.60,
  ADD COLUMN compression_trigger_turns    INT      NOT NULL DEFAULT 1,
  ADD COLUMN recent_verbatim_window       INT      NOT NULL DEFAULT 1,
  ADD COLUMN compression_timeout_secs     INT      NOT NULL DEFAULT 10,
  -- 멀티턴 게이트
  ADD COLUMN multiturn_min_params         INT      NOT NULL DEFAULT 7,
  ADD COLUMN multiturn_min_ctx            INT      NOT NULL DEFAULT 16384,
  ADD COLUMN multiturn_allowed_models     TEXT[]   NOT NULL DEFAULT '{}',
  -- 비전
  ADD COLUMN vision_model                 TEXT,               -- NULL = 자동 선택
  -- 핸드오프
  ADD COLUMN handoff_enabled              BOOLEAN  NOT NULL DEFAULT true;
  -- handoff_threshold 는 BudgetRatios에서 configured_ctx 기준으로 동적 계산 (DB 저장 불필요)
```

| 필드 | 기본값 | 의미 |
|------|--------|------|
| `multiturn_min_params` | 7 | 7B 미만 → 400 |
| `multiturn_min_ctx` | 16384 | 16K 미만 max_ctx → 400 |
| `multiturn_allowed_models` | `[]` | 비어 있으면 전체 허용, 설정 시 화이트리스트 |

### LabSettings struct additions

```rust
pub struct LabSettings {
    // existing fields...

    // 압축
    pub context_compression_enabled: bool,    // false
    pub compression_model: Option<String>,    // None = infer model 재사용
    pub context_budget_ratio: f32,            // 0.60
    pub compression_trigger_turns: i32,       // 1 (매 턴)
    pub recent_verbatim_window: i32,          // 1 (마지막 1턴 raw)
    pub compression_timeout_secs: i32,        // 10

    // 멀티턴 게이트
    pub multiturn_min_params: i32,            // 7
    pub multiturn_min_ctx: i32,               // 16384
    pub multiturn_allowed_models: Vec<String>,// [] = 전체 허용

    // 비전
    pub vision_model: Option<String>,         // None = 자동 선택

    // 핸드오프
    pub handoff_enabled: bool,                // true
}
```

---

## Web UI — Multi-Turn Settings

### 설정 카드 (Lab Features 페이지)

```
┌─────────────────────────────────────────────────────────┐
│  Multi-Turn Conversation Requirements                   │
├─────────────────────────────────────────────────────────┤
│  Minimum model size           Minimum context window    │
│  ┌──────────────┐             ┌──────────────┐         │
│  │  7  B        │             │  16384  tok  │         │
│  └──────────────┘             └──────────────┘         │
│                                                         │
│  Allowed models  (empty = all qualifying models)        │
│  ┌─────────────────────────────────────────────────┐   │
│  │  qwen2.5:7b  ×   mistral:7b  ×   qwen2.5:14b × │   │
│  │  + Add model...                                  │   │
│  └─────────────────────────────────────────────────┘   │
│                                                         │
│  Compression model  (None = reuse inference model)      │
│  ┌────────────────────────────────────────────────┐    │
│  │  qwen2.5:7b                            ▼       │    │
│  └────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
```

### 모델 선택기 경고 — 3가지 케이스

모델 드롭다운 선택 시 즉시 평가. 경고 색상: `text-yellow-500`.

**Case 1 — 모델 사이즈 미달**
```
┌──────────────────────────────────────────────────────┐
│  llama3.2:3b                                 ▼       │
├──────────────────────────────────────────────────────┤
│  ⚠  3B model — below the 7B minimum.                 │
│     Multi-turn conversation will be rejected (400).   │
│     Single-turn requests work normally.               │
└──────────────────────────────────────────────────────┘
```

**Case 2 — 컨텍스트 윈도우 미달** (max_ctx < multiturn_min_ctx)
```
┌──────────────────────────────────────────────────────┐
│  some-model:7b                               ▼       │
├──────────────────────────────────────────────────────┤
│  ⚠  Context window 8K — below the 16K minimum.       │
│     Multi-turn conversation will be rejected (400).   │
└──────────────────────────────────────────────────────┘
```

**Case 3 — 화이트리스트 미포함** (allowed_models 설정 시)
```
┌──────────────────────────────────────────────────────┐
│  mistral:7b-instruct                         ▼       │
├──────────────────────────────────────────────────────┤
│  ⚠  Not in the multi-turn allowlist.                  │
│     Multi-turn conversation will be rejected (400).   │
└──────────────────────────────────────────────────────┘
```

**7B+, 16K+, 화이트리스트 통과**: 경고 없음.
**크기/ctx 불명 모델**: 경고 없음 (fail open).

### 경고 로직 (프론트엔드)

```typescript
interface MultiturnWarning {
  type: 'model_too_small' | 'context_too_small' | 'model_not_allowed'
  message: string
}

function getMultiturnWarnings(
  modelName: string,
  modelMaxCtx: number | null,        // ollama_models API에서 읽음
  lab: LabSettings,
): MultiturnWarning[] {
  const warnings: MultiturnWarning[] = []

  // 1. 모델 사이즈
  const paramMatch = modelName.match(/[:\-_](\d+\.?\d*)b/i)
  if (paramMatch) {
    const params = parseFloat(paramMatch[1])
    if (params < lab.multiturn_min_params) {
      warnings.push({
        type: 'model_too_small',
        message: `${params}B model — below the ${lab.multiturn_min_params}B minimum.`,
      })
    }
  }

  // 2. 컨텍스트 윈도우
  if (modelMaxCtx !== null && modelMaxCtx < lab.multiturn_min_ctx) {
    warnings.push({
      type: 'context_too_small',
      message: `Context window ${modelMaxCtx.toLocaleString()} — below the ${lab.multiturn_min_ctx.toLocaleString()} minimum.`,
    })
  }

  // 3. 화이트리스트
  if (lab.multiturn_allowed_models.length > 0 &&
      !lab.multiturn_allowed_models.includes(modelName)) {
    warnings.push({
      type: 'model_not_allowed',
      message: `Not in the multi-turn allowlist.`,
    })
  }

  return warnings
}
```

### 프론트엔드 구현 파일

| 파일 | 변경 |
|------|------|
| `web/lib/types.ts` | `LabSettings`에 3개 필드 추가, `MultiturnWarning` 타입 |
| `web/components/model-selector.tsx` | `getMultiturnWarnings()` 호출 → 경고 배지 |
| `web/app/providers/components/lab-tab.tsx` | Multi-Turn Requirements 설정 카드 |
| `web/lib/i18n/en.json` | `lab.multiturnMinParams`, `lab.multiturnMinCtx`, `lab.multiturnAllowedModels`, `lab.multiturnWarning.*` |
| `web/lib/i18n/ko.json` | 동일 |

`modelMaxCtx`는 `GET /v1/ollama/models` 응답에 `max_ctx` 필드 추가하여 전달.
(현재 `OllamaModelDto`에 없음 — Phase 5에서 추가)

---

## Compression Prompt

```
[system]
You are a lossless context compressor. Summarize the following conversation turn
into a single compact paragraph. Rules:
- Preserve: intent of question, key decisions, named entities, numbers, errors, code identifiers.
- Omit: filler, repetition, courtesy phrases.
- Output ONLY the summary. No preamble. No labels.
- Target: under 120 words.

[user]
Q: {turn.prompt}
A: {turn.result}
```

Input tokens: ~500–2,000 (one Q&A pair).
Output tokens: ~80–150.
Compression ratio: ~10–20x.

For long input compression (current turn input > input budget):

```
[system]
Extract only the information essential to answering a question from the following
{type: code|document|log|other}. Preserve: key identifiers, numbers, errors,
function signatures, core assertions. Omit everything else.
Target: under {budget} tokens.

[user]
{input}
```

---

## Context Assembly

```rust
// application/use_cases/inference/context_assembler.rs  (new)

pub struct ContextAssembler {
    budget_ratio: f32,
}

impl ContextAssembler {
    /// Build messages array for Turn N+1.
    /// Priority: compressed turns > raw turns > truncate oldest first.
    pub fn assemble(
        &self,
        record: &ConversationRecord,
        system_prompt: Option<&str>,
        current_input: &str,
        configured_ctx: u32,
        recent_verbatim_window: usize,
    ) -> serde_json::Value {
        let budget = (configured_ctx as f32 * self.budget_ratio) as usize;
        let n = record.turns.len();

        let mut messages = vec![];

        // 1. System prompt (fixed)
        if let Some(sys) = system_prompt {
            messages.push(json!({"role": "system", "content": sys}));
        }

        // 2. Past turns — compressed preferred, raw fallback
        //    recent_verbatim_window turns kept verbatim at end
        let verbatim_start = n.saturating_sub(recent_verbatim_window);
        for (i, turn) in record.turns.iter().enumerate() {
            if i < verbatim_start {
                if let Some(c) = &turn.compressed {
                    messages.push(json!({"role": "user", "content": c.summary}));
                } else {
                    // Fallback: raw (turn hasn't been compressed yet)
                    messages.push(json!({"role": "user",      "content": &turn.prompt}));
                    messages.push(json!({"role": "assistant", "content": turn.result.as_deref().unwrap_or("")}));
                }
            } else {
                // Verbatim window
                messages.push(json!({"role": "user",      "content": &turn.prompt}));
                messages.push(json!({"role": "assistant", "content": turn.result.as_deref().unwrap_or("")}));
            }
        }

        // 3. Current input (compressed if over input budget)
        messages.push(json!({"role": "user", "content": current_input}));

        // 4. Enforce budget: drop oldest non-system messages until within budget
        enforce_budget(&mut messages, budget);

        json!(messages)
    }
}

fn enforce_budget(messages: &mut Vec<serde_json::Value>, budget: usize) {
    // Rough token estimate: chars / 4
    while estimate_tokens(messages) > budget && messages.len() > 2 {
        // Remove second message (keep system at [0] and current input at last)
        messages.remove(1);
    }
}
```

---

## Valkey Caching Policy

Multi-turn 핫패스에서 S3/DB 직접 호출을 차단한다.
모든 읽기는 **Valkey → fallback S3/DB** 순서로 진행한다.

### 캐시 키 맵

| 읽기 대상 | Valkey 키 | TTL | 쓰기 시점 | 무효화 시점 |
|-----------|-----------|-----|-----------|------------|
| ConversationRecord (S3) | `veronex:conv:{conversation_id}` | 300s | S3 `put_conversation()` 직후 (Task finalize) | `compress_turn()` S3 재기록 후 DEL |
| 모델 ctx (configured_ctx, max_ctx) | `veronex:ollama:ctx:{provider_id}:{model}` | 600s | capacity analyzer DB upsert 후 | 다음 scrape 사이클에서 자동 덮어쓰기 |
| LabSettings | `CachingLabSettingsRepo` 기존 처리 | 기존 | — | settings PATCH 시 |
| VisionAnalysis | ConversationRecord의 일부 | — | conv 캐시에 포함 | — |
| CompressedTurn summaries | ConversationRecord의 일부 | — | conv 캐시에 포함 | — |

### ConversationRecord 캐시 상세

```
읽기 순서:
  Valkey GET veronex:conv:{conversation_id}
    HIT  → deserialize (zstd JSON) → ContextAssembler 진행
    MISS → S3 get_conversation()
             → 성공: Valkey SET (zstd JSON, EX 300) → ContextAssembler 진행
             → 없음 (신규 대화): 빈 레코드 사용

쓰기 순서 (Task finalize):
  S3 put_conversation()  →  성공 시
  Valkey SET veronex:conv:{conversation_id} (zstd JSON, EX 300)

압축 완료 후 (compress_turn 비동기):
  S3 put_conversation()  →  성공 시
  Valkey DEL veronex:conv:{conversation_id}
  (다음 요청이 갱신된 S3를 읽고 자동으로 캐시 재생성)
```

**DEL을 쓰는 이유**: 압축은 비동기(tokio::spawn)로 실행되며 응답 전송 후 완료된다.
SET으로 덮어쓰면 압축 완료 시점과 다음 요청 시점 사이에 경쟁 조건이 발생할 수 있다.
DEL은 다음 읽기에서 S3 최신값을 강제로 읽게 하므로 안전하다.

### `valkey_keys.rs` 추가

```rust
// ── Conversation record cache ────────────────────────────────────────────────

/// Cached ConversationRecord for a multi-turn session (zstd-compressed JSON).
/// TTL = 300s. Written by: runner.rs after S3 put_conversation().
/// Invalidated (DEL) by: compress_turn() after S3 re-write.
pub fn conversation_record(conversation_id: Uuid) -> String {
    format!("veronex:conv:{conversation_id}")
}
```

### S3 직접 호출 차단 규칙

| 호출 위치 | 변경 전 | 변경 후 |
|-----------|---------|---------|
| `openai_handlers.rs` — Multi-turn 컨텍스트 조립 | `message_store.get_conversation()` 직접 | Valkey GET → miss 시 S3 |
| `runner.rs` — finalize_job() 기존 conv 로드 | `message_store.get_conversation()` 직접 | Valkey GET → miss 시 S3 |
| `compress_turn()` — 압축 전 레코드 로드 | S3 직접 | Valkey GET → miss 시 S3 |

`message_store.get_conversation()` 자체를 Valkey-through로 구현하거나,
호출부에서 Valkey를 먼저 확인 후 miss 시 `message_store`를 호출하는 방식 중 하나 선택.
→ **전자(message_store 내부 캐시 처리) 권장** — 호출부 분산 방지.

---

## Implementation Phases

### Phase 1 — Data model (행동 변화 없음)

**Backend**

| File | Change |
|------|--------|
| `application/ports/outbound/message_store.rs` | `CompressedTurn`, `VisionAnalysis`, `HandoffTurn`, `ConversationTurn` enum 추가; `TurnRecord`에 `compressed`, `vision_analysis` 필드 추가; `get_conversation()` Valkey-through 구현 |
| `infrastructure/outbound/valkey_keys.rs` | `conversation_record(conversation_id)` key fn 추가 |
| `domain/entities/mod.rs` | `InferenceJob`에 `vision_analysis: Option<VisionAnalysis>` 추가 |
| `application/use_cases/inference/mod.rs` | `JobEntry`에 `vision_analysis: Option<VisionAnalysis>` 추가 |
| `migrations/000014_lab_context_compression.up.sql` | 위 DB 섹션의 전체 컬럼 추가 |
| `infrastructure/outbound/persistence/lab_settings_repository.rs` | 신규 필드 SQL 매핑 + `LabSettings` struct 필드 추가 |
| `infrastructure/outbound/persistence/caching_lab_settings_repo.rs` | Pass-through (캐시 변경 불필요) |

---

### Phase 2 — Vision pipeline (모든 Task 적용)

**Backend**

| File | Change |
|------|--------|
| `infrastructure/inbound/http/inference_helpers.rs` | `analyze_images_for_context()` 반환값을 `VisionAnalysis`로 wrap; context-aware vision call 압축 로직 (`compress_for_vision()`) 추가 |
| `infrastructure/inbound/http/ollama_compat_handlers.rs` | Vision Call 결과를 `JobEntry.vision_analysis`에 저장; Multi-turn 시 `compressed_summary` Vision Call 입력에 추가 |
| `infrastructure/inbound/http/openai_handlers.rs` | 동일 — vision 분기 + `compressed_summary` 주입 |
| `infrastructure/inbound/http/handlers.rs` | `/v1/inference` 네이티브 경로에 vision 분기 추가 (현재 누락) |
| `application/use_cases/inference/runner.rs` | `finalize_job()`: `TurnRecord`에 `vision_analysis` 주입 후 S3 저장; S3 저장 후 `Valkey SET veronex:conv:{id}` |

Vision Call budget: vision model의 `configured_ctx` Valkey `ollama_model_ctx` 조회 (`lookup_ctx` 재사용).
Vision N≥2 라우팅: `vision_model` 설정 시 해당 모델이 있는 provider 우선; 없으면 `find_vision_provider()` 자동 선택 (N=1과 동일 — 별도 VisionRouter 불필요).

---

### Phase 3 — CompressionRouter + compression call

**Backend**

| File | Change |
|------|--------|
| `application/use_cases/inference/context_compressor.rs` (new) | `compress_turn()` — Ollama `/api/chat` 직접 호출; 완료 후 S3 재기록 + `Valkey DEL veronex:conv:{id}` |
| `application/use_cases/inference/compression_router.rs` (new) | `CompressionRoute` enum + `decide()` fn |
| `application/use_cases/inference/runner.rs` | S3 write 후: CompressionRouter 통해 압축 spawn |

`compress_turn`은 dispatch queue를 거치지 않음 — compression은 user job이 아님.

---

### Phase 4 — Multi-turn eligibility gate + Context assembler

**Backend**

| File | Change |
|------|--------|
| `application/use_cases/inference/context_assembler.rs` (new) | `ContextAssembler::assemble()`; `check_multiturn_eligibility()` |
| `infrastructure/inbound/http/openai_handlers.rs` | 핸들러 진입 시 eligibility 체크; `conversation_id` 있으면 assembler 호출 |
| `infrastructure/inbound/http/ollama_compat_handlers.rs` | 동일 — `/api/chat` Multi-turn eligibility 체크 + assembler 호출 |
| `infrastructure/inbound/http/inference_helpers.rs` | `estimate_tokens(text)` helper (chars / 4; CJK 오차 known limitation) |

---

### Phase 5 — Long input compression

**Backend**

| File | Change |
|------|--------|
| `application/use_cases/inference/context_assembler.rs` | `compress_input()` 추가 — 현재 입력이 budget × 0.50 초과 시 압축 |
| `infrastructure/inbound/http/openai_handlers.rs` | assembler 호출 전 `compress_input()` 실행 |
| `infrastructure/inbound/http/ollama_compat_handlers.rs` | 동일 |

---

### Phase 6 — Session Handoff (Layer 3)

**Backend**

| File | Change |
|------|--------|
| `application/use_cases/inference/context_assembler.rs` | `check_handoff_threshold()` — compressed token 합계 > `handoff_threshold × configured_ctx` 판단 |
| `application/use_cases/inference/runner.rs` | 핸드오프 조건 충족 시: 마스터 요약 생성 → 신규 `conversation_id` → S3 신규 `ConversationRecord` (첫 턴 = `HandoffTurn`) |
| `infrastructure/inbound/http/openai_handlers.rs` | 응답에 `X-Conversation-ID` 헤더 + `conversation_renewed: true` 추가 |
| `infrastructure/inbound/http/ollama_compat_handlers.rs` | 동일 |

**Frontend**

| File | Change |
|------|--------|
| `web/lib/api.ts` (or fetch wrapper) | 응답 `X-Conversation-ID` 헤더 읽어 다음 요청에 전달 |
| `web/hooks/useConversation.ts` (or equivalent) | `conversation_renewed: true` 시 UI에 "대화가 새로운 세션으로 이어졌습니다" 알림 |

---

### Phase 7 — Lab settings UI + Vision Model Selector

**Backend**

| File | Change |
|------|--------|
| `infrastructure/inbound/http/dashboard_handlers.rs` | `GET /v1/lab`, `PATCH /v1/lab` 에 신규 필드 전체 반영 |
| `infrastructure/inbound/http/admin_handlers.rs` | `GET /v1/ollama/models` 응답 `OllamaModelDto`에 `is_vision: bool`, `configured_ctx: Option<i32>` 추가 |
| `infrastructure/outbound/persistence/ollama_model_repository.rs` (or dto layer) | `OllamaModelDto` 구조체에 위 필드 추가 |

**Frontend**

| File | Change |
|------|--------|
| `web/lib/types.ts` | `LabSettings` 신규 필드 전체; `OllamaModelDto` (`is_vision`, `configured_ctx`); `MultiturnWarning` 타입 |
| `web/app/providers/components/lab-tab.tsx` | Multi-Turn Requirements 카드, Vision Model Selector 카드, Compression 설정 카드 추가 |
| `web/components/vision-model-selector.tsx` (new) | Vision model 셀렉터 컴포넌트 (ctx budget 안내 포함) |
| `web/components/compression-model-selector.tsx` (new) | Compression model 셀렉터 컴포넌트 |
| `web/components/model-selector.tsx` | `getMultiturnWarnings()` 호출 → 경고 배지 표시 |
| `web/components/multiturn-allowed-models-input.tsx` (new) | 태그 입력 컴포넌트 (allowlist 관리) |
| i18n: `en.json`, `ko.json`, `ja.json` | `lab.*` 신규 키 전체 |

---

### Phase 8 — Admin UI Internals

**Backend**

| File | Change |
|------|--------|
| `infrastructure/inbound/http/admin_handlers.rs` | `GET /v1/admin/conversations/{id}` — `vision_analysis`, `compressed` 포함 전체 `TurnRecord` 반환 |
| `infrastructure/inbound/http/admin_handlers.rs` | `GET /v1/admin/jobs/{id}` — 동일 |
| `infrastructure/inbound/http/router.rs` | `/v1/admin/*` 라우트에 Admin 인증 미들웨어 적용 (`KeyTier::Admin` 또는 대시보드 세션) |

**Frontend**

| File | Change |
|------|--------|
| `web/app/admin/conversations/[id]/page.tsx` | `TurnInternals` 컴포넌트 렌더링 |
| `web/app/admin/jobs/[id]/page.tsx` | `TurnInternals` 컴포넌트 렌더링 (신규 페이지일 수 있음) |
| `web/components/turn-internals.tsx` (new) | 접힘/펼침 + Vision 분석 패널 + Compression 패널 + 토큰 통계 |
| `web/lib/types.ts` | `VisionAnalysis`, `CompressedTurn`, `ConversationTurn` 타입 추가 (Phase 7에서 일부 추가됨) |
| i18n: `en.json`, `ko.json`, `ja.json` | `admin.internals.*` 키 |

`TurnInternals` 컴포넌트는 챗봇 뷰(`/chat`)에서 import하지 않음 — 번들 분리.

---

## Failure Modes & Fallbacks

| Failure | Behavior |
|---------|----------|
| Compression Ollama call fails | `turn.compressed = None`, inference proceeds with raw |
| Compression times out | Same as above — timeout = `lab_settings.compression_timeout_secs` (default 10s) |
| S3 write of compressed turn fails | Log warn, inference not blocked |
| `configured_ctx` unknown for model | Use 4,096 as conservative default |
| `context_compression_enabled = false` | Context assembler skipped entirely, raw messages forwarded as-is (current behavior) |
| All providers busy at Turn N+1 (Skip route) | SyncInline fallback — compress before inference even on N≥2 |

---

## Metrics (ClickHouse)

Add to `inference_logs` (via OTel pipeline, no schema change needed — use existing `attributes` map):

```
// 압축
compression_ratio        Float32   (compressed_tokens / original_tokens)
compression_model        String
compression_latency_ms   UInt32
context_budget_used_pct  Float32   (assembled_tokens / budget_tokens)

// 비전
vision_call_latency_ms   UInt32
vision_model             String
vision_image_count       UInt8
vision_fallback          Bool      (true = vision model 없어서 이미지 드롭)
vision_compressed_input  Bool      (true = vision prompt 압축 적용됨)

// 핸드오프
handoff_triggered        Bool
handoff_prev_conv_id     String    (이전 conversation_id, 추적용)
```

압축/핸드오프 지표: `compress_turn()` 및 handoff 로직에서 `ObservabilityPort` 경유 emit.
비전 지표: vision call 완료 후 동일 `ObservabilityPort` 경유 emit.

---

## Image Handling

비전 처리는 **Task 레벨 동작**입니다.
이미지가 포함된 모든 Task에 적용되며, conversation_id(Multi-turn) 유무와 무관합니다.

### 적용 범위

```
Task 레벨 (항상 적용)
──────────────────────────────────────────────────────
모든 API 경로 (/v1/inference, /api/generate, /api/chat, /v1/chat/completions)
이미지 포함 시:
    is_vision_model(inference_model)?
        Yes → 직접 추론 진행
        No  → Vision Call → VisionAnalysis 저장 → Inference Call

Multi-turn 레벨 (conversation_id 있을 때만 추가 적용)
──────────────────────────────────────────────────────
Vision Call 텍스트 입력에 compressed_summary 추가:
    Task (독립):        Vision Call 입력 = user_question
    Multi-turn Task:    Vision Call 입력 = compressed_summary + user_question
```

**현재 코드베이스 상태**: `analyze_images_for_context()`가 `ollama_compat_handlers.rs`에
이미 존재하지만, 분석 결과를 `VisionAnalysis` 구조체로 `TurnRecord`에 저장하지 않음.
Phase 2 구현에서 이 함수의 출력을 `VisionAnalysis`로 wrap하여 `finalize_job()`에 전달.

budget 계산 로직(vision model의 `configured_ctx × 0.60`)은 모든 경우 동일 적용.

### 능력 분리

```
멀티턴 게이트          비전 게이트
─────────────          ───────────
params >= 7B           is_vision_model(model_name)
max_ctx >= 16K         (기존 inference_helpers.rs 함수)

두 게이트는 독립적.
모든 Task: 비전 게이트만 적용 (Multi-turn 게이트 평가 없음).
Multi-turn Task: 두 게이트 모두 적용.
7B+/16K+ 이지만 비전 불가 → Multi-turn 허용, 이미지는 Vision Call로 우회 처리.
```

### 이미지 처리 흐름

```
Task 요청 (이미지 포함)
        │
        ▼
is_vision_model(inference_model)?
        │
   ┌────┴──────┐
  Yes           No
   │             │
   │        vision_model 설정 있음?
   │        (lab_settings.vision_model)
   │             │
   │       ┌─────┴──────┐
   │      Yes            No
   │       │              │
   │  지정 모델로      available providers에서
   │  이미지 분석      비전 모델 자동 선택
   │       │              │
   │       └──────┬───────┘
   │              ▼
   │      [Vision Call]
   │      입력: compressed_prompt + images
   │      "다음 이미지를 분석하고
   │       이 질문과 관련된 핵심 정보만
   │       텍스트로 요약해라: {user_question}"
   │      출력: 이미지 분석 텍스트 (~200 tokens)
   │              │
   │      이미지 → 분석 텍스트로 교체
   │              │
   └──────────────┘
                  │
        [Inference Call]
        입력: compressed_ctx + 분석텍스트 + 질문
        (이미지 없음 — 텍스트만)
```

### 압축된 컨텍스트 + 이미지 분석 프롬프트

```
[Vision Call 프롬프트]

이전 대화 맥락:
{compressed_summary}  ← 이전 턴들의 압축본

현재 질문: {user_message}

위 맥락을 고려하여 이미지에서 질문과 관련된
핵심 정보만 200 단어 이내로 추출하라.
코드/수치/에러는 정확히 보존할 것.
```

목적 파악 + 맥락 기반 분석 — 단순 이미지 설명이 아닌 질문 목적에 맞는 정보 추출.

### 비전 분석 결과 저장 (필수)

비전 Call 결과는 **반드시 TurnRecord에 저장**해야 합니다.
저장하지 않으면 이후 압축 시 이미지 컨텍스트가 완전히 소실됩니다.

```rust
// application/ports/outbound/message_store.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    pub job_id: Uuid,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    pub created_at: String,
    // compression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compressed: Option<CompressedTurn>,
    // NEW: vision call output — stored before inference runs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision_analysis: Option<VisionAnalysis>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionAnalysis {
    /// 비전 모델이 생성한 이미지 분석 텍스트.
    /// 압축 프롬프트에 포함되어 이미지 컨텍스트를 보존.
    pub analysis: String,
    /// 분석에 사용된 모델 (e.g. "llava:7b")
    pub vision_model: String,
    /// 원본 이미지 수
    pub image_count: u32,
    /// 분석 토큰 수
    pub analysis_tokens: u32,
}
```

**저장 시점**: Vision Call 완료 직후, Inference Call 실행 전.

**저장 경로 (요청 유형별):**

```
Task/Job 요청                       채팅 요청 (단일/멀티)
─────────────────                   ────────────────────
Vision Call 완료                    Vision Call 완료
    │                                   │
    ▼                                   ▼
InferenceJob.vision_analysis 설정   S3 TurnRecord.vision_analysis 저장
(in-memory JobEntry)                (put_conversation() 호출)
    │                                   │
    ▼                                   ▼
Inference Call 실행                 Inference Call 실행
    │                                   │
    ▼                                   ▼
finalize_job():                     finalize_job():
  TurnRecord에 vision_analysis 포함    S3 TurnRecord.result 갱신
  S3 put_conversation() 호출          [tokio::spawn] 압축 시작
```

`InferenceJob` 구조체에 `vision_analysis: Option<VisionAnalysis>` 필드 추가.
`JobEntry`(in-memory)에서 보유하다가 `finalize_job()` 시 `TurnRecord`에 주입 후 S3 저장.
DB(`inference_jobs`)에는 저장하지 않음 — S3 `TurnRecord`가 단일 소스.

### 비전 턴 압축 프롬프트

vision_analysis가 있는 턴은 압축 시 별도 포맷 사용:

```
[system]
You are a lossless context compressor. Summarize the following conversation turn
into a single compact paragraph. Rules:
- Preserve: intent of question, key decisions, named entities, numbers, errors, code identifiers.
- Omit: filler, repetition, courtesy phrases.
- Output ONLY the summary. No preamble. No labels.
- Target: under 150 words.

[user]
Q: {turn.prompt}
[Image Analysis]: {turn.vision_analysis.analysis}
A: {turn.result}
```

**압축 요약 예시 (비전 턴):**
```
"Q: [이미지: Rust 컴파일 에러 스크린샷] borrow checker 에러 해결법?
 Image: line 42에서 `data`에 대해 &mut 참조 중복 발생.
        `process()` 함수가 이미 mutable borrow를 보유 중.
 A: Arc<Mutex<T>>로 교체하거나 borrow scope를 분리할 것.
    let result = { let d = &mut data; process(d) }; 패턴 사용."
```

→ ~100 tokens. 이미지 없이 다음 턴에서 완전히 재현 가능.

### 이전 턴 이미지 처리

이전 턴의 이미지는 **절대 재전송하지 않음**.
`TurnRecord.vision_analysis.analysis`가 텍스트로 컨텍스트를 보존함:

```
Context assembly (Turn N+1):

Turn 3 (비전 턴, compressed):
  compressed.summary = "Q: [이미지] borrow 에러? Image: line 42 &mut 중복. A: Arc<Mutex> 권장"

이미지 파일 자체: S3 messages에만 보존 (audit/training용)
추론에는 절대 포함 안 함 — 텍스트 요약만 사용
```

### 비전 모델 자동 선택 로직

```rust
fn find_vision_provider(providers: &[LlmProvider]) -> Option<&LlmProvider> {
    providers.iter().find(|p| {
        p.provider_type == ProviderType::Ollama
            && is_vision_model(&p.default_model)  // 기존 헬퍼 재사용
    })
}
```

### 실패 처리

| 케이스 | 동작 |
|--------|------|
| 비전 모델 없음 (자동 선택 실패) | 이미지 드롭 + 텍스트 질문만 진행, 경고 로그 |
| Vision call 타임아웃 | 이미지 드롭 + fallback |
| 이미지 분석 결과가 너무 김 | 200 tokens로 truncate |

### Web UI 경고 (이미지 관련)

모델 선택기에서 비전 불가 모델 선택 + 이미지 첨부 시:

```
┌────────────────────────────────────────────────────┐
│  qwen2.5:7b (text-only)                    ▼      │
├────────────────────────────────────────────────────┤
│  ℹ  This model does not support images.            │
│     Images will be analyzed by: llava:7b           │ ← vision_model 표시
│     and the result will be passed as text.         │
└────────────────────────────────────────────────────┘
```

색상: `text-blue-400` (에러/경고 아님 — 정보성).
vision_model 없을 경우: `text-yellow-500` 경고로 격상.

---

## Vision Model Selector UI

Lab Features 페이지에 독립적인 **Vision Model Selector** 카드를 배치합니다.
MCP 탭의 `OrchestratorModelSelector`와 동일한 구조를 따릅니다.

### 설정 카드 (Lab Features 페이지)

```
┌─────────────────────────────────────────────────────────┐
│  Vision Model                                           │
├─────────────────────────────────────────────────────────┤
│  Used to analyze images in conversations where the      │
│  active inference model does not support vision.        │
│                                                         │
│  Vision model                                           │
│  ┌────────────────────────────────────────────────┐    │
│  │  llava:7b                              ▼       │    │
│  └────────────────────────────────────────────────┘    │
│                                                         │
│  ┌──────────────────────────────────────────────────┐  │
│  │  ℹ  Context window: 4,096 tokens                 │  │
│  │     Vision prompt budget: 2,457 tokens (60%)     │  │
│  └──────────────────────────────────────────────────┘  │
│                                                         │
│  [Case: no vision provider available]                   │
│  ┌──────────────────────────────────────────────────┐  │
│  │  ⚠  No vision-capable model is currently loaded. │  │
│  │     Images will be dropped from conversations.   │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

드롭다운에는 현재 `GET /v1/ollama/models` 응답에서 `is_vision = true` 인 모델만 표시.
`is_vision` 필드는 기존 `is_vision_model()` 헬퍼 로직을 DTO에 반영 (Phase 7).

선택 직후 Valkey `ollama_model_ctx` 캐시에서 해당 모델의 `configured_ctx`를 읽어
**Vision prompt budget** 안내 문구를 즉시 갱신.

구현 파일은 → **Phase 7** 참조.

---

## Context-Aware Vision Call

Vision call 전에 입력 프롬프트를 **비전 모델 자체의 `configured_ctx`** 기준으로 압축합니다.
이미지 페이로드는 항상 그대로 첨부되므로, 텍스트 부분만 예산 내로 줄입니다.

### 예산 계산

```
vision_budget_tokens = vision_model.configured_ctx × 0.60

(configured_ctx 는 Valkey veronex:ollama:ctx:{provider_id}:{model} 에서 읽음.
 캐시 미스 시 기본값: 4,096)

vision_text_tokens = estimate_tokens(compressed_summary + "\n" + user_question)

vision_text_tokens > vision_budget_tokens?
  → compress_for_vision(compressed_summary, user_question, budget)
  → 압축된 텍스트 + 이미지로 Vision Call 실행
else
  → 원본 그대로 + 이미지로 Vision Call 실행
```

### Vision용 압축 프롬프트

```
[system]
You are a context distiller. Reduce the following conversation context and question
to fit under {budget} tokens while preserving all information needed to analyze an image.
Preserve: topic, named entities, key decisions, the exact question.
Omit: filler, prior image analyses (they are no longer available).
Output ONLY the distilled text. No preamble.

[user]
Context:
{compressed_summary}

Question: {user_question}
```

출력을 `compressed_summary + "\n" + user_question` 대신 Vision Call 텍스트로 사용.

### 코드 흐름 (runner.rs 또는 vision_handler)

```rust
async fn run_vision_call(
    vision_provider: &LlmProvider,
    valkey: Option<&fred::clients::Pool>,
    compressed_summary: &str,
    user_question: &str,
    images: &[EncodedImage],
    compression_client: &dyn InferenceProviderPort,
) -> Result<String> {
    // 1. 비전 모델 ctx 읽기
    let vision_ctx = match valkey {
        Some(vk) => lookup_ctx(vk, vision_provider.id, &vision_provider.default_model)
            .await
            .unwrap_or(4_096),
        None => 4_096,
    };

    // 2. 예산 계산
    let budget = (vision_ctx as f32 * 0.60) as usize;
    let text = format!("{compressed_summary}\n{user_question}");

    // 3. 필요 시 압축
    let vision_text = if estimate_tokens(&text) > budget {
        compress_for_vision(compression_client, &text, budget).await
            .unwrap_or(text)  // 압축 실패 시 원본 (fallback)
    } else {
        text
    };

    // 4. Vision Call 실행
    // vision_text + images → 분석 결과 (~200 tokens)
    call_vision(vision_provider, &vision_text, images).await
}
```

### 실패 처리

| 케이스 | 동작 |
|--------|------|
| Valkey 미스 (ctx 불명) | 4,096 기본값 사용 → 보수적 예산 |
| Vision 압축 call 실패 | 원본 텍스트로 Vision Call 진행 (fallback) |
| Vision Call 자체 실패 | 이미지 드롭 + 텍스트 질문만 Inference로 전달 |
| `vision_budget_tokens` < 200 | 압축 스킵, 경고 로그, 원본 전송 |

### DB + LabSettings 추가 필드

```sql
-- vision_model 은 이미 정의됨 (위 Image Handling 섹션)
-- 추가 필요 없음
```

```rust
// vision_model: Option<String> 이미 포함됨
// configured_ctx 는 Valkey에서 런타임에 읽으므로 DB 저장 불필요
```

### Phase 5 UI 구현 노트

- `GET /v1/ollama/models` 응답에 `is_vision: bool`, `configured_ctx: number | null` 추가
- `VisionModelSelector` 컴포넌트: 선택 시 `configured_ctx`로 예산 안내 갱신
- `lab-tab.tsx`: Vision Model Selector 카드 → Multi-Turn Requirements 카드 아래 배치

---

## Admin UI — Turn Internals

관리자 대화/작업 상세 페이지에서 각 턴의 내부 처리 결과를 노출합니다.
**챗봇/엔드유저 뷰에서는 완전히 숨김** — 역할 기반 필터링으로 구현.

### 노출 대상 데이터

| 데이터 | 소스 | 표시 조건 |
|--------|------|-----------|
| Q&A (prompt + result) | `TurnRecord` | 항상 표시 |
| Vision 분석 텍스트 + 모델 + 이미지 수 | `TurnRecord.vision_analysis` | `Some(...)` 일 때 |
| 압축 요약 + 압축 모델 + 토큰 수 | `TurnRecord.compressed` | `Some(...)` 일 때 |
| 토큰 합계 (prompt / response / total) | `TurnRecord` → `InferenceJob` metrics | 항상 표시 |

### Admin 대화 상세 페이지 — Turn 카드

```
┌─ Turn 3 ───────────────────────────────── ⚙ Internals ▼ ─┐
│                                                            │
│  User                                                      │
│  이 Rust 코드에서 borrow 에러 어떻게 해결해?               │
│  [📎 screenshot.png]                                       │
│                                                            │
│  Assistant                                                 │
│  Arc<Mutex<T>>로 교체하거나 borrow scope를 분리...        │
│                                                            │
│  ▼ Internals ──────────────────────────────────────────── │
│                                                            │
│  👁 Vision Analysis · llava:7b · 1 image                  │
│  ┌──────────────────────────────────────────────────────┐ │
│  │ line 42에서 `data`에 대해 &mut 참조 중복 발생.      │ │
│  │ `process()`가 이미 mutable borrow 보유 중.           │ │
│  └──────────────────────────────────────────────────────┘ │
│                                                            │
│  🗜 Compression · qwen2.5:3b · 847t → 98t (88.4%)         │
│  ┌──────────────────────────────────────────────────────┐ │
│  │ Q: [이미지] borrow 에러? Image: line 42 &mut 중복.  │ │
│  │ A: Arc<Mutex> 또는 scope 분리 권장.                  │ │
│  └──────────────────────────────────────────────────────┘ │
│                                                            │
│  Tokens  prompt 312 · response 235 · total 547            │
└────────────────────────────────────────────────────────────┘
```

- `⚙ Internals` 기본 **접힘(collapsed)** — 일반 열람 시 노이즈 없음
- 배지(`👁`, `🗜`)가 있는 턴은 turn 헤더에 아이콘으로 표시 (펼치지 않아도 파악 가능)
- Task/Job 요청도 동일 카드 구조 사용 (`/admin/jobs/{id}` 상세 페이지)

### API 노출 정책

```
GET /v1/admin/conversations/{id}   → TurnRecord 전체 반환 (vision_analysis, compressed 포함)
GET /v1/conversations/{id}         → TurnRecord 필터링 반환 (vision_analysis, compressed 제외)

GET /v1/admin/jobs/{id}            → InferenceJob + TurnRecord 전체 (vision_analysis 포함)
GET /v1/jobs/{id}                  → InferenceJob 기본 필드만 (vision_analysis 제외)
```

역할 판단: `X-Api-Key` 의 `KeyTier::Admin` 여부, 또는 대시보드 세션 쿠키.
별도 엔드포인트 분리 (path prefix `/admin/`) — 미들웨어 레이어에서 인증 처리.

### 프론트엔드 구현 파일

| 파일 | 변경 |
|------|------|
| `web/app/admin/conversations/[id]/page.tsx` | `TurnInternals` 컴포넌트 렌더링 |
| `web/app/admin/jobs/[id]/page.tsx` | 동일 `TurnInternals` 컴포넌트 렌더링 |
| `web/components/turn-internals.tsx` (new) | 접힘/펼침 + Vision/Compression 패널 |
| `web/lib/types.ts` | `VisionAnalysis`, `CompressedTurn`, `TurnRecord` 타입 추가 |
| `web/lib/i18n/en.json` | `admin.internals.*` 키 |
| `web/lib/i18n/ko.json` | 동일 |

`TurnInternals` 컴포넌트는 챗봇 뷰(`/chat`)에서 import하지 않음 — 번들 분리.

---

## Settings — Full Web UI Reference

Lab Features 페이지에서 관리되는 모든 멀티턴/압축 설정:

| 설정 | 타입 | 기본값 | 경고 조건 |
|------|------|--------|-----------|
| `context_compression_enabled` | bool | false | — |
| `compression_model` | string? | null | 설정 모델이 로드되지 않은 경우 ⚠ |
| `compression_timeout_secs` | int | 10 | — |
| `context_budget_ratio` | float | 0.60 | 0.50 미만 ⚠, 0.80 초과 ⚠ |
| `compression_trigger_turns` | int | 1 | — |
| `recent_verbatim_window` | int | 1 | — |
| `multiturn_min_params` | int | 7 | 7 미만으로 낮추면 ⚠ |
| `multiturn_min_ctx` | int | 16384 | 16384 미만 ⚠ |
| `multiturn_allowed_models` | string[] | [] | — |
| `vision_model` | string? | null | 비전 모델 없을 때 ⚠ |
| `handoff_enabled` | bool | true | — |

경고 기준:
- `multiturn_min_params < 7`: "모델 품질 저하 위험"
- `multiturn_min_ctx < 16384`: "잦은 세션 핸드오프 발생"
- `context_budget_ratio < 0.50`: "생성 공간 부족 — 출력 잘림 위험"
- `context_budget_ratio > 0.80`: "모델 컨텍스트 품질 저하 위험"
- `vision_model` 미설정 + 비전 불가 모델 사용: "이미지 드롭 발생 가능"

---

## Not In Scope

- Semantic / vector-based compression (LLMLingua, embeddings) — too heavy for target hardware
- KV cache compression — model-level, requires custom Ollama build
- Cross-conversation compression (summarizing across sessions)
- Compression of system prompts
- Kafka/Redpanda pipeline — in-process tokio::spawn is sufficient for target scale
- **Gemini provider** — multi-turn gate, context compression, vision pipeline 모두 Gemini는 추후 지원. 현재 구현 범위는 Ollama 전용.
