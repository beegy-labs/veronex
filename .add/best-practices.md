# Best Practices

> ADD Execution | **Last Updated**: 2026-03-24

## 역할

이 파일은 두 가지 워크플로우를 담는다:

1. **갱신 워크플로우** — best practices 문서(`docs/llm/policies/`)를 언제, 어떻게 업데이트하는지
2. **리팩토링 워크플로우** — 기존 코드를 현행 best practices에 맞게 정렬하는 절차

---

## Part 1 — Best Practices 갱신

### 갱신 트리거

| 트리거 | 설명 | 대상 문서 |
|--------|------|----------|
| 코드 리뷰에서 동일 이슈가 2회 이상 반복 | 패턴으로 고정할 신호 | `patterns.md` |
| 새로운 architectural 결정 (ADR) | 구조 변경, 새 의존성 채택 | `architecture.md` |
| 보안/성능 사고 발생 후 교훈 도출 | 사고 재발 방지 규칙 | `patterns.md` 또는 `security.md` |
| 새 기술 스택 도입 (새 crate, 새 패턴) | 사용 규칙 문서화 | 해당 도메인 문서 |
| 분기별 정기 감사 (3개월마다) | 낡은 규칙 제거, 새 패턴 반영 | 전체 `docs/llm/policies/` |

### 어디에 무엇을 쓰는가

| 문서 | 담는 내용 |
|------|----------|
| `docs/llm/policies/patterns.md` | Rust 코드 패턴 (Valkey 키 레지스트리, DashMap 안전 사용, UTF-8 truncation 등) |
| `docs/llm/policies/architecture.md` | 레이어 구조, hexagonal architecture 경계, crate 간 의존 규칙 |
| `docs/llm/policies/testing-strategy.md` | 테스트 작성 규칙, 통합 테스트 vs 단위 테스트 기준 |
| `docs/llm/policies/security.md` (있는 경우) | SSRF 방어, 입력 검증, 에러 노출 규칙 |

### 갱신 절차

| Step | Action |
|------|--------|
| 1 | 갱신 트리거 확인 — 어떤 패턴/규칙을 추가/수정/삭제할지 명확히 정의 |
| 2 | 대상 문서(`docs/llm/policies/`) 해당 섹션 읽기 |
| 3 | 변경 내용을 간결하게 작성 — WHAT이 아닌 WHY + 적용 조건을 포함 |
| 4 | 새 규칙이 기존 코드를 위반하는지 `grep`으로 즉시 확인 |
| 5 | 위반 코드 발견 시 → Part 2 리팩토링 워크플로우로 진입 |
| 6 | 문서 `Last Updated` 날짜 갱신 |

### 갱신 주기 요약

| 주기 | 작업 |
|------|------|
| 즉시 (코드 리뷰 직후) | 반복 이슈 → `patterns.md`에 규칙 추가 |
| 즉시 (PR 머지 직후) | 새 패턴 확립 시 → 해당 도메인 문서 업데이트 |
| 분기 1회 | 전체 `docs/llm/policies/` 감사 — 낡은 규칙 제거, 실제 코드와 정합성 확인 |

---

## Part 2 — Best Practices 기반 코드 리팩토링

### Trigger

- 분기 감사에서 위반 코드 발견
- 코드 리뷰에서 반복 위반 패턴 발견
- 새 best practice 규칙 확립 후 기존 코드 정렬 필요

### Read Before Execution

| Domain | Path |
| ------ | ---- |
| Rust 패턴 | `docs/llm/policies/patterns.md` |
| 아키텍처 | `docs/llm/policies/architecture.md` |
| 테스트 전략 | `docs/llm/policies/testing-strategy.md` |
| 코드 리뷰 기준 | `.add/code-review.md` |

### 실행 절차

| Step | Action |
|------|--------|
| 1 | **범위 확정** — 어떤 규칙을 기준으로 리팩토링할지 명시. 전체 codebase vs 특정 모듈 |
| 2 | **위반 코드 탐색** — `grep` / `Glob`으로 위반 패턴 전수 조사 |
| 3 | **우선순위 분류** — P1(보안/정확성) → P2(아키텍처/성능) → P3(품질/가독성) |
| 4 | **Round-based 수정** — 한 번에 하나의 규칙, 한 번에 하나의 파일 그룹 |
| 5 | **Round마다 검증** — `cargo check --workspace` 통과 확인 |
| 6 | **전체 테스트** — `cargo nextest run --workspace` 모두 통과 |
| 7 | **CDD 동기화** — 리팩토링 과정에서 새 패턴 발견 시 policies 문서 업데이트 |

### 규칙

| 규칙 | 상세 |
|------|------|
| 동작 보존 | 리팩토링 중 로직 변경 금지. 출력/상태 전이/API 계약 불변 |
| Round 단위 | 작은 단계로 쪼개서 진행, 매 Round 후 `cargo check` |
| 스코프 제한 | 요청된 모듈/파일 외 범위 리팩토링 금지 |
| 테스트 통과 필수 | 모든 Round 종료 후 전체 테스트 그린 상태 유지 |
| 문서 동기화 | 새 패턴 확립 시 해당 policies 문서 즉시 업데이트 |

### 분기 감사 체크리스트

분기 1회 실행. 각 항목을 `grep`으로 검색하여 위반 여부 확인:

#### Valkey 키 레지스트리
```bash
# veronex crate에서 veronex:* 키를 직접 하드코딩하는지 확인
grep -rn '"veronex:' crates/veronex/src/ | grep -v valkey_keys
```
기대값: 0건 (valkey_keys.rs를 통해서만 생성)

#### DashMap .await 안전성
```bash
# DashMap Ref를 .await 건너 hold하는지 확인 (자동 감지 어려움 — 수동 검토)
grep -rn "\.iter()" crates/ --include="*.rs" | grep -v "\.collect()"
```
기대값: DashMap iter 후 `.collect()` 없이 `.await` 호출 없음

#### UTF-8 안전 truncation
```bash
# String::truncate 직접 호출 여부
grep -rn "\.truncate(" crates/ --include="*.rs"
```
기대값: 모두 `is_char_boundary()` 역방향 탐색 후 호출

#### SSRF 방어
```bash
# 외부 URL을 받는 핸들러에서 validate_provider_url 호출 여부
grep -rn "url.*String\|String.*url" crates/veronex/src/infrastructure/inbound/ --include="*.rs" -l
```
발견된 파일에서 `validate_provider_url` 호출 여부 수동 확인

#### 매직 넘버
```bash
# 타임아웃/TTL 관련 raw 숫자 리터럴
grep -rn "Duration::from_secs([0-9]" crates/ --include="*.rs" | grep -v "const "
```
기대값: 0건 (모든 Duration은 named const로 선언)
