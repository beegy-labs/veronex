# Rust Performance Best Practices (2026)

> **Tier 2 CDD** | Editable | Last Updated: 2026-03-03
>
> Runtime performance optimizations applied to Veronex.
> No functional changes — same API responses and DB results.

## 1. mimalloc Global Allocator

**What**: Replace the default system allocator (glibc malloc / macOS libmalloc) with [mimalloc](https://github.com/microsoft/mimalloc).

**Why**: mimalloc delivers up to 5.3x faster multi-threaded small allocations compared to glibc. In 2026, it is the production standard for high-throughput Rust services (rust-analyzer, TiKV, Meilisearch). Tokio-based servers allocate heavily on hot paths (request parsing, JSON serialization, SSE framing).

**Config**: `mimalloc = { version = "0.1", default-features = false }` — no secure/debug features in production.

## 2. Release Build Profile (LTO + codegen-units=1)

**What**: Enable link-time optimization and single codegen unit for release builds.

```toml
[profile.release]
lto = true
codegen-units = 1
```

**Why**: LTO allows LLVM to inline across crate boundaries, eliminating monomorphization overhead in generic-heavy code (serde, sqlx, axum extractors). `codegen-units = 1` trades compile time for 10-20% runtime improvement by enabling whole-program optimization. Recommended by the [Rust Performance Book](https://nnethercote.github.io/perf-book/).

**Trade-off**: Release build time increases significantly. Debug builds are unaffected.

## 3. Streaming Hash (Zero-Allocation Hashing)

**What**: Replace `serde_json::to_string()` + `hasher.update(bytes)` with `serde_json::to_writer(HashWriter, value)`.

**Why**: The previous approach allocated 2-4 intermediate `String`s per job (full messages + prefix). `HashWriter` implements `io::Write` by forwarding bytes directly to `Digest::update()`, eliminating all intermediate allocations. For large conversation contexts (10K+ tokens), this avoids multi-KB heap allocations on every job.

## 4. Enum `as_str()` (Static String Returns)

**What**: Add `as_str() -> &'static str` methods to `FinishReason` and `JobStatus` enums.

**Why**: Three observability adapters used `format!("{:?}", enum).to_lowercase()` — allocating a new `String` on every inference event. `as_str()` returns a `&'static str` with zero allocation. This is a per-inference hot path.

## 5. Dashboard HashMap Initialization

**What**: Use iterator `.collect()` instead of 5 individual `.insert(s.to_string(), 0)` calls.

**Why**: Minor improvement — reduces 5 `to_string()` calls to a single `.collect()` with `to_owned()`. More idiomatic Rust.

## 6. Tool Calls `reserve()`

**What**: Call `Vec::reserve(arr.len())` before extending accumulated tool calls.

**Why**: Without `reserve()`, the Vec may reallocate multiple times as tool call chunks arrive during streaming. Each reallocation copies all existing elements. `reserve()` ensures at most one allocation per chunk.

## Where Applied

| Optimization | File(s) |
|---|---|
| mimalloc | `crates/veronex/Cargo.toml`, `crates/veronex/src/main.rs` |
| LTO + codegen-units=1 | `Cargo.toml` (workspace `[profile.release]`) |
| Streaming hash (`HashWriter`) | `domain/services/message_hashing.rs` |
| `FinishReason::as_str()` | `domain/enums.rs` → `observability/{http,redpanda,clickhouse}_adapter.rs` |
| `JobStatus::as_str()` | `domain/enums.rs` (available, not yet called) |
| HashMap `.collect()` | `infrastructure/inbound/http/dashboard_handlers.rs` |
| `reserve()` | `application/use_cases/inference.rs` (tool calls accumulation) |

## References

- [Rust Performance Book](https://nnethercote.github.io/perf-book/) — LTO, codegen-units, allocator guidance
- [mimalloc](https://github.com/microsoft/mimalloc) — Microsoft's general-purpose allocator
- [serde_json::to_writer](https://docs.rs/serde_json/latest/serde_json/fn.to_writer.html) — streaming serialization
