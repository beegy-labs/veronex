# Spec 18 — Web Design System (CSS SSOT)

> SSOT | **Status**: 구현 완료 | **Last Updated**: 2026-02-25

## Goal

inferq web admin(`web/`)의 모든 색상·간격·반경 값이 단일 파일에서 관리되도록 한다.
컴포넌트는 토큰 참조만 허용 — 원시 hex/hsl 값을 직접 사용 금지.

---

## 파일 구조

```
web/app/
├── tokens.css       ← 📌 SSOT — 모든 색상·반경 토큰 정의
├── globals.css      ← entry point — tokens.css import + Tailwind base layer
└── ...

web/postcss.config.mjs  ← @tailwindcss/postcss 플러그인만 사용 (tailwind.config.ts 없음)
```

### globals.css (entry point — 수정 금지)

```css
@import 'tailwindcss';       /* Tailwind v4 CSS-first */
@import 'tw-animate-css';    /* 애니메이션 유틸리티 */
@import './tokens.css';      /* 토큰 SSOT */

@layer base {
  * { @apply border-border outline-ring/50; }
  body { @apply bg-background text-foreground; }
}
```

**규칙**: `globals.css`에 새 토큰을 추가하지 않는다. 모든 토큰은 `tokens.css`에만 정의한다.

---

## 4-레이어 토큰 아키텍처 (tokens.css)

```
Layer 0  @property        → CSS 타입 안전 + 색상 트랜지션 지원
Layer 1  --palette-*      → 원시 hex 값 (컴포넌트에서 직접 참조 금지)
Layer 2  --theme-*        → 시맨틱 토큰 (palette에서 매핑, 컴포넌트용)
Layer 3  @theme inline    → Tailwind 유틸리티 생성 (bg-background 등)
```

### Layer 0 — @property 정의 (타입 + 기본값)

`@property`는 브라우저가 토큰을 색상 타입으로 인식하도록 한다.
덕분에 CSS transition이 color 보간을 지원하고, 타입 오류를 브라우저 DevTools에서 확인 가능.

```css
@property --theme-bg-page       { syntax: '<color>'; inherits: true; initial-value: #090e1a; }
@property --theme-primary       { syntax: '<color>'; inherits: true; initial-value: #6467f2; }
/* ... 모든 --theme-* 토큰 동일 패턴 */
```

### Layer 1 — Palette (원시 값)

```css
:root {
  /* 배경 스케일 (어두운 순) */
  --palette-bg-950: #090e1a;   /* page background   hsl(222 47%  7%) */
  --palette-bg-900: #131a25;   /* card / popover    hsl(217 33% 11%) */
  --palette-bg-800: #192535;   /* elevated surfaces */
  --palette-bg-700: #1f2937;   /* border / hover    hsl(215 28% 17%) */

  /* 텍스트 */
  --palette-text-100: #e1e7ef;   /* primary   hsl(213 31% 91%) */
  --palette-text-400: #7588a3;   /* muted     hsl(215 20% 55%) */

  /* 브랜드 */
  --palette-indigo-500: #6467f2;   /* primary / ring  hsl(239 84% 67%) */

  /* 상태 */
  --palette-emerald-500: #10b981;   /* success */
  --palette-red-500:     #ef4444;   /* error   */
  --palette-red-700:     #dc2828;   /* destructive + chart-4 */
  --palette-amber-500:   #f59e0b;   /* warning + chart-3     */
  --palette-blue-500:    #3b82f6;   /* info                  */

  /* 차트 */
  --palette-chart-green: #1daf7e;   /* hsl(160 72% 40%) */
  --palette-chart-sky:   #28a0dc;   /* hsl(200 72% 51%) */
}
```

**규칙**: `--palette-*` 값은 `tokens.css` 내부에서만 `--theme-*` 매핑에 사용한다.
컴포넌트 TSX/CSS에서 `--palette-*`를 직접 `var()` 참조하지 않는다.

### Layer 2 — Semantic Tokens (컴포넌트용)

```css
:root {
  /* 배경 */
  --theme-bg-page:     var(--palette-bg-950);
  --theme-bg-card:     var(--palette-bg-900);
  --theme-bg-elevated: var(--palette-bg-800);
  --theme-bg-hover:    var(--palette-bg-700);

  /* 텍스트 */
  --theme-text-primary:   var(--palette-text-100);
  --theme-text-secondary: var(--palette-text-400);

  /* 테두리 */
  --theme-border: var(--palette-bg-700);

  /* 브랜드 */
  --theme-primary:            var(--palette-indigo-500);
  --theme-primary-foreground: #ffffff;
  --theme-ring:               var(--palette-indigo-500);

  /* 상태 */
  --theme-destructive:    var(--palette-red-700);
  --theme-status-success: var(--palette-emerald-500);
  --theme-status-error:   var(--palette-red-500);
  --theme-status-warning: var(--palette-amber-500);
  --theme-status-info:    var(--palette-blue-500);

  /* 차트 (Recharts tooltip contentStyle에서 직접 참조) */
  --theme-chart-1: var(--palette-indigo-500);
  --theme-chart-2: var(--palette-chart-green);
  --theme-chart-3: var(--palette-amber-500);
  --theme-chart-4: var(--palette-red-700);
  --theme-chart-5: var(--palette-chart-sky);

  /* 반경 */
  --radius: 0.5rem;
}
```

