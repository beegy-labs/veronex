# Code Patterns: Rust — Build, Test & Utility Conventions

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns.md`](../patterns.md)

## Docker Build Cache — `sharing=locked`

All `--mount=type=cache` directives for the Cargo registry and target directory must use `sharing=locked`:

```dockerfile
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release -p my-crate --recipe-path recipe.json
```

Without `sharing=locked`, parallel `docker compose build` services extracting the same crates simultaneously cause `EEXIST (os error 17)` failures. Apply to both `cargo chef cook` and `cargo build` steps in every service Dockerfile.

## Test Code Conventions

| Rule | Rationale |
|------|-----------|
| **Pure function tests** | No external state (env, fs, network, shared mutex) — `cargo test` parallel safe |
| **Avoid duplicate tests** | Merge tests verifying the same property (e.g., if determinism ⊂ uniqueness, keep only the uniqueness test) |
| **1 test = 1 property** | Each test verifies one unique property — the name is the spec |
| **env var tests** | Never call `env::var()` directly → validate parsing logic inline only (prevents race conditions) |
| **DOS boundary values** | Cap tests for `MAX_*` constants are required |

```rust
// Good: pure, individual, non-overlapping
#[test]
fn no_duplicates() {  // uniqueness check (implies determinism)
    for id in &["a", "b", "c"] {
        let owners: Vec<u32> = (0..3).filter(|&o| owns(id, o, 3)).collect();
        assert_eq!(owners.len(), 1);
    }
}

// Bad: duplicate (subset of the above test)
#[test]
fn deterministic_assignment() {  // determinism is trivial once uniqueness is proven
    assert!(owns("a", owner, 3));
}
```

## UTF-8 Safe Truncation

All string truncation must respect UTF-8 char boundaries. Use the shared utility in `veronex_mcp::truncate_at_char_boundary` instead of calling `String::truncate(n)` directly.

```rust
// CORRECT — via shared utility (veronex-mcp crate)
use veronex_mcp::truncate_at_char_boundary;
truncate_at_char_boundary(&mut s, MAX_BYTES);

// CORRECT — inline (when veronex_mcp not in scope)
let boundary = (0..=max_len).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0);
s.truncate(boundary);

// WRONG — panics on multi-byte char boundaries
s.truncate(MAX_BYTES);
```

The audit grep:
```bash
grep -rn "\.truncate(" crates/ --include="*.rs"
```
Expected: all calls preceded by `is_char_boundary()` reverse-scan or delegated to `truncate_at_char_boundary()`.

