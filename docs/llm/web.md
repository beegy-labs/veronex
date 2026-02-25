# Web — Brand Identity & Design System

> SSOT | **Last Updated**: 2026-02-25

## Brand

### Name & Concept

**InferQ** — 이중 의미:
- 표면적: **Infer** + **Q**ueue (추론 대기열 시스템)
- 내포적: **IQ** (Intelligence Quotient)

### Logo Mark (iQ 마크)

```
  •̈   ╭───╮
  │   │ · │
  │   ╰───╯→
```

| 요소 | 의미 |
|------|------|
| `i` dot (glow) | 지능의 스파크 |
| `i` stem | 추론 요청 스트림 |
| `Q` ring | 순환 큐 버퍼 |
| `Q` inner dot | 활성 뉴런 노드 |
| `Q` tail arrow | 처리 완료 출력 |

### 색상

| 역할 | 값 |
|------|----|
| 그래디언트 시작 | `#4f46e5` (indigo-600) |
| 그래디언트 끝 | `#7c3aed` (violet-600) |
| 워드마크 Q | `#c4b5fd` (violet-300) |
| 내부 요소 | `white` |

### 파일

- `web/public/favicon.svg` — 32×32 브라우저 탭
- `web/public/logo.svg` — 172×44 전체 로고 (마크 + 워드마크)
- `web/components/nav.tsx` — 인라인 SVG `IQLogo` (28×28) + "InferQ" 워드마크

---

## Design System

### 파일 구조

```
web/app/
├── tokens.css      ← 📌 SSOT — 모든 색상·반경 토큰
├── globals.css     ← entry point (tokens.css import + Tailwind base)
web/postcss.config.mjs  ← @tailwindcss/postcss 전용 (tailwind.config.ts 없음)
```

### 4-레이어 토큰 아키텍처 (tokens.css)

```
Layer 0  @property      → CSS 타입 안전 + color transition 지원
Layer 1  --palette-*    → 원시 hex 값 (컴포넌트 직접 참조 금지)
Layer 2  --theme-*      → 시맨틱 토큰 (컴포넌트용)
Layer 3  @theme inline  → Tailwind 유틸리티 생성
```

### 시맨틱 토큰 (Layer 2)

| 토큰 | 값 | 용도 |
|------|----|------|
| `--theme-bg-page` | `#090e1a` | 페이지 배경 |
| `--theme-bg-card` | `#131a25` | 카드 / 팝오버 |
| `--theme-bg-elevated` | `#192535` | 입력 필드, 드롭다운 |
| `--theme-bg-hover` | `#1f2937` | hover, 테두리 |
| `--theme-text-primary` | `#e1e7ef` | 본문 텍스트 |
| `--theme-text-secondary` | `#7588a3` | 보조 텍스트 |
| `--theme-border` | `#1f2937` | 테두리 |
| `--theme-primary` | `#6467f2` | 브랜드, 버튼, ring |
| `--theme-destructive` | `#dc2828` | 삭제, 오류 |
| `--theme-status-success` | `#10b981` | 성공 |
| `--theme-status-error` | `#ef4444` | 오류 |
| `--theme-status-warning` | `#f59e0b` | 경고 |
| `--theme-chart-1~5` | 각각 | Recharts 라인 컬러 |
| `--radius` | `0.5rem` | 기본 반경 |

### 사용 규칙

```tsx
// ✅ Tailwind 유틸리티 (Layer 3 경유)
<div className="bg-card text-foreground border-border" />

// ✅ 시맨틱 CSS 변수 (Recharts 등 Tailwind 불가 케이스)
contentStyle={{ background: 'var(--theme-bg-card)', border: '1px solid var(--theme-border)' }}

// ❌ 원시 hex 직접 사용 금지
contentStyle={{ background: '#131a25' }}

// ❌ palette 변수 컴포넌트에서 직접 참조 금지
contentStyle={{ background: 'var(--palette-bg-900)' }}
```

새 토큰 추가 순서: Layer 1 `--palette-*` → Layer 2 `--theme-*` → Layer 0 `@property` → Layer 3 `@theme inline` (필요시)

### 기술 스택

| 항목 | 선택 | 비고 |
|------|------|------|
| CSS 프레임워크 | Tailwind v4 | CSS-first, `tailwind.config.ts` 없음 |
| PostCSS 플러그인 | `@tailwindcss/postcss` | 단독 사용 |
| 애니메이션 | `tw-animate-css` | `tailwindcss-animate` 대체 |
| 컴포넌트 라이브러리 | shadcn/ui | `bg-card`, `text-foreground` 등 Tailwind 네이밍 의존 |
| 상태 관리 | TanStack Query v5 | 서버 상태 fetch + 캐시 |
| 차트 | Recharts | contentStyle에서 `var(--theme-*)` 직접 참조 |

---

## Pages

| 경로 | 내용 |
|------|------|
| `/overview` | DashboardStats (키 수, 잡 수, 상태별 분포) |
| `/jobs` | 잡 목록 + 검색 + 상태 필터 + JobDetail 모달 |
| `/keys` | API 키 목록 + 생성/삭제 |
| `/backends` | GPU Servers + LLM Backends 관리 |
| `/usage` | API 키별 토큰 사용량 차트 |
| `/performance` | P50/P95/P99 latency, 시간별 처리량 |
| `/api-test` | 브라우저 내 인퍼런스 테스트 (SSE 스트림) |
