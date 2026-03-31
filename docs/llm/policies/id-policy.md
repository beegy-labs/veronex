# ID & API Key Policy

> SSOT | **Last Updated**: 2026-03-28

## General Resource IDs

DB stores UUID only. API exposes `{prefix}_{base62(uuid)}`.

### Conversion

```rust
// UUID → public ID (API response)
fn to_public_id(prefix: &str, uuid: Uuid) -> String {
    format!("{}_{}", prefix, base62::encode(uuid.as_u128()))
}

// Public ID → UUID (API request)
fn from_public_id(prefix: &str, id: &str) -> Option<Uuid> {
    id.strip_prefix(&format!("{}_", prefix))
        .and_then(|s| base62::decode(s).ok())
        .map(|n| Uuid::from_u128(n as u128))
}
```

### Prefix Registry

| Resource | Prefix | Example |
|----------|--------|---------|
| Provider | `prv_` | `prv_032pei0u2Kk2g` |
| Server | `srv_` | `srv_032pei0u2Kk2g` |
| Account | `acc_` | `acc_032pei0u2Kk2g` |
| Job | `job_` | `job_032pei0u2Kk2g` |
| MCP Server | `mcp_` | `mcp_032pei0u2Kk2g` |
| Role | `rol_` | `rol_032pei0u2Kk2g` |
| Conversation | `conv_` | `conv_032pei0u2Kk2g` |
| Session | `ses_` | `ses_032pei0u2Kk2g` |

### Properties

- DB: UUID column only (1 index)
- Deterministic: same UUID → same base62 (no stored mapping)
- Bidirectional: UUID ↔ base62
- Cost: ~50ns per conversion (no DB lookup)
- No `public_id` column needed

---

## API Key

Secrets — one-way hash, prefix for environment detection.

### Flow

```
[Create]
  raw_key = random 32 bytes → base62
  full_key = "vnx_live_" + raw_key
  prefix = full_key[0..16]
  key_hash = SHA256(full_key)
  DB INSERT: { prefix, key_hash, scopes, ... }
  Response: { "key": "vnx_live_Ak3xR9..." }  ← shown once

[Auth]
  Client: Authorization: Bearer vnx_live_Ak3xR9...
  Server: SHA256(bearer_value) → DB WHERE key_hash = $1

[List]
  Response: { "prefix": "vnx_live_Ak3x", ... }
  ※ original key cannot be shown again (hash only)
```

### Key Prefixes

| Environment | Prefix | Use |
|-------------|--------|-----|
| Production | `vnx_live_` | Real API access |
| Test | `vnx_test_` | Test/sandbox access |

### Properties

- DB: prefix + hash (2 columns)
- One-way: hash → original key not recoverable
- Full key shown once at creation
- Prefix enables environment detection + leak scanning

---

## Comparison

| | Resource ID | API Key |
|---|---|---|
| DB storage | UUID only | prefix + hash |
| Direction | Bidirectional (reversible) | One-way (irreversible) |
| Conversion | API server real-time | Once at creation |
| Prefix | `prv_` `acc_` `job_` ... | `vnx_live_` `vnx_test_` |
| Purpose | Resource identification | Authentication (secret) |
| Exposure | Every response | Once at creation |
| Leak risk | Low (JWT required) | High (auth capable) |
| Index | 1 (UUID) | 1 (hash) |
| 1M+ TPS cost | ~50ns/req | 0 (auth only) |

---

## Files

| File | Purpose |
|------|---------|
| `domain/constants.rs` | Prefix constants |
| `domain/public_id.rs` (new) | `to_public_id()`, `from_public_id()` |
| All handler files | Convert UUID ↔ public ID at API boundary |
