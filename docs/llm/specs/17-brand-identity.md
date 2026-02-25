# Spec 17 — Brand Identity

> SSOT | **Status**: 구현 완료 | **Last Updated**: 2026-02-25

## 브랜드 네임

**InferQ**

## 핵심 컨셉 — iQ의 이중 의미

| 레이어 | 의미 |
|--------|------|
| 표면적 | **Infer** + **Q**ueue — 추론 작업의 대기열/처리 시스템 |
| 내포적 | **IQ** (Intelligence Quotient) — 스마트하고 고성능인 AI 라우터 |

로고 마크의 `i`(소문자)와 `Q`(대문자)를 함께 읽으면 자연스럽게 **iQ** = IQ.

---

## 로고 마크 구조 (iQ 마크)

```
┌───────────────┐
│  •̈   ╭───╮   │   •̈  = 빛나는 점 (intelligence spark)
│  │   │ · │   │   │  = 추론 요청 스트림 (inference stem)
│  │   ╰───╯→  │   Q  = 순환 큐 (queue ring)
│               │   →  = 데이터 출력 방향 (queue exit arrow)
└───────────────┘
```

### 각 요소의 의미

| 요소 | 형태 | 의미 |
|------|------|------|
| `i` 점 (dot) | 발광하는 원 + glow 필터 | 지능의 스파크, 번뜩이는 아이디어 (Intelligence) |
| `i` 기둥 (stem) | 수직 직사각형 | 추론 요청의 흐름 (Inference input stream) |
| `Q` 원 (ring) | 원형 stroke | 순환 대기열, 처리 중인 요청 (Queue ring buffer) |
| `Q` 내부 점 | 반투명 원 | 활성화된 뉴런/노드 (activated intelligence node) |
| `Q` 꼬리 화살표 | 사선 + 화살촉 | 큐에서 처리 완료 후 빠져나가는 결과 (queue exit) |

---

## 파일

| 파일 | 용도 | 크기 |
|------|------|------|
| `web/public/favicon.svg` | 브라우저 탭 아이콘 | 32×32 viewBox |
| `web/public/logo.svg` | 전체 로고 (마크 + 워드마크) | 172×44 viewBox |

### logo.svg 워드마크 처리

```
[iQ 마크]  Infer Q
            ─────╴╴──── violet-300(#c4b5fd)로 강조
```

"Q"만 `#c4b5fd`(violet-300)로 채색하여 마크의 Q와 시각적으로 연결.

---

## 색상

| 역할 | 값 | 설명 |
|------|----|------|
| 그래디언트 시작 | `#4f46e5` (indigo-600) | 신뢰성, 기술 |
| 그래디언트 끝 | `#7c3aed` (violet-600) | 창의성, AI |
| 내부 요소 | `white` | 마크 위 iQ 글리프 |
| 내부 노드 | `white @ 55% opacity` | 활성화된 상태 암시 |
| 워드마크 Q | `#c4b5fd` (violet-300) | iQ 이중 의미 강조 |

그래디언트 방향: 좌상 → 우하 (45°)

---

## 구현 위치

| 파일 | 변경 내용 |
|------|----------|
| `web/components/nav.tsx` | 인라인 SVG `IQLogo` 컴포넌트 (28×28), "InferQ" 워드마크 |
| `web/app/layout.tsx` | `<title>InferQ</title>`, `<link rel="icon" href="/favicon.svg">` |

---

## 디자인 원칙

1. **32×32 우선**: 파비콘 가독성을 최우선으로 설계. 복잡한 디테일은 제거.
2. **의미 있는 단순함**: 모든 요소(dot glow, inner node, arrow)에 의미 부여.
3. **확장성**: 16px 파비콘에서도 `i`와 `Q`의 실루엣이 구별됨.
4. **앱 테마 일치**: `--theme-primary` (#6366f1)과 같은 계열의 indigo-violet 그래디언트.

---

## 거절된 대안

| 옵션 | 이유 |
|------|------|
| 옵션 A: 뇌 실루엣 | 32px에서 판독 불가, 너무 구상적 |
| 옵션 C: 3노드 추상 | iQ 연상 없음, 브랜드 개성 약함 |
| 대문자 `I` serif | IQ 연상은 되나, 소문자 `i`보다 기술적 감성 낮음 |
