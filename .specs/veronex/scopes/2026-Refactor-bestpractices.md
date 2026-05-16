# Scope: 2026-Refactor-bestpractices — best-practices + deps + SSOT/deadcode/dup refactor

> L2: 승인 완료 (사용자 지시) | branch `feat/ai-baas-refactoring-plan` | **Created**: 2026-05-16

## Change Summary

3단계를 한 번의 위임으로 순차 수행한다:

1. 2026 best practices(개발 방법·함수 사용법) 조사 후 CDD 정책 문서 갱신
2. Rust crate + npm 의존성을 **major 포함 latest stable**로 최신화
3. `.add` 감사 워크플로우 기반으로 백엔드(SSOT·deadcode·중복함수 제거)와
   프론트엔드(verodesign 디자인 통합·SSOT·코드/중복함수 제거) 리팩토링

순서는 best-practices 조사 → 의존성 → 백엔드 → 프론트엔드. 단, 시작 전에
현재 커밋되지 않은 WIP(persistence 쿼리 SSOT 추출 + Ollama→llama-server 레거시 제거)를
검증 후 베이스라인으로 커밋한다.

## Change Type

Change (behavior-preserving refactor) + Dependency upgrade + Docs

## CDD References

- `.ai/README.md`, `.ai/architecture.md`, `.ai/rules.md`
- `docs/llm/policies/patterns.md`, `patterns-frontend.md`, `architecture.md`, `testing-strategy.md`
- `docs/llm/frontend/design-system*.md`, `execution-contracts.md`
- `.add/best-practices.md`, `dependency-upgrade.md`, `audit-backend.md`,
  `audit-frontend.md`, `audit-security.md`, `code-review.md`, `refactor.md`, `skills.md`

## Hard Constraints (위반 금지)

| # | 제약 | 근거 |
|---|------|------|
| C1 | 커밋·PR·코드에 AI/LLM/Claude/Codex 언급 및 co-author 금지 | 프로젝트 정책 |
| C2 | behavior-preserving — 리팩토링 중 로직 변경 없음 | `refactor.md` |
| C3 | 라운드 기반 — 라운드마다 verify, 그린 상태 유지 | `best-practices.md` Part 2 |
| C4 | 프론트는 verodesign만 사용. tailwind/raw color·spacing 리터럴 금지. `vds-*` 유틸리티는 디자인 시스템 본체 — 제거·인라인 style 변환 금지 | verodesign-only 원칙 |
| C5 | Button/Frame/Card 등 디자인 프리미티브 로컬 재발명 금지 — `@verobee/design-react` 소비. 갭은 보고만(구현 X) | no-local-design-primitives |
| C6 | semver 자동 +1 금지 — 변경 범위 보고 후 사용자 판단 대상으로 남김(임의 bump X) | semver-intentional |
| C7 | 외부 API sync 호출은 단일 오케스트레이터 한 곳에서만 — 페이지/위젯은 read-only 구독 유지 | sync-ssot |
| C8 | 스케일 타깃 비협상: 10K providers / 1K+ MCP / 1M TPS. O(N) DB 스캔·순차 await·무한 메모리 증가 도입 금지 | `.add/README.md` |
| C9 | `docs/en/`, `docs/kr/` 수동 편집 금지(자동 생성). `docs/llm/`만 갱신 | CLAUDE.md |
| C10 | 스코프 밖 모듈 리팩토링 금지 — 본 spec 범위만 | `best-practices.md` |

## Out of Scope

- 신규 기능 추가, AIMD/ProcessManager 알고리즘 변경(Phase 3/4 별도 spec 소관)
- DB 스키마 마이그레이션(불가피한 deadcode 컬럼은 보고만)
- 인프라/도커/프로비저닝 시스템 변경

## Acceptance

- `cargo check --workspace` / `cargo clippy --all-targets`(0 warn) / `cargo nextest run --workspace` 그린
- `npx tsc --noEmit`(0 err) / `npx vitest run` / `npx playwright test` 그린
- `cargo deny check` 통과
- 의존성 status 표(`.add/dependency-upgrade.md`) 갱신 + `Last Updated`
- 변경된 패턴이 `docs/llm/policies/`에 반영(`best-practices.md` Part 1 라우팅)
- 단계별 커밋(C1 준수), Layer 2 fresh-eyes 리뷰 APPROVE
