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

- [ ] PostgreSQL 18 — new features beyond uuidv7 (MERGE, COPY BINARY improvements)
- [ ] ClickHouse MergeTree partition strategies for time-series (TTL, partition by month)
- [ ] ClickHouse `ASOF JOIN` for time-series lookups
- [ ] sqlx 0.9 — any breaking changes or new features

---

## Sources

- PostgreSQL 18 release notes
- ClickHouse docs: https://clickhouse.com/docs
- Verified: `crates/veronex/migrations/`, `crates/veronex-analytics/`
