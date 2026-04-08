# Database — 2026 Research

> **Last Researched**: 2026-03-01 | **Source**: Implementation + PG docs
> **Status**: Partial — PG18 uuidv7 verified | ClickHouse optimization to research

---

## PostgreSQL 18 — uuidv7

```sql
-- Native uuidv7() function (PG18, no extension needed)
CREATE TABLE jobs (
  id UUID PRIMARY KEY DEFAULT uuidv7(),
  ...
);
```

**Why UUIDv7 over UUIDv4**: Time-ordered → better index locality, no random I/O scatter.
**Why native**: No `uuid-ossp` extension needed in PG18+.

Image: `postgres:18-alpine` (minimal, native uuidv7 support).

---

## sqlx — Key Patterns

```rust
// UUIDv7 in sqlx: use uuid::Uuid type, PG native function handles generation
let job = sqlx::query_as!(Job, "INSERT INTO jobs (...) VALUES (...) RETURNING *", ...)
    .fetch_one(&pool)
    .await?;

// Nullable FK: Option<Uuid> maps to nullable UUID column
server_id: Option<Uuid>   // NULL = no server linked
```

---

## Migrations (sqlx migrate)

```
crates/veronex/migrations/
  20260101000001_initial.sql
  20260228000033_api_key_type.sql
  20260228000034_accounts.sql
  ...
```

- Naming: `YYYYMMDD[N]_description.sql` (sequential within a day using counter suffix)
- `sqlx::migrate!()` called at startup — idempotent, ordered by filename
- Never edit existing migrations — always add a new one

---

## ClickHouse — Query Patterns

```sql
-- Anti-pattern: alias collision
SELECT sum(prompt_tokens) + sum(completion_tokens) AS total_tokens  -- ERROR if used in WHERE/HAVING

-- Fix: use subquery
SELECT * FROM (
  SELECT sum(prompt_tokens) + sum(completion_tokens) AS total_tokens FROM otel_logs WHERE ...
) WHERE total_tokens > 0
```

```sql
-- Time bucket queries: toStartOfInterval
SELECT toStartOfInterval(Timestamp, INTERVAL 1 MINUTE) AS bucket,
       avg(latency_ms) AS avg_latency
FROM otel_logs
WHERE event_name = 'inference.completed'
GROUP BY bucket
ORDER BY bucket
```

---

## To Research

## Valkey 8/9 — io-threads & New Features (2026)

> Updated: 2026-04-07

### io-threads — most important performance change

Valkey 8.0+ async I/O threading: I/O threads handle read, parse, write, and dealloc concurrently with the main thread.

```
# valkey.conf
io-threads 6            # core_count - 2 (e.g. 8 cores → 6 threads)
io-threads-do-reads yes
```

Benchmark (AWS c8g.2xlarge): Valkey 8.1 = 947K GET/s @ 0.21ms p50 vs. Redis 8.0 = 821K GET/s @ 0.44ms. ~15% throughput, ~52% latency improvement. TLS specifically: ~300% improvement in new connection rate.

### Valkey 9 — new features

| Feature | Usefulness for Veronex |
|---------|------------------------|
| Hash field expiration | Per-hash-field TTL — useful for per-provider slot leases without separate key namespace |
| Multiple logical DBs in cluster mode | Separate DB indices usable in cluster |
| Per-slot metrics | Prometheus-compatible cluster observability |
| Atomic slot migration | Safe cluster rebalancing |

**Hash field expiration example:**
```
HEXPIRE veronex:slots:{provider_id}:{model} 300 FIELDS 1 {instance_id}
```
Replaces the current ZSET-based slot lease expiration pattern.

### Memory efficiency

Valkey 8.1: ~20–30 bytes saved per key-value pair (redesigned hash table). At 50M keys ≈ 1GB savings.

## ClickHouse — MergeTree Partition & TTL (2026)

**Partition by date for O(1) retention enforcement:**
```sql
ENGINE = MergeTree()
PARTITION BY toYYYYMM(Timestamp)
ORDER BY (service_name, Timestamp)
TTL Timestamp + INTERVAL 30 DAY DELETE;
```

Each MV creates its own independent table — apply different retention windows per tier:
- Raw logs: 30 days
- Aggregated metrics: 1 year

`ALTER TABLE DROP PARTITION` is O(1) for manual cleanup. `TTL` clause handles automatic background deletion.

## Vespa 8 — ANN / HNSW Tuning (2026)

New ACORN-1 adaptive filtered ANN parameters (add to schema when using Vespa for vector search):

```
filter-first-threshold: 0.4    # activate ACORN-1 when >60% docs filtered (default: disabled)
exploration-slack: 0.05        # distance-based termination; 10-15% recall improvement
```

**Tensor type optimization:** float→bfloat16 halves memory with negligible recall impact. float→int8 further reduces cost.

**Tuning tool:** `pyVespa` ANN autotune sweeps recall/latency tradeoffs on representative data. All parameters are dataset-dependent — benchmark before committing.

---

## Sources

- PostgreSQL 18 release notes
- ClickHouse docs: https://clickhouse.com/docs
- [Redis 8.0 vs Valkey 8.1 comparison](https://www.dragonflydb.io/blog/redis-8-0-vs-valkey-8-1-a-technical-comparison)
- [Valkey key features 2025](https://www.dragonflydb.io/guides/valkey-key-features-pros-cons-and-comparison-with-redis-2025)
- [Vespa ACORN-1 + Adaptive Beam Search](https://blog.vespa.ai/additions-to-hnsw/)
- [Vespa ANN parameter tuning](https://blog.vespa.ai/tweaking-ann-parameters/)
- Verified: `crates/veronex/migrations/`, `crates/veronex-analytics/`
