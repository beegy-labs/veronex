# Task 11: Web Dashboard

> Admin + 사용량 모니터링 UI. inferq 백엔드 API를 직접 호출.
> Stack: Next.js 15 + shadcn/ui + Recharts + TanStack Query

## Background (Web Research)

OpenAI, Anthropic, LiteLLM, Langfuse, Helicone 등 주요 플랫폼 분석 결과:
- 핵심: 요청/토큰/비용/레이턴시 4가지가 모든 대시보드의 공통 기반
- 차별점: TTFT, KV-Cache Hit Rate, GPU 현황 (자체 서버일 때)
- 사용자별/API Key별 attribution이 운영 핵심

---

## Steps

### Phase 1 — Frontend 프로젝트 구조

- [ ] `web/` 디렉토리 (monorepo 방식, inferq 루트 하위):

```
web/
├── app/
│   ├── (dashboard)/
│   │   ├── page.tsx          # Overview
│   │   ├── usage/page.tsx    # 사용량 분석
│   │   ├── performance/page.tsx  # 레이턴시/성능
│   │   ├── backends/page.tsx     # LLM 백엔드 관리
│   │   ├── keys/page.tsx         # API Key 관리
│   │   └── errors/page.tsx       # 오류 로그
│   └── layout.tsx
├── components/
│   ├── charts/      # Recharts 래퍼
│   └── ui/          # shadcn/ui
└── lib/
    └── api.ts       # inferq API 클라이언트
```

### Phase 2 — Overview 페이지 (핵심 지표)

MVP 필수 위젯 (6개):

| 위젯 | 데이터 소스 | 설명 |
|------|------------|------|
| Total Requests | ClickHouse | 기간 선택 가능 (일/주/월) |
| Token Usage | ClickHouse | Input + Output 분리 표시 |
| Error Rate % | ClickHouse | `finish_reason=error` / 전체 |
| Cancelled Rate % | ClickHouse | SSE disconnect (`finish_reason=cancelled`) |
| Active Backends | PostgreSQL | 온라인/오프라인 서버 현황 |
| Active API Keys | PostgreSQL | 활성 키 수 |

**시간 필터:** 오늘 / 어제 / 7일 / 30일 / 직접 입력

### Phase 3 — 사용량 분석 페이지

```
[기간 선택] [API Key 필터] [Backend 필터] [Model 필터]

┌─────────────────────────────────────────┐
│ Requests Over Time (일별 막대그래프)      │
│  ■ success  ■ cancelled  ■ error        │
└─────────────────────────────────────────┘

┌──────────────────┐  ┌──────────────────┐
│ Input Tokens     │  │ Output Tokens    │
│ Time Series      │  │ Time Series      │
└──────────────────┘  └──────────────────┘

┌──────────────────────────────────────────┐
│ Model Distribution (도넛 차트)            │
│ 모델별 요청 비율                          │
└──────────────────────────────────────────┘

┌──────────────────────────────────────────┐
│ API Key Usage Table                       │
│ key_prefix | requests | tokens | errors  │
└──────────────────────────────────────────┘
```

### Phase 4 — 성능 페이지

```
┌──────────────────────────────────────────┐
│ TTFT (Time to First Token)               │
│ P50 / P95 / P99 time series             │
│ 목표선 표시: <500ms (대화형 UX 기준)     │
└──────────────────────────────────────────┘

┌──────────────────┐  ┌──────────────────┐
│ End-to-End       │  │ Latency by Model │
│ Latency Trend    │  │ 백엔드/모델 비교  │
└──────────────────┘  └──────────────────┘

┌──────────────────────────────────────────┐
│ Queue Depth Over Time                    │
│ (대기 중인 요청 수 — 병목 감지)          │
└──────────────────────────────────────────┘

┌──────────────────────────────────────────┐
│ Tokens Per Second (TPS) — 시간별 처리량  │
│ (Ollama 백엔드만 해당)                   │
└──────────────────────────────────────────┘
```

- [ ] `inference_logs`에 `ttft_ms` 컬럼 추가 필요 (Task 08과 연동)