### Layer 3 — @theme inline (Tailwind 유틸리티 매핑)

`@theme inline` 블록이 Tailwind v4에서 `bg-background`, `text-foreground`,
`border-border` 등의 유틸리티를 생성한다.
shadcn/ui 컴포넌트들은 이 이름 규약에 의존한다.

```css
@theme inline {
  --color-background:      var(--theme-bg-page);
  --color-foreground:      var(--theme-text-primary);
  --color-card:            var(--theme-bg-card);
  --color-muted:           var(--theme-bg-elevated);
  --color-muted-foreground:var(--theme-text-secondary);
  --color-primary:         var(--theme-primary);
  --color-border:          var(--theme-border);
  --color-ring:            var(--theme-ring);
  --color-destructive:     var(--theme-destructive);
  /* charts, radius … */
}
```

---

## 사용 규칙

### 컴포넌트 TSX에서

```tsx
// ✅ Tailwind 유틸리티 (Layer 3 경유)
<div className="bg-card text-foreground border-border" />

// ✅ 시맨틱 CSS 변수 — Tailwind로 표현 불가능한 경우 (Recharts 등)
contentStyle={{ background: 'var(--theme-bg-card)', border: '1px solid var(--theme-border)' }}

// ❌ 원시 hex 하드코딩 금지
contentStyle={{ background: '#131a25' }}

// ❌ palette 변수 직접 참조 금지
contentStyle={{ background: 'var(--palette-bg-900)' }}
```

### 새 토큰 추가 시

1. `tokens.css` Layer 1에 `--palette-*` 추가
2. Layer 2에 `--theme-*` 시맨틱 이름으로 매핑
3. Layer 0에 `@property` 정의 추가 (색상 타입 보장)
4. 필요하면 Layer 3 `@theme inline`에 Tailwind 유틸리티 이름 추가

---

## Tailwind v4 설정

```
tailwind.config.ts  → 삭제됨 (Tailwind v4 CSS-first)
postcss.config.mjs  → @tailwindcss/postcss 플러그인만 선언
```

Tailwind v4에서는 `tailwind.config.ts` 대신 `@theme inline` 블록으로
모든 커스터마이징을 CSS 안에서 처리한다.

---

## 현재 토큰 팔레트 (전체)

| 토큰 | 값 | 용도 |
|------|----|------|
| `--theme-bg-page` | `#090e1a` | 페이지 배경 |
| `--theme-bg-card` | `#131a25` | 카드 / 팝오버 배경 |
| `--theme-bg-elevated` | `#192535` | 드롭다운, 입력 필드 |
| `--theme-bg-hover` | `#1f2937` | hover 상태, 테두리 |
| `--theme-text-primary` | `#e1e7ef` | 본문 텍스트 |
| `--theme-text-secondary` | `#7588a3` | 보조 텍스트, placeholder |
| `--theme-border` | `#1f2937` | 테두리 |
| `--theme-primary` | `#6467f2` | 브랜드, 버튼, 링 |
| `--theme-primary-foreground` | `#ffffff` | 버튼 위 텍스트 |
| `--theme-ring` | `#6467f2` | focus ring |
| `--theme-destructive` | `#dc2828` | 삭제, 오류 (강조) |
| `--theme-status-success` | `#10b981` | 성공 뱃지 |
| `--theme-status-error` | `#ef4444` | 오류 뱃지 |
| `--theme-status-warning` | `#f59e0b` | 경고 뱃지 |
| `--theme-status-info` | `#3b82f6` | 정보 뱃지 |
| `--theme-chart-1` | `#6467f2` | 차트 primary line |
| `--theme-chart-2` | `#1daf7e` | 차트 success line |
| `--theme-chart-3` | `#f59e0b` | 차트 warning line |
| `--theme-chart-4` | `#dc2828` | 차트 error line |
| `--theme-chart-5` | `#28a0dc` | 차트 info line |
| `--radius` | `0.5rem` | 기본 반경 |

---

## 접근성

admin 내부 도구이므로 WCAG AA 기준 준수, AAA는 목표치.
색상 대비 예시:

| 조합 | 대비비 | 등급 |
|------|--------|------|
| `--theme-text-primary` on `--theme-bg-page` | ~12:1 | AAA |
| `--theme-text-secondary` on `--theme-bg-card` | ~5.5:1 | AA |
| `--theme-primary` on `--theme-bg-page` | ~5:1 | AA |

---

## 구현 체크리스트

- [x] `web/app/tokens.css` — 4-레이어 SSOT 구현
- [x] `web/app/globals.css` — tokens.css import + Tailwind base
- [x] `tailwind.config.ts` 삭제 (Tailwind v4 CSS-first)
- [x] `postcss.config.mjs` — `@tailwindcss/postcss` 단독 사용
- [x] shadcn/ui 컴포넌트 토큰 연동 (Card, Badge, Button, Input, Table, Select, Dialog)
- [x] Recharts tooltip — `var(--theme-bg-card)`, `var(--theme-border)` 직접 참조
- [x] `tw-animate-css` 사용 (tailwindcss-animate 대체)
