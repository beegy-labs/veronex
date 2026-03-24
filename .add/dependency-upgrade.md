# Dependency Upgrade

> ADD Execution | **Last Updated**: 2026-03-24

## Trigger

의존성 버전 업데이트 요청, 또는 보안 취약점(CVE) 발견 시.

## Read Before Execution

| Domain | Path |
| ------ | ---- |
| Architecture | `docs/llm/policies/architecture.md` |
| Patterns (Rust) | `docs/llm/policies/patterns.md` |

---

## Step 0 — 실행 전 필수: 현재 날짜 확인 + 버전 검색

> **이 단계를 반드시 먼저 실행하라.** 아래 "업그레이드 현황" 섹션은 스냅샷이며 시간이 지나면 낡는다.
> 최신 버전은 항상 실행 시점에 웹 검색으로 확인해야 한다.

### 0-A. 현재 날짜 확인

```bash
date "+%Y-%m-%d"
```

### 0-B. 현재 사용 중인 버전 수집

```bash
# workspace + crate별 Cargo.toml
grep -hE "^(axum|sqlx|fred|jsonwebtoken|reqwest|argon2|opentelemetry|tracing|tokio|sha2|dashmap|async-trait)" \
  crates/veronex/Cargo.toml \
  crates/veronex-mcp/Cargo.toml \
  crates/veronex-agent/Cargo.toml \
  crates/veronex-analytics/Cargo.toml \
  | sort -u
```

### 0-C. 최신 버전 웹 검색

각 핵심 크레이트에 대해 아래 쿼리로 웹 검색을 수행한다:

| 크레이트 | 검색 쿼리 |
|---------|----------|
| opentelemetry 번들 | `"opentelemetry rust crate latest stable {현재연도}"` |
| jsonwebtoken | `"jsonwebtoken rust crate latest version {현재연도}"` |
| axum | `"axum tokio-rs latest version {현재연도}"` |
| sqlx | `"sqlx latest stable version {현재연도}"` |
| fred | `"fred redis rust crate latest {현재연도}"` |
| reqwest | `"reqwest rust crate latest {현재연도}"` |
| 보안 CVE | `"CVE rust axum {현재연도}"`, `"CVE sqlx {현재연도}"` |

> `{현재연도}`에 Step 0-A에서 확인한 연도를 삽입한다.

### 0-D. 버전 비교 후 현황 섹션 갱신

검색 결과로 아래 "업그레이드 현황" 표를 업데이트한 뒤 실행한다.
`Last Updated` 날짜도 오늘 날짜로 수정한다.

---

## 2026-03-24 기준 업그레이드 현황

> 웹 검색 기반 최신 버전 조사 결과. 다음 업그레이드 사이클 때 이 섹션을 갱신할 것.

### Rust 크레이트

| 크레이트 | 현재 | 최신 안정 | 우선순위 | 비고 |
|---------|------|----------|---------|------|
| `opentelemetry` | ~~0.27~~ | **0.31.0** ✅ | ~~🔴 긴급~~ 완료 | `TracerProvider`→`SdkTracerProvider`, runtime arg 제거 |
| `opentelemetry_sdk` | ~~0.27~~ | **0.31.0** ✅ | ~~🔴 긴급~~ 완료 | `features = ["rt-tokio"]` 유지 |
| `opentelemetry-otlp` | ~~0.27~~ | **0.31.1** ✅ | ~~🔴 긴급~~ 완료 | `features = ["grpc-tonic"]` 명시 필수 |
| `tracing-opentelemetry` | ~~0.28~~ | **0.32.1** ✅ | ~~🔴 긴급~~ 완료 | 위 3개와 동시 적용 |
| `jsonwebtoken` | ~~9~~ | **10.3.0** ✅ | ~~🟠 높음~~ 완료 | v10 기본 backend aws_lc_rs, API 호환 |
| `rand` | ~~0.8~~ | **0.9.x** ✅ | ~~🟠 높음~~ 완료 | `thread_rng()` → `rng()` (api_key_generator.rs) |
| `async-trait` | 0.1 | 유지 | 🟡 중간 | `dyn Trait` DI용으로만 유지, 나머지 native async fn 가능 |
| `base64` | 0.22 | 0.22.x | ✅ 최신 | semver 안정 |
| `axum` | 0.8 | 0.8.x | ✅ 최신 | 현재 major 최신 |
| `tokio` | 1 | 1.x | ✅ 최신 | semver 안정 |
| `tower` | 0.5 | 0.5.x | ✅ 최신 | — |
| `tower-http` | 0.6 | 0.6.x | ✅ 최신 | — |
| `sqlx` | 0.8 | 0.8.6 | ✅ 최신 | 0.9-alpha 관찰 중 (stable 아님) |
| `fred` | 10 | 10.1.0 | ✅ 최신 | — |
| `reqwest` | 0.13 | 0.13.x | ✅ 최신 | — |
| `argon2` | 0.5 | 0.5.x | ✅ 최신 | — |
| `sha2` / `blake2` | 0.10 | 0.10.x | ✅ 최신 | — |
| `thiserror` | 2 | 2.x | ✅ 최신 | — |
| `tracing` | 0.1 | 0.1.x | ✅ 최신 | — |
| `tracing-subscriber` | 0.3 | 0.3.x | ✅ 최신 | — |
| `dashmap` | 6 | 6.x | ✅ 최신 | — |
| `mimalloc` | 0.1 | 0.1.x | ✅ 최신 | — |
| `chrono` | 0.4 | 0.4.x | ✅ 최신 | — |
| `uuid` | 1 | 1.x | ✅ 최신 | — |
| `anyhow` | 1 | 1.x | ✅ 최신 | — |

