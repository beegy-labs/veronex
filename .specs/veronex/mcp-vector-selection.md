# MCP Vector Selection SDD

> **Status**: Planned | **Last Updated**: 2026-03-30
> **Scope**: S12 — Vespa 기반 MCP 툴 벡터 선택 + 벡터 SSOT
> **Branch**: `feat/mcp-vector-selection` (신규)
> **Depends on**: S11 (mcp-integration)

---

## 목적

현재 MCP 브리지는 등록된 **모든** 툴을 LLM에 주입한다 (`MAX_TOOLS=32` 하드 컷).
툴이 100K+ 규모로 늘어나면 이 방식은 동작하지 않는다.

**목표**: 사용자 쿼리를 임베딩해 벡터 유사도로 관련 툴만 선택(Top-K) → LLM에 주입.

```
Before: 모든 툴(N개) → LLM (MAX_TOOLS=32 컷)
After:  쿼리 임베딩 → Vespa ANN 검색 → Top-K 툴 → LLM
```

**Vespa = 벡터 SSOT.** MCP 툴 선택뿐 아니라 추후 모든 벡터 연산(RAG, 대화 검색, 검색 시스템)은 Vespa 단일 인스턴스로 통합한다.

---

## 엔진 선택 근거

### Vespa (채택)

| 기술 지표 | 내용 |
|---------|------|
| 벡터 HNSW | 네이티브 1급 시민 |
| BM25 + 벡터 하이브리드 | 동일 랭킹 파이프라인 내 처리 |
| RAM 효율 | C++ 데이터 플레인, JVM 없음 |
| GC 이슈 | 없음 |
| 하이브리드 처리량 | ES 대비 8.5X (자체 벤치마크) |
| 라이센스 | **Apache 2.0** — 상업적 사용 무제한, 소스 공개 의무 없음 |
| 레퍼런스 | Perplexity, Spotify, Yahoo, Vinted |

### ES 미채택 이유

- 벡터 kNN이 후발 추가 기능 — BM25와 동일 파이프라인 불가
- JVM 힙 오버헤드, GC 이슈
- ClickHouse가 애널리틱스/aggregation을 이미 커버 → ES의 핵심 강점 불필요

### 향후 Vespa 확장 범위

| 유스케이스 | Vespa 스키마 |
|-----------|------------|
| MCP 툴 선택 (현재) | `mcp_tools` |
| RAG 문서 청크 | `rag_chunks` |
| 대화 시맨틱 검색 | `conversations` |
| 검색 시스템 (BM25 + 벡터 하이브리드) | `search_docs` |

---

## 구체적 예시

```
Client: "오늘 서울 날씨가 어때?"

[1] 쿼리 임베딩
    embed("오늘 서울 날씨가 어때?") → [0.12, -0.45, ...]

[2] Vespa ANN 검색 (service_id 필터)
    Top-8 유사 툴 반환:
      - mcp_weather_get_coordinates (score: 0.91)
      - mcp_weather_get_weather     (score: 0.89)
      - mcp_maps_geocode            (score: 0.72)

[3] LLM 호출 (8개 툴만 주입)
    LLM 판단: "get_coordinates, get_weather 필요"

[4] 기존 McpBridgeAdapter 루프 실행 (S11 그대로)
```

---

## 아키텍처

```
Client
  │ POST /v1/chat/completions
  ▼
McpBridgeAdapter (S11)
  │
  ├── [기존 S11] API key mcp_access 확인
  │
  ├── [신규] McpVectorSelector.select(query, service_id, top_k=16)
  │         │
  │         ├── embed(query) → EmbeddingService (/v1/embeddings, 기존 엔드포인트)
  │         │
  │         └── Vespa.query(schema="mcp_tools")
  │                 filter: service_id = "svc-abc"
  │                 ann: embedding → top_k
  │                 ← Vec<ScoredTool>
  │
  ├── [기존 S11] LLM 호출 (Top-K 툴만 주입)
  │
  └── [기존 S11] tool_call 루프
```

### 컴포넌트

| 컴포넌트 | 위치 | 역할 |
|---------|------|------|
| `McpVectorSelector` | `crates/veronex-mcp/src/vector/` | 쿼리 임베딩 + Vespa ANN 검색 |
| `McpToolIndexer` | `crates/veronex-mcp/src/vector/` | 툴 등록/업데이트 시 Vespa 인덱싱 |
| `VespaClient` | `crates/veronex-mcp/src/vector/` | Vespa HTTP 클라이언트 래퍼 |
| Vespa 인스턴스 | 인프라 (단일 노드 → 확장) | 벡터 SSOT |

---

## 데이터 모델

### Vespa 스키마: `mcp_tools`

```
schema mcp_tools {
    document mcp_tools {
        field tool_id         type string  { indexing: attribute | summary }
        field service_id      type string  { indexing: attribute | summary }
        field server_id       type string  { indexing: attribute | summary }
        field tool_name       type string  { indexing: attribute | summary }
        field description     type string  { indexing: index | summary }
        field input_schema    type string  { indexing: summary }
        field embedding       type tensor<float>(x[768]) {
            indexing: attribute | index
            attribute { distance-metric: angular }
            index { hnsw { max-links-per-node: 16 neighbors-to-explore-at-insert: 200 } }
        }
    }

    rank-profile semantic {
        first-phase { expression: closeness(field, embedding) }
    }
}
```

