# Image Inference SDD

> **Status**: Pending | **Last Updated**: 2026-03-15
> **Branch**: feat/image-inference (미생성)
> **Scope**: veronex backend + web 프론트엔드 (Ollama, MinIO, Redpanda, OTel 설정 변경 없음)

---

## 문제

veronex가 multimodal 이미지 추론을 지원하지 않음.

- `POST /api/generate` — `images` 필드가 파싱은 되지만 Ollama에 전달되지 않음 (`#[allow(dead_code)]`)
- `POST /api/chat` — messages 안의 images는 `serde_json::Value`로 이미 통과 → **동작함**
- `/jobs` Test Runs 탭 — 이미지 첨부 UI 없음

---

## 설계 결정

### MinIO 임시 업로드 불필요

Ollama는 이미지를 **raw base64 문자열만** 받음 — URL 지원 없음.
`data:image/jpeg;base64,...` prefix 포함 시 `"illegal base64 data at input byte 4"` 에러 발생 (확인된 Ollama 버그).

MinIO에 올리면 Ollama 전송 시 재다운로드 후 base64 변환 필요 → 왕복만 추가됨.

### 이미지 압축은 브라우저에서

서버에서 압축 시 `image` crate 의존성 추가 + CPU 소모 + 브라우저→서버 네트워크 절감 없음.
Canvas API로 브라우저에서 압축 후 전송 — 서버 코드 변경 없음.

---

## Vision 모델별 실효 해상도 (웹 조사 결과)

| 모델 | 내부 인코더 | 실효 해상도 | 권장 전송 해상도 |
|------|------------|------------|-----------------|
| LLaVA 1.5 | CLIP-ViT 336px | 336×336px | ≤ 672px |
| LLaVA 1.6 (llava-next) | 336px 타일 동적 분할 | 최대 1344×1344px | ≤ 1024px |
| Llama 3.2 Vision | 224px → 타일링 | 최대 1120×1120px | ≤ 1024px |
| MiniCPM-V 2.6 | 가변 비율 | 최대 1344×1344px (~1.8MP) | ≤ 1024px |

> **결론: 긴 변 1024px** 이 모든 모델에서 정보 손실 없이 안전한 상한선.
> 1280px+ 전송 시 모델이 내부 다운샘플링 — 대역폭만 낭비됨.

---

## Ollama 이미지 전송 형식

### `/api/generate` — 최상위 `images` 배열

```json
{
  "model": "llava",
  "prompt": "이 이미지를 설명해줘",
  "images": ["<raw_base64_no_prefix>"],
  "stream": true
}
```

### `/api/chat` — message 내 `images` 필드 (이미 지원)

```json
{
  "model": "llava",
  "messages": [
    { "role": "user", "content": "설명해줘", "images": ["<raw_base64_no_prefix>"] }
  ]
}
```

---

## 알려진 Ollama 이미지 관련 이슈