---

## 업그레이드 실행 순서

### Phase 1 — 긴급 (즉시)

#### ✅ 1-A. opentelemetry 0.27 → 0.31 (4개 크레이트 동시) — 완료 2026-03-24

> 4개 크레이트는 반드시 같은 커밋에 함께 변경해야 한다. 부분 업그레이드 시 컴파일 오류 발생.

`crates/veronex/Cargo.toml` 수정:
```toml
opentelemetry = "0.31"
opentelemetry-otlp = "0.31"
opentelemetry_sdk = { version = "0.31", features = ["rt-tokio"] }
tracing-opentelemetry = "0.32"
```

파괴적 변경 대응:

| 변경 사항 | 이전 (0.27) | 이후 (0.31) |
|-----------|------------|------------|
| Provider 타입명 | `TracerProvider` | `SdkTracerProvider` |
| Tracer 반환 타입 | `opentelemetry_sdk::trace::Tracer` | `opentelemetry_sdk::trace::SdkTracer` |
| batch_exporter | `.with_batch_exporter(exp, runtime::Tokio)` | `.with_batch_exporter(exp)` |
| grpc-tonic 피처 | 기본 포함 | `features = ["grpc-tonic"]` 명시 필수 |
| Provider 종료 | `global::shutdown_tracer_provider()` | `tracer_provider.shutdown()` |
| `set_tracer_provider()` 반환값 | `Some(이전 provider)` | `()` |

영향 파일: `crates/veronex/src/main.rs` (`build_otlp_tracer()` 함수)

검증: `RUSTC_WRAPPER="" cargo check` → `cargo nextest run --workspace`

---

### Phase 2 — 높음 (1주 이내)

#### ✅ 2-A. `jsonwebtoken` 9 → 10.3.0 — 완료 2026-03-24

`crates/veronex/Cargo.toml` 수정:
```toml
jsonwebtoken = { version = "10", default-features = false }
```

이유: v10은 암호화 백엔드 명시 필수. `reqwest 0.13`이 기본으로 `rustls`를 사용하므로
`ring` 계열로 맞춰 바이너리 크기 중복 방지.

영향 파일: `crates/veronex/src/infrastructure/inbound/http/auth_handlers.rs`

검증: JWT 발급/검증 단위 테스트 확인

---

#### ✅ 2-B. `rand` 0.8 → 0.9 — 완료 2026-03-24

`rand 0.9`에서 `thread_rng()` API가 `rng()` / `rand::rng()` 로 변경됨.

```bash
grep -rn "thread_rng\|rand::thread_rng" crates/
```

발견된 사용처 전부 `rand::rng()` 로 교체 후 검증.

---

### Phase 3 — 중간 (2주 이내)

#### 3-A. `async-trait` 감사 및 최소화

규칙:
- `Arc<dyn Trait>` / `Box<dyn Trait>` DI로 사용되는 Port trait → `async-trait` 유지 필수
- 구체 타입 또는 제네릭 바운드로만 쓰이는 trait → native async fn 전환 가능

```bash
grep -rn "#\[async_trait\]" crates/ | wc -l
```

veronex의 Port trait은 전부 `Arc<dyn ...>` DI이므로 대부분 유지.
native async fn 전환 가능한 경우만 선택적 제거.

---

## 검증 체크리스트

업그레이드 완료 후 반드시 실행:

- [ ] `RUSTC_WRAPPER="" cargo clippy --all-targets` — 0 warnings
- [ ] `RUSTC_WRAPPER="" cargo check --workspace` — 컴파일 성공
- [ ] `RUSTC_WRAPPER="" cargo nextest run --workspace` — 전체 통과
- [ ] `.add/dependency-upgrade.md` 의 `Last Updated` 날짜를 오늘 날짜 (`date "+%Y-%m-%d"`)로 갱신
- [ ] "업그레이드 현황" 표에서 완료된 항목을 ✅ 로 마크

---

## 규칙

| 규칙 | 상세 |
|------|------|
| 동시 업그레이드 금지 | Phase 단위로 하나씩, 검증 후 다음 단계 |
| OTel 4개 동시 | opentelemetry 4개 크레이트는 반드시 같은 커밋에 함께 변경 |
| 파괴적 변경 먼저 감사 | 업그레이드 전 항상 CHANGELOG 또는 migration guide 확인 |
| 테스트 통과 필수 | 각 Phase 끝에 전체 검증 체크리스트 실행 |
| 날짜 갱신 필수 | 실행 후 반드시 `Last Updated` 업데이트 |
