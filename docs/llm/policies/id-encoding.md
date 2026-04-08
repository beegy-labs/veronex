# ID Encoding Policy

> SSOT | **Last Updated**: 2026-04-01

## Summary

All entity IDs are stored internally as **UUIDv7** and exposed externally as
**`{prefix}_{base62}`** strings. The encoding is reversible but opaque to clients —
they never see or construct raw UUIDs. API key secrets use a separate one-way hash.

---

## Two Distinct ID Concepts

| Concept | Format | Reversible | Purpose |
|---------|--------|-----------|---------|
| **Entity ID** | `{prefix}_{base62(uuid)}` | Yes (server-side) | Database row identifier, URL path params, JSON fields |
| **API Key Secret** | `vnx_{base62(random128)}` | No (BLAKE2b-256 hash stored) | Bearer token for API authentication |

These must never be confused. `ApiKeyId` (`key_xxx`) identifies the *row*;
`key` (`vnx_xxx`) is the *secret*.

---

## Entity ID Types

Defined in `crates/veronex/src/domain/value_objects.rs` via the `define_entity_id!` macro.

| Rust Type | Prefix | Example |
|-----------|--------|---------|
| `JobId` | `job` | `job_3X4aBcDefGh...` |
| `ConvId` | `conv` | `conv_3X4aBcDefGh...` |
| `AccountId` | `acct` | `acct_3X4aBcDefGh...` |
| `ApiKeyId` | `key` | `key_3X4aBcDefGh...` |
| `RoleId` | `role` | `role_3X4aBcDefGh...` |
| `SessionId` | `sess` | `sess_3X4aBcDefGh...` |
| `ProviderId` | `prov` | `prov_3X4aBcDefGh...` |
| `GpuServerId` | `gpu` | `gpu_3X4aBcDefGh...` |
| `McpId` | `mcp` | `mcp_3X4aBcDefGh...` |

---

## Encoding Mechanics

```
DB (UUIDv7)  ──encode──►  API (prefix_base62)
               decode
```

```rust
// Encode: UUID → "job_3X4aB..."
let public = JobId::from_uuid(uuid).to_string();   // or JobId(uuid).to_string()

// Decode: "job_3X4aB..." → UUID (happens automatically in Path extractors)
let jid: JobId = "job_3X4aB...".parse().unwrap();
let uuid: Uuid = jid.0;

// Generic helpers (when entity type is unknown)
pub_id_encode("job", uuid)          // → "job_3X4aB..."
pub_id_decode("job", "job_3X4aB…") // → Some(uuid)
```

The macro generates for each type:
- `Serialize` — outputs `"{prefix}_{base62}"` string
- `Deserialize` — parses `"{prefix}_{base62}"` string
- `Display` — same as Serialize
- `FromStr` — validates prefix, decodes base62
- `From<Uuid>` / `Into<Uuid>` — internal conversion
- `new()` — creates with `Uuid::now_v7()`
- `from_uuid(uuid)` — wraps existing UUID
- `as_uuid()` — returns inner UUID

---

## Handler Pattern

```rust
// Path parameter — automatically decoded by Axum via Deserialize
pub async fn delete_thing(
    Path(jid): Path<JobId>,          // client sends "job_3X4aB..."
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    state.repo.delete(&jid.0).await?;   // .0 = inner Uuid for DB calls
    Ok(StatusCode::NO_CONTENT)
}

// Response — automatically encoded by serde_json via Serialize
#[derive(Serialize)]
pub struct ThingSummary {
    pub id: JobId,         // serializes as "job_3X4aB..."
    pub name: String,
}
// Construction:
ThingSummary { id: JobId::from_uuid(row.id), name: row.name }

// Request body with ID field
#[derive(Deserialize)]
pub struct AssignRequest {
    pub role_ids: Vec<RoleId>,     // client sends ["role_abc...", "role_def..."]
}
// Extract inner UUIDs for DB:
let uuids: Vec<Uuid> = req.role_ids.iter().map(|r| r.0).collect();
```

---

## API Key Secret (Different Policy)

Defined in `domain/services/api_key_generator.rs`.

```rust
pub fn generate_api_key() -> (Uuid, String, String, String) {
    // id       = Uuid::now_v7()             — entity ID (stored as UUID, exposed as ApiKeyId)
    // plaintext = "vnx_{base62(random128)}"  — shown once at creation
    // key_hash  = hex(BLAKE2b-256(plaintext)) — stored in DB, NEVER reversed
    // key_prefix = plaintext[..12]           — display only ("vnx_01ARZ3NDE…")
}
```

| Property | Value |
|----------|-------|
| Algorithm | BLAKE2b-256 |
| Output | 64 hex chars |
| Reversible | No |
| Stored | `key_hash` column (never serialized to API) |
| Plaintext exposure | Once — `CreateKeyResponse.key` only |

Auth lookup: `hash_api_key(bearer_token)` → compare against `key_hash` in DB/cache.

---

## Rules

| Rule | Detail |
|------|--------|
| Never expose raw `Uuid` in API responses | Use the appropriate typed ID (`JobId`, `AccountId`, etc.) |
| Never store base62/prefixed string in DB | DB always stores `UUID` (UUIDv7); encoding is HTTP-layer only |
| Never use `Path<Uuid>` in handlers | Use `Path<JobId>`, `Path<AccountId>`, etc. — prefix validates entity type |
| Use `.0` to extract UUID for DB/repo calls | `jid.0`, `aid.0`, etc. |
| API key secret ≠ entity ID | `vnx_...` secret → BLAKE2b hash; `key_...` entity ID → base62 UUID |
| Path decode failure → 400 Bad Request | Axum returns 400 when `FromStr` fails for path params |
