# Image Inference SDD

> **Status**: Pending | **Last Updated**: 2026-03-15
> **Branch**: feat/image-inference (not yet created)
> **Scope**: veronex backend + web frontend (no Ollama, MinIO, Redpanda, or OTel config changes)

---

## Problem

Veronex does not support multimodal image inference.

- `POST /api/generate` — `images` field is parsed but not forwarded to Ollama (`#[allow(dead_code)]`)
- `POST /api/chat` — images inside messages already pass through as `serde_json::Value` → **works**
- `/jobs` Test Runs tab — no image attachment UI

---

## Design Decisions

### MinIO temporary upload unnecessary

Ollama only accepts images as **raw base64 strings** — no URL support.
Including `data:image/jpeg;base64,...` prefix causes `"illegal base64 data at input byte 4"` error (confirmed Ollama bug).

Uploading to MinIO requires re-downloading and base64 conversion before sending to Ollama → only adds round trips.

### Image compression in browser

Server-side compression adds `image` crate dependency + CPU cost + no browser→server network savings.
Compress in browser via Canvas API then transmit — no server code changes.

---

## Per-Vision-Model Effective Resolution (web research)

| Model | Internal encoder | Effective resolution | Recommended upload resolution |
|------|------------|------------|-----------------|
| LLaVA 1.5 | CLIP-ViT 336px | 336×336px | ≤ 672px |
| LLaVA 1.6 (llava-next) | 336px dynamic tile split | max 1344×1344px | ≤ 1024px |
| Llama 3.2 Vision | 224px → tiling | max 1120×1120px | ≤ 1024px |
| MiniCPM-V 2.6 | variable ratio | max 1344×1344px (~1.8MP) | ≤ 1024px |

> **Conclusion: long edge 1024px** is the safe upper bound with no information loss across all models.
> Sending 1280px+ causes internal downsampling by the model — only wastes bandwidth.

---

## Ollama Image Transfer Format

### `/api/generate` — top-level `images` array

```json
{
  "model": "llava",
  "prompt": "Describe this image",
  "images": ["<raw_base64_no_prefix>"],
  "stream": true
}
```

### `/api/chat` — `images` field inside message (already supported)

```json
{
  "model": "llava",
  "messages": [
    { "role": "user", "content": "Describe this", "images": ["<raw_base64_no_prefix>"] }
  ]
}
```

---

## Known Ollama Image Issues

