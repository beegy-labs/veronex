# Hot-Path Caching Strategy

> SSOT | **Last Updated**: 2026-03-26

Inference API (`POST /v1/chat/completions`) 경로에서 매 요청마다 실행되는 RDBMS 쿼리를 제거하기 위한 캐싱 계층 정리.
Scale target: 10K providers, 1M TPS.

---

## 핫패스 쿼리 전체 현황

| 우선순위 | 위치 | 쿼리 | 캐시 | 상태 |
|----------|------|-------|------|------|
| P1 | `infer_auth` 미들웨어 | `SELECT FROM api_keys WHERE key_hash = $1` | TtlCache 60s | ✅ 완료 |
| P1 | `openai_handlers.rs:293` | `SELECT FROM lab_settings WHERE id = 1` (이미지) | TtlCache 30s | ✅ 완료 |
| P1 | `openai_handlers.rs:604` | `SELECT FROM lab_settings WHERE id = 1` (MCP) | TtlCache 30s | ✅ 완료 |
| P1 | `bridge.rs` `run_loop()` | `SELECT server_id FROM mcp_key_access` | Valkey 60s | ✅ 완료 |
| P1 | `openai_handlers.rs` MCP 사전 체크 | `COUNT(*) FROM mcp_key_access` | Valkey 60s | ✅ 완료 |
| — | `inference_jobs` INSERT | 매 요청 필수 write | 캐싱 불가 (정합성 필요) | 유지 |

---

## 캐싱 구현

### CachingApiKeyRepo

- **위치**: `infrastructure/outbound/persistence/caching_api_key_repo.rs`
- **TTL**: 60s (인스턴스 내 in-memory TtlCache)
- **캐시 키**: `key_hash` (BLAKE2b-256 — 민감정보 아님)
- **캐시 값**: `Option<ApiKey>` (None도 캐시 — negative cache)
- **무효화**: revoke / set_active / soft_delete / regenerate / update_fields / set_tier → `invalidate_all()`
- **쓰기 경로 (create)**: 캐시 업데이트 없음 (다음 인증 시 자동 채워짐)

**적용 효과**: 동일 API 키 인증 요청이 60초 내 반복 시 DB 쿼리 0회.

```rust
// bootstrap/repositories.rs
let api_key_repo: Arc<dyn ApiKeyRepository> =
    Arc::new(CachingApiKeyRepo::new(Arc::new(
        PostgresApiKeyRepository::new(pg_pool.clone()),
    )));
```

### CachingLabSettingsRepo

- **위치**: `infrastructure/outbound/persistence/caching_lab_settings_repo.rs`
- **TTL**: 30s (인스턴스 내 in-memory TtlCache)
- **캐시 키**: `()` (전역 싱글턴 — `lab_settings` 테이블은 `id=1` 단일 행)
- **무효화**: `update()` 후 `invalidate_all()` — 설정 변경 즉시 반영

**적용 효과**: 이미지 요청 및 MCP 요청의 lab_settings DB 조회가 30초 TTL 내 0회로 감소.

```rust
// bootstrap/repositories.rs
let lab_settings_repo: Arc<dyn LabSettingsRepository> =
    Arc::new(CachingLabSettingsRepo::new(Arc::new(
        PostgresLabSettingsRepository::new(pg_pool.clone()),
    )));
```

### MCP ACL Valkey 캐시

- **위치**: `infrastructure/outbound/mcp/bridge.rs` `fetch_mcp_acl()`
- **Valkey 키**: `veronex:mcp:acl:{api_key_id}` (TTL 60s)
- **값**: JSON UUID 배열 — 허용된 MCP server ID 목록
- **빈 배열도 캐시** — 권한 없는 키의 반복 DB 조회 차단 (negative cache)
- **무효화**: `key_mcp_access_handlers.rs` grant/revoke 시 `DEL` 명시 호출

---

## TtlCache 패턴 (공통)

`infrastructure/outbound/persistence/ttl_cache.rs` — 모든 캐싱 래퍼에서 공유.

| 특성 | 내용 |
|------|------|
| 구현 | `RwLock<HashMap<K, (V, Instant)>>` |
| Read path | 읽기 락 (fast path) |
| Miss path | 쓰기 락 + double-check re-entry |
| Thundering herd | double-check로 차단 |
| 멀티 인스턴스 | 인스턴스별 독립 캐시 — eventual consistency |

**기존 TtlCache 사용처**:

| 래퍼 | TTL | 용도 |
|------|-----|------|
| `CachingOllamaModelRepo` | 10s | 모델→provider 매핑 (dispatch 핫패스) |
| `CachingModelSelection` | 30s | 모델 활성화 여부 |
| `CachingProviderRegistry` | 5s | provider 목록 스냅샷 |
| `CachingApiKeyRepo` | 60s | API 키 인증 (추론 핫패스) |
| `CachingLabSettingsRepo` | 30s | 실험 기능 설정 (이미지/MCP 핫패스) |

---

## 장기 방향 (미구현)

| 항목 | 내용 |
|------|------|
| `inference_jobs` | 현재 PG — 라우팅 데이터를 Valkey, 분석 데이터를 ClickHouse로 이관 검토 (장기) |
| API 키 Valkey 캐시 | 현재 in-memory TtlCache — 멀티 인스턴스 즉시 무효화가 필요해지면 Valkey로 업그레이드 |
| Kafka → ClickHouse | `inference_jobs` write는 ClickHouse 적합, 단 워커 읽기 경로는 Valkey 필요 |