| 이슈 | 증상 | 대응 |
|------|------|------|
| [#7477](https://github.com/ollama/ollama/issues/7477) | 4장 이상 전송 시 stall — 응답 없음 | **최대 4장으로 제한** |
| [#10041](https://github.com/ollama/ollama/issues/10041) | 이미지 입력 시 VRAM 추가 할당 → OOM | 이미지 크기 제한으로 완화 |
| [ollama-js#68](https://github.com/ollama/ollama-js/issues/68) | data URL prefix → base64 decode 에러 | **반드시 prefix 제거 후 전송** |

---

## 현재 상태

| 위치 | 상태 |
|------|------|
| `OllamaGenerateBody.images` | `Option<Vec<String>>` 필드 있음, `#[allow(dead_code)]`, 버려짐 |
| `OllamaChatBody.messages` | `Vec<serde_json::Value>` — images 포함 messages 통과 가능 |
| `TestCompletionRequest` | `images` 필드 없음 |
| `SubmitJobRequest` | `images` 필드 없음 |
| `InferenceJob` | `images` 필드 없음 |
| `OllamaAdapter::stream_generate` | `images` body 포함 안 함 |
| `OllamaAdapter::stream_chat` | messages 그대로 전달 → 이미 동작 |
| `router.rs:290` | **전역 1MB body limit** — 이미지 엔드포인트 override 필요 |
| `ApiTestForm` | 이미지 첨부 UI 없음 |
| `ApiTestPanel` | images 상태 없음 |

---

## 데이터 흐름

```
브라우저 (Test Runs 탭)
  │  이미지 파일 선택 → Canvas 1024px 리사이즈 → JPEG 85% → raw base64
  │  POST /v1/test/completions { messages, images: ["<raw_base64>"] }
  ▼
test_handlers::test_completions()     ← body limit: 20MB (override)
  │  images 개수 검증 (≤ 4)
  │  images 장당 크기 검증 (≤ 2MB base64)
  ▼
SubmitJobRequest { prompt, images: Some([...]) }
  ▼
InferenceUseCase::submit()
  │  InferenceJob { images: Some([...]) }  ← DB 저장 안 함
  ▼
OllamaAdapter::stream_tokens()
  │  job.messages 있으면 stream_chat (이미지 messages 안에 포함)
  │  job.messages 없으면 stream_generate(model, prompt, images)
  ▼
body["images"] = [...]  → Ollama /api/generate
```

---

## 구현 계획

### Phase 1 — Router body limit override (Backend)

**`router.rs`** — 이미지 엔드포인트에 별도 body limit 적용:

현재 `DefaultBodyLimit::max(1024 * 1024)` (1MB) 전역 적용.
이미지 엔드포인트는 `DefaultBodyLimit::max(20 * 1024 * 1024)` (20MB)로 override.

```rust
// 이미지 포함 가능 라우트에 별도 레이어
let image_routes = Router::new()
    .route("/v1/test/completions", post(test_completions))
    .route("/api/generate", post(generate))
    .layer(DefaultBodyLimit::max(20 * 1024 * 1024));
```

---

### Phase 2 — 도메인 경계 확장 (Backend)

**`inference_use_case.rs`** — `SubmitJobRequest`에 `images` 추가:

```rust
pub struct SubmitJobRequest {
    // ... 기존 필드 ...
    pub images: Option<Vec<String>>,
}
```

**`domain/entities/mod.rs`** — `InferenceJob`에 `images` 추가:

```rust
pub struct InferenceJob {
    // ... 기존 필드 ...
    pub images: Option<Vec<String>>,
}
```

> DB 저장 안 함 — adapter에서 직접 참조. messages_json 컬럼은 이미 NULL로 마이그레이션 중.

---

### Phase 3 — 검증 상수 추가 (Backend)

**`constants.rs`**:

```rust
/// Maximum images per request. Ollama stalls with 4+ images (issue #7477).
pub const MAX_IMAGES: usize = 4;

/// Maximum base64 string length per image.
/// 1024px JPEG 85% ≈ 200KB → base64 ≈ 267KB.
/// Limit set to 2MB (raw ~1.5MB) to allow uncompressed small images.
/// base64 overhead: exactly 4/3 × original bytes.
pub const MAX_IMAGE_B64_BYTES: usize = 2 * 1024 * 1024; // 2MB base64
```

---

### Phase 4 — Test 핸들러 images 수신 + 검증 (Backend)

**`test_handlers.rs`**:

```rust
#[derive(Deserialize)]
pub struct TestCompletionRequest {
    pub model: String,
    pub messages: Vec<TestChatMessage>,
    pub provider_type: Option<String>,
    #[serde(default)]
    pub images: Option<Vec<String>>,
}
```

`test_completions()` 검증:

```rust
if let Some(imgs) = &req.images {
    if imgs.len() > MAX_IMAGES {
        return (StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": {"message": "too many images (max 4)"}})))
            .into_response();
    }
    for img in imgs {
        if img.len() > MAX_IMAGE_B64_BYTES {
            return (StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": {"message": "image too large (max 2MB base64)"}})))
                .into_response();
        }
    }
}
```

---

### Phase 5 — generate() 핸들러 images 연결 + 검증 (Backend)

**`ollama_compat_handlers.rs`** — 동일 검증 적용 후 `SubmitJobRequest`에 전달:

```rust
state.use_case.submit(SubmitJobRequest {
    // ...
    images: req.images,
    // ...
}).await
```

---

### Phase 6 — use_case → InferenceJob 전달 (Backend)

**`use_case.rs`**:

```rust
let job = InferenceJob {
    // ... 기존 필드 ...
    images: req.images,
};
```

---

### Phase 7 — Ollama adapter body 포함 (Backend)

**`outbound/ollama/adapter.rs`** — `stream_tokens()`에서 `images` 참조:

```rust
fn stream_tokens(&self, job: &InferenceJob) -> ... {
    if let Some(messages) = &job.messages {
        // /api/chat — messages 안에 이미 images 포함 가능
        return self.stream_chat(job.model_name.as_str(), messages.clone(), job.tools.clone());
    }
    // /api/generate — 최상위 images 전달
    self.stream_generate(job.model_name.as_str(), job.prompt.as_str(), job.images.clone())
}
```

`stream_generate()` body:

```rust
fn stream_generate(&self, model: &str, prompt: &str, images: Option<Vec<String>>) -> ... {
    let mut body = serde_json::json!({
        "model":   model,
        "prompt":  prompt,
        "stream":  true,
        "options": { "num_ctx": num_ctx },
    });
    if let Some(imgs) = images {
        body["images"] = serde_json::json!(imgs);
    }
    // ...
}
```

---

### Phase 8 — 프론트엔드 압축 유틸 (Frontend)

**라이브러리**: `browser-image-compression` (npm)
- Web Worker 내부 실행 → 메인 스레드 비차단
- Canvas API 직접 사용 대비 크로스브라우저 일관성 보장
- Safari `toBlob()` 타이밍 버그 등 엣지 케이스 처리 포함
- 번들 크기: ~25KB gzip

**`web/lib/compress-image.ts`** (신규):

```ts
import imageCompression from 'browser-image-compression'

/**
 * browser-image-compression으로 이미지 압축 후 raw base64 반환.
 * Ollama는 data URL prefix("data:image/...;base64,") 없는 raw base64만 허용.
 * useWebWorker: true → 메인 스레드 비차단.
 */
export async function compressImage(
  file: File,
  maxDim = 1024,    // 모든 Ollama vision 모델 안전 상한선
  quality = 0.85,   // JPEG 품질 0.82-0.90 권장 (AI 추론 정확도 유지)
): Promise<string> {
  const compressed = await imageCompression(file, {
    maxSizeMB: 1.5,
    maxWidthOrHeight: maxDim,
    useWebWorker: true,
    fileType: 'image/jpeg',
    initialQuality: quality,
  })

  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => {
      // "data:image/jpeg;base64,<base64>" → "<base64>" (prefix 제거 필수)
      resolve((reader.result as string).split(',')[1])
    }
    reader.onerror = reject
    reader.readAsDataURL(compressed)
  })
}
```

---

### Phase 9 — 프론트엔드 이미지 업로드 UI (Frontend)

#### `web/components/api-test-types.ts`

```ts
export interface Run {
  // ... 기존 필드 ...
  images: string[]   // raw base64 (no data URL prefix)
}
```

#### `web/components/api-test-form.tsx`

props 추가: `images: string[]`, `onImagesChange: (imgs: string[]) => void`, `isCompressing: boolean`

UI 레이아웃:
```
[ Provider ▼ ] [ Model ▼ ]
[ Prompt textarea              ] [🖼] [▶]
[ 🖼 thumb1 ×] [ 🖼 thumb2 ×]   (압축 중: spinner)
```

- 파일 선택 → `compressImage()` 호출 → `isCompressing: true` (버튼 비활성화)
- 썸네일: `data:image/jpeg;base64,{raw}` → `<img src>` (표시용 prefix 재조립)
- 전송: raw base64만 (prefix 없음)
- 최대 4장, 원본 10MB 초과 시 toast 경고 (압축 전 차단)

#### `web/components/api-test-panel.tsx`

```ts
const [images, setImages] = useState<string[]>([])
const [isCompressing, setIsCompressing] = useState(false)

// fetch body
images: images.length > 0 ? images : undefined,
```

#### i18n (`en.json` / `ko.json` / `ja.json`)

```json
"imageAttach": "Attach images",
"imageRemove": "Remove",
"imageCompressing": "Compressing…",
"imageTooLarge": "Image exceeds 10MB limit",
"imageTooMany": "Maximum 4 images (Ollama limit)"
```

---

### Phase 10 — 테스트

**Backend**:
- `images: Some(vec!["abc"])` → Ollama body에 `"images": ["abc"]` 포함 검증
- `images: None` → body에 `"images"` 키 없음 검증
- 5장 이상 → 400 반환 검증
- 2MB+1 base64 → 400 반환 검증

**Frontend**:
- `compressImage()`: 2048×2048 입력 → 출력 긴 변 1024px 검증 (Vitest + canvas mock)
- 파일 선택 → 썸네일 표시 → fetch body에 raw base64 포함 검증 (Playwright)

---

## 제약 사항

| 항목 | 값 | 검증 위치 | 근거 |
|------|-----|-----------|------|
| 이미지 최대 개수 | **4장** | **백엔드** | Ollama issue #7477 — 4장 이상 stall |
| 장당 base64 최대 크기 | **2MB** | **백엔드** | 1024px JPEG 85% 기준 ~267KB, 여유 8× |
| HTTP body limit (이미지 라우트) | **20MB** | **router.rs override** | 전역 1MB 제한 우회 필요 |
| 원본 파일 최대 크기 | 10MB/장 | 프론트엔드 (UX) | 브라우저 메모리 보호 |
| 압축 해상도 | 1024px (긴 변) | 프론트엔드 | 모든 Ollama vision 모델 안전 상한 |
| JPEG 품질 | 0.85 | 프론트엔드 | AI 추론 정확도 유지 스위트스팟 |
| 전송 포맷 | **raw base64 (prefix 없음)** | - | Ollama 스펙 — prefix 포함 시 decode 에러 |
| 지원 입력 형식 | `image/*` | - | Ollama 모델에 위임 |

> **프론트엔드 검증은 UX, 백엔드가 실제 보안 경계.**
> API 직접 호출로 미압축 이미지 전송 시에도 2MB/장, 4장 한도로 차단됨.

---

## Tasks

| # | Task | 파일 | Status |
|---|------|------|--------|
| 1 | 이미지 라우트 body limit 20MB override | `router.rs` | pending |
| 2 | `MAX_IMAGES=4`, `MAX_IMAGE_B64_BYTES=2MB` 상수 추가 | `constants.rs` | pending |
| 3 | `SubmitJobRequest`에 `images` 필드 추가 | `inference_use_case.rs` | pending |
| 4 | `InferenceJob`에 `images` 필드 추가 | `domain/entities/mod.rs` | pending |
| 5 | `TestCompletionRequest`에 `images` + 개수/크기 검증 | `test_handlers.rs` | pending |
| 6 | `generate()` 핸들러에서 `images` 연결 + 검증 | `ollama_compat_handlers.rs` | pending |
| 7 | `use_case.submit()`에서 `InferenceJob.images` 연결 | `use_case.rs` | pending |
| 8 | `stream_generate()`에 `images` 파라미터 추가 + body 포함 | `ollama/adapter.rs` | pending |
| 9 | `compressImage()` 구현 (`browser-image-compression`, 1024px JPEG 0.85, raw base64) | `web/lib/compress-image.ts` | pending |
| 10 | `ApiTestForm`에 이미지 첨부 + 압축 + 썸네일 UI | `web/components/api-test-form.tsx` | pending |
| 11 | `ApiTestPanel`에 images 상태 + fetch body 포함 | `web/components/api-test-panel.tsx` | pending |
| 12 | i18n 메시지 추가 | `web/messages/*.json` | pending |
| 13 | 백엔드 유닛 테스트 (adapter + 검증 로직) | `adapter.rs`, `test_handlers.rs` | pending |
| 14 | `compressImage()` 유닛 테스트 | Vitest | pending |
