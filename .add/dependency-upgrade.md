# Dependency Upgrade

> ADD Execution | **Last Updated**: 2026-03-24

## Trigger

Dependency version update request or CVE discovered.

## Step 0 — Before execution: collect versions + web search

> Run this step first. The status table below is a snapshot and becomes stale.

### 0-A. Collect current versions

```bash
grep -hE "^(axum|sqlx|fred|jsonwebtoken|reqwest|argon2|opentelemetry|tracing|tokio|sha2|dashmap|async-trait)" \
  crates/veronex/Cargo.toml \
  crates/veronex-mcp/Cargo.toml \
  crates/veronex-agent/Cargo.toml \
  crates/veronex-analytics/Cargo.toml \
  | sort -u
```

### 0-B. Web search for latest versions

| Crate | Search query |
|-------|-------------|
| opentelemetry bundle | `"opentelemetry rust crate latest stable {year}"` |
| jsonwebtoken | `"jsonwebtoken rust crate latest version {year}"` |
| axum | `"axum tokio-rs latest version {year}"` |
| sqlx | `"sqlx latest stable version {year}"` |
| fred | `"fred redis rust crate latest {year}"` |
| CVE scan | `"CVE rust axum {year}"`, `"CVE sqlx {year}"` |

### 0-C. Update status table below, then proceed

---

## Status as of 2026-03-24

| Crate | Current | Latest stable | Status |
|-------|---------|--------------|--------|
| `opentelemetry` bundle (4 crates) | 0.31 | 0.31.x | done |
| `jsonwebtoken` | 10.3.0 | 10.3.x | done |
| `rand` | 0.9 | 0.9.x | done |
| `async-trait` | 0.1 | keep | pending — DI only (see Phase 3) |
| `axum` | 0.8 | 0.8.x | current |
| `sqlx` | 0.8 | 0.8.6 | current (0.9-alpha: watch only) |
| `fred` | 10 | 10.1.x | current |
| `reqwest` | 0.13 | 0.13.x | current |
| `tokio` | 1 | 1.x | current |
| `thiserror` | 2 | 2.x | current |

---

## Phase 3 — async-trait audit (pending)

Rule: `Arc<dyn Trait>` DI Port traits must keep `async-trait`. Concrete-type-only traits can migrate to native async fn.

```bash
grep -rn "#\[async_trait\]" crates/ | wc -l
```

All veronex Port traits use `Arc<dyn ...>` DI — most must stay. Only selectively remove where concrete types are used throughout.

---

## Verification checklist

- [ ] `cargo clippy --all-targets` — 0 warnings
- [ ] `cargo check --workspace` — compiles
- [ ] `cargo nextest run --workspace` — all pass
- [ ] Update `Last Updated` date in this file
- [ ] Mark completed items as `done` in status table

## Rules

| Rule | Detail |
|------|--------|
| One phase at a time | Verify before proceeding to next phase |
| OTel 4 crates together | Must be updated in the same commit |
| Breaking changes first | Read CHANGELOG/migration guide before upgrading |
| Tests must pass | Run full verification checklist after each phase |