| Issue | Symptom | Mitigation |
|------|------|------|
| [#7477](https://github.com/ollama/ollama/issues/7477) | Stall with 4+ images — no response | **Limit to max 4 images** |
| [#10041](https://github.com/ollama/ollama/issues/10041) | Additional VRAM allocation on image input → OOM | Mitigated by image size limits |
| [ollama-js#68](https://github.com/ollama/ollama-js/issues/68) | data URL prefix → base64 decode error | **Must strip prefix before sending** |

---

## Current State

| Location | Status |
|------|------|
| `OllamaGenerateBody.images` | `Option<Vec<String>>` field exists, `#[allow(dead_code)]`, discarded |
| `OllamaChatBody.messages` | `Vec<serde_json::Value>` — messages containing images pass through |
| `TestCompletionRequest` | no `images` field |
| `SubmitJobRequest` | no `images` field |
| `InferenceJob` | no `images` field |
| `OllamaAdapter::stream_generate` | `images` not included in body |
| `OllamaAdapter::stream_chat` | messages passed as-is → already works |
| `router.rs:290` | **global 1MB body limit** — image endpoints need override |
| `ApiTestForm` | no image attachment UI |
| `ApiTestPanel` | no images state |

---

## Data Flow

```
Browser (Test Runs tab)
  │  Select image file → Canvas 1024px resize → JPEG 85% → raw base64
  │  POST /v1/test/completions { messages, images: ["<raw_base64>"] }
  ▼
test_handlers::test_completions()     ← body limit: 20MB (override)
  │  Validate image count (≤ 4)
  │  Validate per-image size (≤ 2MB base64)
  ▼
SubmitJobRequest { prompt, images: Some([...]) }
  ▼
InferenceUseCase::submit()
  │  InferenceJob { images: Some([...]) }  ← not persisted to DB
  ▼
OllamaAdapter::stream_tokens()
  │  if job.messages → stream_chat (images included in messages)
  │  if no job.messages → stream_generate(model, prompt, images)
  ▼
body["images"] = [...]  → Ollama /api/generate
```

---

## Implementation Plan

### Phase 1 — Router body limit override (Backend)

**`router.rs`** — apply separate body limit to image endpoints:

Currently `DefaultBodyLimit::max(1024 * 1024)` (1MB) applied globally.
Image endpoints override to `DefaultBodyLimit::max(20 * 1024 * 1024)` (20MB).

```rust
// separate layer for routes that may include images
let image_routes = Router::new()
    .route("/v1/test/completions", post(test_completions))
    .route("/api/generate", post(generate))
    .layer(DefaultBodyLimit::max(20 * 1024 * 1024));
```

---

### Phase 2 — Domain boundary extension (Backend)

**`inference_use_case.rs`** — add `images` to `SubmitJobRequest`:

```rust
pub struct SubmitJobRequest {
    // ... existing fields ...
    pub images: Option<Vec<String>>,
}
```

**`domain/entities/mod.rs`** — add `images` to `InferenceJob`:

```rust
pub struct InferenceJob {
    // ... existing fields ...
    pub images: Option<Vec<String>>,
}
```

> Not persisted to DB — referenced directly by adapter. messages_json column is already migrating to NULL.

---

### Phase 3 — Admin-configurable image limits (Backend)

#### Background

`MAX_IMAGES` limit is not an Ollama server constraint.
- Ollama server: no image count limit, accepts any number
- Qwen3 VL and other vision models: support N-image multi-image architecture
- Issue #7477: stall bug in specific version/model combinations — inappropriate basis for hardcoding
- **Conclusion**: admin must be able to adjust per model/environment

#### Recommended approach: extend `lab_settings`

No new endpoint needed. Add columns to existing `lab_settings` table/port/adapter.

- `lab_settings` is effectively used as an "admin operational settings" table
- 1 migration, reuse existing plumbing (port/adapter/handler/route)
- `0` = disable image feature (disable all images)

**Migration**:

```sql
ALTER TABLE lab_settings
    ADD COLUMN max_images_per_request INTEGER NOT NULL DEFAULT 4,
    ADD COLUMN max_image_b64_bytes    INTEGER NOT NULL DEFAULT 2097152;
-- DEFAULT 4: conservative default (avoids Ollama #7477)
-- DEFAULT 2097152: 2MB base64 per image
```

**`lab_settings_repository.rs`** — `LabSettings` struct:

```rust
pub struct LabSettings {
    pub gemini_function_calling: bool,
    /// Max images per inference request. 0 = image input disabled.
    /// Ollama has no server-side limit; this guards against model/version stalls.
    pub max_images_per_request: i32,
    /// Max base64 bytes per image (default 2MB).
    pub max_image_b64_bytes: i32,
    pub updated_at: DateTime<Utc>,
}

impl Default for LabSettings {
    fn default() -> Self {
        Self {
            gemini_function_calling: false,
            max_images_per_request: 4,
            max_image_b64_bytes: 2 * 1024 * 1024,
            updated_at: Utc::now(),
        }
    }
}
```

**`PatchLabSettingsBody`**:

```rust
pub struct PatchLabSettingsBody {
    pub gemini_function_calling: Option<bool>,
    pub max_images_per_request:  Option<i32>,  // None = keep current
    pub max_image_b64_bytes:     Option<i32>,
}
```

**Handler validation** (`test_handlers.rs`, `ollama_compat_handlers.rs`):

```rust
// dynamically read from lab_settings instead of constants
let lab = state.lab_settings_repo.get().await.unwrap_or_default();
let max_images = lab.max_images_per_request as usize;

if let Some(imgs) = &req.images {
    if max_images == 0 {
        return bad_request("image input is disabled");
    }
    if imgs.len() > max_images {
        return bad_request(format!("too many images (max {max_images})"));
    }
    for img in imgs {
        if img.len() > lab.max_image_b64_bytes as usize {
            return bad_request("image too large");
        }
    }
}
```

**`constants.rs`** — only fallback constants remain:

```rust
/// Fallback max images when lab_settings unavailable.
pub const MAX_IMAGE_B64_BYTES_FALLBACK: usize = 2 * 1024 * 1024;
```

#### API response (`GET /v1/dashboard/lab`)

```json
{
  "gemini_function_calling": false,
  "max_images_per_request": 4,
  "max_image_b64_bytes": 2097152,
  "updated_at": "2026-03-15T00:00:00Z"
}
```

#### Frontend integration

Read `max_images_per_request` from `useQuery(labSettingsQuery)` in `ApiTestPanel`
and use dynamically instead of `MAX_IMAGES` constant:

```ts
const { data: labSettings } = useQuery(labSettingsQuery)
const maxImages = labSettings?.max_images_per_request ?? 4
```

Adjustable via slider or number input on admin Lab Settings screen.

---

### Phase 4 — Test handler images receive + validate (Backend)

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

`test_completions()` validation:

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

### Phase 5 — generate() handler images connection + validation (Backend)

**`ollama_compat_handlers.rs`** — apply same validation then pass to `SubmitJobRequest`:

```rust
state.use_case.submit(SubmitJobRequest {
    // ...
    images: req.images,
    // ...
}).await
```

---

### Phase 6 — use_case → InferenceJob pass-through (Backend)

**`use_case.rs`**:

```rust
let job = InferenceJob {
    // ... existing fields ...
    images: req.images,
};
```

---

### Phase 7 — Ollama adapter body inclusion (Backend)

**`outbound/ollama/adapter.rs`** — reference `images` in `stream_tokens()`:

```rust
fn stream_tokens(&self, job: &InferenceJob) -> ... {
    if let Some(messages) = &job.messages {
        // /api/chat — images already included in messages
        return self.stream_chat(job.model_name.as_str(), messages.clone(), job.tools.clone());
    }
    // /api/generate — pass top-level images
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

### Phase 8 — Frontend compression utility (Frontend)

**Library**: `browser-image-compression` (npm)
- Runs inside Web Worker → non-blocking main thread
- Cross-browser consistency vs direct Canvas API usage
- Handles Safari `toBlob()` timing bugs and other edge cases
- Bundle size: ~25KB gzip

**`web/lib/compress-image.ts`** (new):

```ts
import imageCompression from 'browser-image-compression'

/**
 * Compress image via browser-image-compression and return raw base64.
 * Ollama only accepts raw base64 without data URL prefix ("data:image/...;base64,").
 * useWebWorker: true → non-blocking main thread.
 */
export async function compressImage(
  file: File,
  maxDim = 1024,    // safe upper bound for all Ollama vision models
  quality = 0.85,   // JPEG quality 0.82-0.90 recommended (maintains AI inference accuracy)
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
      // "data:image/jpeg;base64,<base64>" → "<base64>" (prefix removal required)
      resolve((reader.result as string).split(',')[1])
    }
    reader.onerror = reject
    reader.readAsDataURL(compressed)
  })
}
```

---

### Phase 9 — Frontend image upload UI (Frontend)

#### `web/components/api-test-types.ts`

```ts
export interface Run {
  // ... existing fields ...
  images: string[]   // raw base64 (no data URL prefix)
}
```

#### `web/components/api-test-form.tsx`

Added props: `images: string[]`, `onImagesChange: (imgs: string[]) => void`, `isCompressing: boolean`

UI layout:
```
[ Provider ▼ ] [ Model ▼ ]
[ Prompt textarea              ] [IMG] [>]
[ IMG thumb1 x] [ IMG thumb2 x]   (compressing: spinner)
```

- File select → call `compressImage()` → `isCompressing: true` (button disabled)
- Thumbnails: `data:image/jpeg;base64,{raw}` → `<img src>` (prefix reassembled for display)
- Send: raw base64 only (no prefix)
- Max 4 images, toast warning if original exceeds 10MB (blocked before compression)

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

### Phase 10 — Testing

**Backend**:
- `images: Some(vec!["abc"])` → verify Ollama body contains `"images": ["abc"]`
- `images: None` → verify body has no `"images"` key
- 5+ images → verify 400 returned
- 2MB+1 base64 → verify 400 returned

**Frontend**:
- `compressImage()`: 2048×2048 input → verify output long edge 1024px (Vitest + canvas mock)
- File select → thumbnail display → verify fetch body contains raw base64 (Playwright)

---

## Constraints

| Item | Value | Validation location | Rationale |
|------|-----|-----------|------|
| Max images per request | **Admin setting** (default 4) | **Backend** (lab_settings) | Not an Ollama server limit — avoids model/version stall potential. Qwen3 VL supports N images |
| Max base64 size per image | **Admin setting** (default 2MB) | **Backend** (lab_settings) | 1024px JPEG 85% is ~267KB, default has 8× headroom |
| HTTP body limit (image routes) | **20MB** | **router.rs override** | Must bypass global 1MB limit |
| Max original file size | 10MB/image | Frontend (UX) | Browser memory protection |
| Compression resolution | 1024px (long edge) | Frontend | Safe upper bound for all Ollama vision models |
| JPEG quality | 0.85 | Frontend | Sweet spot for AI inference accuracy |
| Transfer format | **raw base64 (no prefix)** | - | Ollama spec — prefix causes decode error |
| Supported input formats | `image/*` | - | Delegated to Ollama models |

> **Frontend validation is UX; backend is the actual security boundary.**
> Even direct API calls with uncompressed images are blocked at 2MB/image, 4 image limit.

---

## Tasks

| # | Task | File | Status |
|---|------|------|--------|
| 1 | Image route body limit 20MB override | `router.rs` | **done** |
| 2 | DB migration: add `max_images_per_request`, `max_image_b64_bytes` columns to `lab_settings` | `migrations/*.sql` | **done** |
| 3 | `LabSettings` struct + `LabSettingsRepository::update()` parameter extension | `lab_settings_repository.rs` | **done** |
| 4 | `PatchLabSettingsBody` + add new fields to GET/PATCH handler responses | `dashboard_handlers.rs` | **done** |
| 5 | Add `images` field to `SubmitJobRequest` | `inference_use_case.rs` | **done** |
| 6 | Add `images` field to `InferenceJob` | `domain/entities/mod.rs` | **done** |
| 7 | `TestCompletionRequest` `images` + lab_settings-based dynamic validation | `test_handlers.rs` | **done** |
| 8 | `generate()` handler `images` connection + lab_settings-based dynamic validation | `ollama_compat_handlers.rs` | **done** |
| 9 | Connect `InferenceJob.images` in `use_case.submit()` | `use_case.rs` | **done** |
| 10 | Add `images` parameter to `stream_generate()` + include in body | `ollama/adapter.rs` | **done** |
| 11 | Implement `compressImage()` (`browser-image-compression`, 1024px JPEG 0.85, raw base64) | `web/lib/compress-image.ts` | **done** |
| 12 | Image attach + compression + thumbnail UI in `ApiTestForm` | `web/components/api-test-form.tsx` | **done** |
| 13 | Images state + fetch body inclusion in `ApiTestPanel` | `web/components/api-test-panel.tsx` | **done** |
| 14 | Dynamic `max_images_per_request` via `labSettings` in `ApiTestPanel` | `web/components/api-test-panel.tsx` | **done** |
| 15 | i18n messages added | `web/messages/*.json` | **done** |
| 16 | Add `max_images_per_request` setting UI to Admin Lab Settings screen | `web/components/nav-settings-dialog.tsx` | **done** |
| 17 | Backend unit tests (adapter + dynamic validation logic) | `ollama_compat_handlers.rs` | **done** |
| 18 | `compressImage()` unit test | Vitest | **done** |
