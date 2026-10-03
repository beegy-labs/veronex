# Tasks: 2026-Refactor-bestpractices

> L3: ADD 자율 실행 | scopes/2026-Refactor-bestpractices.md 기반 | **Created**: 2026-05-16
> 단일 codex 세션. 각 Phase 끝에서 verify→commit→다음 Phase. 실패 시 최대 3회 재시도.

## Phase 0 — WIP 베이스라인 검증·커밋

- [ ] 현재 워킹트리(75 파일: 쿼리 SSOT 추출 17 신규 + Ollama→llama-server 레거시 제거) 대상
  - `cargo check --workspace` → `cargo clippy --all-targets` → `cargo nextest run --workspace`
  - `npx tsc --noEmit`(web) 그린 확인. 레드면 **최소** 수정으로 그린화(범위 확장 금지)
- [ ] 단일 커밋(C1 준수). 메시지 예: `refactor(persistence): extract SQL into per-domain *_queries SSOT; purge llama-server-legacy paths`
- [ ] 베이스라인 SHA 기록

## Phase 1 — 2026 best practices 조사 + CDD 갱신

- [ ] web search: Rust(axum 0.8/0.9, sqlx 0.8/0.9, tokio 1.x, edition 2024 idiom, async-trait 대체),
      React 19 / Next 16, TanStack Query v5, vitest 4, verodesign 통합 — 2026 안정 권장 패턴·함수 사용법
- [ ] 확정된 패턴만 라우팅대로 반영:
  - Rust 패턴 → `docs/llm/policies/patterns.md`
  - FE 패턴 → `docs/llm/policies/patterns-frontend.md`
  - 테스트 패턴 → `docs/llm/policies/testing-strategy.md`
  - 아키텍처 경계 → `docs/llm/policies/architecture.md`
  - 스택 인벤토리/버전 changelog → `.add/skills.md`
- [ ] WHY + 적용 조건 포함, 간결하게. `Last Updated` 갱신. `docs/en|kr/` 손대지 말 것(C9)

## Phase 2 — 의존성 major 포함 최신화  ⚠️ DEFERRED (2026-05-16)

> 이 환경은 호스트·샌드박스 모두 네트워크 차단 → crate/npm fetch·버전 web search 불가.
> 사용자 결정으로 Phase 2 연기. 네트워크 가용 환경에서 별도 수행. Phase 3·4 먼저 진행.


- [ ] `.add/dependency-upgrade.md` Step 0 실행: 현재 버전 수집 + latest stable web search(Rust+npm) + CVE 스캔
- [ ] Rust: axum/sqlx/fred/reqwest/jsonwebtoken/opentelemetry(4종 동시)/tokio/thiserror 등 major 포함 최신화.
      Port 트레잇 `Arc<dyn>` DI는 `async-trait` 유지(concrete-only만 native async fn)
- [ ] npm: `.add/dependency-upgrade.md` Upgrade Order대로 (next→react→tailwind-merge+tailwindcss 동시→
      lucide-react(브랜드 아이콘 감사)→vitest4→playwright→react-query→jsdom→@types/node→typescript 6)
- [ ] 마이그레이션 노트(tailwind-merge v3, lucide v1 aria-hidden/브랜드아이콘, vitest v4 config) 적용
- [ ] Phase 단위 verify(Rust 체크리스트 / npm 체크리스트). status 표 + `Last Updated` 갱신
- [ ] semver 임의 bump 금지(C6) — 라이브러리/패키지 자체 버전 표기 변경만, 산출물 버전은 보고
- [ ] 커밋(C1)

## Phase 3 — 백엔드 SSOT·deadcode·중복함수 제거

- [ ] `.add/audit-backend.md` P1→P2→P3→P4 grep 블록 실행, `.add/audit-security.md` P0–P2 동반
- [ ] 우선순위 P1(보안/정합성)→P2(아키텍처/성능)→P3(품질). 라운드별 한 규칙·한 파일그룹
  - deadcode: 미사용 fn/struct/모듈/feature flag, llama-server 잔재 제거
  - 중복함수: 동일 시그니처/로직 통합 → 단일 SSOT(쿼리는 Phase 0의 `*_queries.rs` 패턴으로 일원화)
  - SSOT: 분산된 상수/설정/쿼리/에러 매핑 일원화
- [ ] behavior-preserving(C2), 라운드마다 `cargo check`→끝에 `cargo nextest run --workspace`+`cargo deny check`
- [ ] sync-ssot(C7)·스케일(C8) 위반 도입 금지. 신규 안정 패턴 확정 시 CDD 반영
- [ ] 커밋(C1)

## Phase 4 — 프론트엔드 verodesign 통합·SSOT·중복 제거

- [ ] `.add/audit-frontend.md` P1→P2→P3 grep 블록 실행
- [ ] verodesign 통합(C4/C5): tailwind/raw 리터럴 → `--vds-theme-*`/`vds-*`, 로컬 프리미티브 → `@verobee/design-react`.
      `@verobee/design-react`에 없는 갭은 **목록으로 보고만**(로컬 신설 금지)
- [ ] deadcode(미사용 hook/component/type, Ollama 잔재) 제거, 중복 hook/util/component 통합 → SSOT
- [ ] behavior-preserving(C2), 라운드마다 `npx tsc --noEmit`→끝에 `npx vitest run`+`npx playwright test`
- [ ] 신규 안정 FE 패턴 확정 시 `patterns-frontend.md`/`design-system*.md` 반영
- [ ] 커밋(C1)

## Phase 5 — 최종 검증·CDD sync·요약

- [ ] 전체 verify 스위트(Acceptance 전 항목) 그린 확인
- [ ] `.add/best-practices.md` Part 1 라우팅대로 CDD 최종 sync, 각 문서 `Last Updated`
- [ ] 압축 요약 반환(≤400 token): Phase별 커밋 SHA, 변경 파일 path+1줄 의도, 테스트 결과(pass/fail+실패명),
      스코프 deviation, verodesign 갭 목록, semver 판단 필요 항목

## 보고 규칙

- 코드/diff/빌드로그 붙여넣기 금지. 파일 path + 1줄 의도만
- deviation·갭·semver 판단 필요 항목은 명시 플래그
- 각 Phase 3회 재시도 실패 또는 "scope unclear" → 중단하고 Claude로 반환