> **리서치 인사이트**: TTFT가 UX에서 가장 중요한 단일 지표.
> P99 레이턴시 = "최악의 경우 SLA" 지표로 별도 모니터링 권장.

### Phase 5 — LLM 백엔드 관리 페이지

inferq의 핵심 관리 기능 — 코드/재배포 없이 백엔드 추가/제거:

```
[+ Add Backend]

┌────────────────────────────────────────────────────────┐
│ ID      │ Type      │ URL/Endpoint  │ Status  │ Models │
│ gpu-01  │ OLLAMA    │ http://...    │ ● ONLINE │ 12    │
│ gemini  │ GEMINI    │ api.google... │ ● ONLINE │  5    │
│ gpu-02  │ OLLAMA    │ http://...    │ ○ OFFLINE│  -    │
└────────────────────────────────────────────────────────┘
```

**Add Backend 모달:**
```
Type:     [OLLAMA ▼]  [GEMINI ▼]  [OPENAI ▼]  [ANTHROPIC ▼]  [COMPATIBLE ▼]
ID:       [gpu-03          ]
Name:     [My GPU Server   ]
URL:      [http://192.168.1.10:11434]  ← Ollama/COMPATIBLE만 표시
API Key:  [••••••••••••••••]           ← 클라우드 API만 표시
VRAM(MB): [98304           ]           ← Ollama만 표시
[Test Connection]  [Register]
```

### Phase 6 — API Key 관리 페이지

```
[+ Create API Key]

┌────────────────────────────────────────────────────┐
│ Prefix      │ Name     │ Requests │ Tokens │ Status│
│ iq_01ARZ... │ prod-app │  12,450  │ 2.1M   │ ● ON  │
│ iq_02BHK... │ dev-test │     230  │  45K   │ ● ON  │
└────────────────────────────────────────────────────┘
```

- Key 생성 시 plaintext 1회 표시 (복사 후 닫기 경고)
- 클릭 → 해당 키의 시간별 사용량 드릴다운

### Phase 7 — 에러 로그 페이지

```
[기간] [API Key] [Backend] [Model] [Type: error|cancelled|all]

┌───────────────────────────────────────────────────────────┐
│ Time        │ Job ID │ Model  │ Reason    │ Error Message │
│ 14:23:01   │ uuid.. │ gemini │ error     │ rate_limit..  │
│ 14:21:45   │ uuid.. │ llama3 │ cancelled │ (disconnect)  │
└───────────────────────────────────────────────────────────┘
```

### Phase 8 — inferq API 엔드포인트 (대시보드용)

대시보드가 호출하는 집계 API:

```
GET /v1/dashboard/overview?period=7d
GET /v1/dashboard/requests?period=30d&group_by=day&model=&key_id=
GET /v1/dashboard/tokens?period=30d&group_by=day
GET /v1/dashboard/performance?period=7d    # TTFT, latency percentiles
GET /v1/dashboard/errors?period=7d&limit=100
GET /v1/backends                           # 백엔드 목록 + 상태
GET /v1/keys                               # API Key 목록
```

- ClickHouse 집계 쿼리를 FastAPI 응답으로 래핑
- 대시보드 자체는 API Key 인증 필요 (admin key)

### Phase 9 — docker-compose 추가

```yaml
  inferq-web:
    build: ./web
    ports: ["3000:3000"]
    environment:
      - INFERQ_API_URL=http://inferq:8000
      - INFERQ_ADMIN_KEY=${INFERQ_ADMIN_KEY}
    depends_on: [inferq]
```

## Done

- [ ] Overview: 요청/토큰/오류율/취소율/백엔드현황 (6 위젯)
- [ ] 사용량: 일/주/월 + API Key별 + 모델별 필터
- [ ] 성능: TTFT P50/P95/P99, 레이턴시, 큐 깊이
- [ ] 백엔드 관리: OLLAMA/GEMINI/OPENAI/ANTHROPIC/COMPATIBLE 등록 UI
- [ ] API Key 관리: 발급/취소/사용량 드릴다운
- [ ] 에러 로그: finish_reason 기준 필터 (error / cancelled)
- [ ] 집계 API (`/v1/dashboard/*`) 구현