- **벡터 차원**: 768 (Ollama `nomic-embed-text` 기준), 모델 변경 시 스키마 재배포
- **거리 메트릭**: Angular (Cosine 동일)
- **멀티테넌트**: `service_id` attribute 필터로 격리

### 인덱싱 트리거

| 이벤트 | 동작 |
|-------|------|
| MCP 서버 등록 (`POST /mcp/servers`) | 툴 목록 전체 upsert |
| MCP 서버 헬스 복구 | 툴 목록 재동기화 |
| MCP 서버 삭제 | `service_id + server_id` 조건으로 삭제 |
| 툴 캐시 갱신 (`McpToolCache`) | 변경된 툴만 upsert |

---

## 설계 결정

### 1. Vespa = 벡터 SSOT

모든 벡터 연산은 Vespa 단일 인스턴스로 통합. pgvector, Qdrant 미사용.
추후 검색 시스템 도입 시 BM25 + 벡터 하이브리드를 동일 파이프라인에서 처리.

### 2. 단일 노드 시작 → 수평 확장

Vespa는 단일 노드로 시작 가능. 트래픽 증가 시 content node 추가로 확장.
K8s StatefulSet으로 배포, Longhorn PVC.

### 3. 임베딩은 기존 엔드포인트 재사용

`/v1/embeddings` (veronex-inference)가 이미 구현됨.
`McpVectorSelector`는 해당 엔드포인트를 내부 HTTP 호출.

### 4. top_k 기본값 16

LLM context 부담과 재현율 균형점.
`mcp_access` 권한에서 per-key 오버라이드 가능 (추후).

### 5. OrchestratorModelSelector 제거

벡터 선택 도입 후 `mcp_orchestrator_model` 설정 불필요. 구현 완료 후 제거.

---

## 구현 단계

### Phase 1: 인프라 (Vespa 배포)

- [ ] K8s StatefulSet + Longhorn PVC로 Vespa 단일 노드 배포
- [ ] `mcp_tools` 스키마 배포 (application package)
- [ ] 환경 변수: `VESPA_URL`, `VESPA_CERT` (mTLS, 선택)

### Phase 2: 클라이언트 구현

- [ ] `crates/veronex-mcp/src/vector/vespa_client.rs`
  - `feed(doc: McpToolDoc)` — upsert (Vespa document API)
  - `delete(service_id, server_id)` — delete by selection
  - `search(embedding, service_id, top_k)` — ANN query

### Phase 3: 인덱싱 파이프라인

- [x] `crates/veronex-mcp/src/vector/tool_indexer.rs`
  - `index_server_tools(service_id, server_id, tools)`
  - `remove_server_tools(service_id, server_id)`
- [x] `McpToolCache` 갱신 훅 연결 — `refresh()` → `Option<Vec<McpTool>>` 반환, main.rs에서 indexer 호출
- [x] MCP 서버 등록/삭제 핸들러에서 인덱서 호출

### Phase 4: 벡터 선택기

- [ ] `crates/veronex-mcp/src/vector/selector.rs`
  - `select(query: &str, service_id: &str, top_k: usize) -> Vec<McpTool>`
  - 임베딩 호출 → Vespa search → McpTool 역직렬화
- [ ] 임베딩 캐시 (Valkey, TTL 5분) — 동일 쿼리 재임베딩 방지

### Phase 5: McpBridgeAdapter 연결

- [x] `McpBridgeAdapter.run_loop()`에서 `McpToolCache.get_all()` → `McpVectorSelector.select()` 교체
- [x] 폴백: Vespa 장애 시 `McpToolCache.get_all()` + MAX_TOOLS=32 컷
- [x] `OrchestratorModelSelector` 관련 코드/설정 제거 — `mcp_orchestrator_model` DB 컬럼/API/로직 전체 삭제

### Phase 6: 검증

- [x] 단위 테스트: `McpVectorSelector` mock Vespa (WireMock)
- [x] 통합 테스트: 실제 Vespa 컨테이너 + 샘플 툴 (12-mcp.sh)
- [x] 부하 테스트: `scripts/e2e/14-vespa-load-test.sh` — 100K 툴 인덱싱 후 ANN p99 < 20ms 검증

---

## 환경 변수

```env
VESPA_URL=http://vespa:8080
VESPA_FEED_URL=http://vespa:8080          # document API
VESPA_QUERY_URL=http://vespa:8080         # query API
MCP_VECTOR_TOP_K=16
MCP_VECTOR_EMBED_CACHE_TTL_SECS=300
MCP_VECTOR_FALLBACK_ON_ERROR=true
```

---

## 성능 목표

| 지표 | 목표 |
|-----|-----|
| Vespa ANN 검색 레이턴시 | < 10ms p99 (100K 툴) |
| 임베딩 레이턴시 | < 50ms p99 (캐시 미스) |
| 전체 벡터 선택 레이턴시 | < 60ms p99 |
| 인덱싱 처리량 | 1K 툴/초 이상 |
| Vespa 사양 (단일 노드, 100K) | 4 vCPU / 8GB RAM |

---

## 의존성

- Vespa (Apache 2.0)
- `/v1/embeddings` 엔드포인트 (veronex-inference, 기존 구현)
- S11 `McpBridgeAdapter` (Phase 5 연결 대상)
- Longhorn PVC (K8s 스토리지)
