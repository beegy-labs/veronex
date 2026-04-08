# Auth -- RBAC, JWT & Sessions

> SSOT | **Last Updated**: 2026-03-28 (rev 6 -- N:N roles, permissions in JWT, RequirePermission extractors)

## Overview

Two independent auth layers:

| Layer | Mechanism | Protects |
|-------|-----------|----------|
| API Key | `X-API-Key` header (BLAKE2b hash) | Inference only: `/v1/chat/*`, `/v1/inference/*`, `/api/*`, `/v1beta/*`, `/v1/jobs/*/stream` |
| JWT Bearer | `Authorization: Bearer <token>` (HS256) | All admin routes: `/v1/accounts/*`, `/v1/audit`, `/v1/keys/*`, `/v1/usage/*`, `/v1/dashboard/*`, `/v1/providers/*`, `/v1/servers/*`, `/v1/gemini/*`, `/v1/ollama/*`, `/v1/test/*` |
| Public | None | `/v1/auth/*`, `/v1/setup/*`, `/health`, `/readyz`, `/docs/*`, `/v1/metrics/targets` |

## Roles & Permissions (N:N)

Accounts have N:N role assignment via `account_roles` join table. Each role grants a set of permissions and menu visibility.

| Table | Purpose |
|-------|---------|
| `roles` | `id, name, permissions TEXT[], menus TEXT[], is_system BOOL` |
| `account_roles` | `account_id, role_id` (composite PK) |

### Built-in Roles

| Role | `is_system` | Permissions |
|------|-------------|-------------|
| `super` | true | All: `dashboard_view`, `api_test`, `provider_manage`, `key_manage`, `account_manage`, `audit_view`, `settings_manage`, `role_manage`, `model_manage` |
| `viewer` | true | `dashboard_view` only |

System roles (`is_system=true`) cannot be edited or deleted.

### Permission Identifiers

| Permission | Purpose |
|------------|---------|
| `dashboard_view` | View dashboard and overview |
| `api_test` | Run test inferences |
| `provider_manage` | CRUD providers and servers |
| `key_manage` | CRUD API keys |
| `account_manage` | CRUD accounts |
| `audit_view` | View audit log |
| `settings_manage` | Modify system settings |
| `role_manage` | CRUD roles (except system roles) |
| `model_manage` | Manage models (enable/disable, sync) |

### JWT Claims

Permissions and menus are embedded in JWT claims for frontend gating:

```json
{
  "sub": "account-uuid",
  "role": "super",
  "jti": "session-uuid",
  "exp": 1234567890,
  "permissions": ["dashboard_view", "provider_manage", "role_manage"],
  "menus": ["dashboard", "providers", "accounts"],
  "role_name": "super"
}
```

## Router Layers (4-layer)

```
Public         /v1/auth/*, /v1/setup/*, /health, /readyz, /docs/*, /v1/metrics/targets   no middleware
API Key Auth   /v1/inference/*, /v1/chat/*, /api/*, /v1beta/*, /v1/jobs/*/stream          api_key_auth + rate_limiter
JWT Auth       /v1/accounts/*, /v1/audit, /v1/keys/*, /v1/usage/*, /v1/dashboard/*,       jwt_auth
               /v1/providers/*, /v1/servers/*, /v1/gemini/*, /v1/ollama/*
JWT Auth       /v1/test/*                                                                  jwt_auth (no rate limit)
```

## JWT Middleware (`jwt_auth`)

- Extracts `Authorization: Bearer <token>`
- Decodes via `jsonwebtoken::decode<Claims>(token, HS256, secret)`
- Checks Valkey `veronex:revoked:{jti}` -- 401 if revoked (O(1) blocklist)
- `tokio::spawn` calls `session_repo.update_last_used(&jti)` (non-blocking)
- Inserts `Claims { sub, role, jti, exp }` into request extensions

### Permission Extractors

| Extractor | Check |
|-----------|-------|
| `RequireSuper` | `role == Super` (403 otherwise) |
| `RequireRoleManage` | `role_manage` permission or super |
| `RequireAccountManage` | `account_manage` permission or super |
| `RequireProviderManage` | `provider_manage` permission or super |
| ... | One per permission via `define_require_permission!` macro |

All generated extractors bypass the permission check for super-admin accounts.
Usage: `RequireRoleManage(claims): RequireRoleManage` as handler arg.

## Accounts Table

```sql
CREATE TABLE accounts (
  id            UUID PRIMARY KEY DEFAULT uuidv7(),
  username      VARCHAR(64) NOT NULL UNIQUE,
  password_hash VARCHAR(255) NOT NULL,          -- Argon2id
  name          VARCHAR(128) NOT NULL,
  email         VARCHAR(255),
  department    VARCHAR(128),
  position      VARCHAR(128),
  is_active     BOOLEAN NOT NULL DEFAULT true,
  created_by    UUID REFERENCES accounts(id),
  last_login_at TIMESTAMPTZ,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  deleted_at    TIMESTAMPTZ                     -- soft-delete
);
-- N:N role assignment (replaces old accounts.role column)
CREATE TABLE account_roles (
  account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  role_id    UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
  PRIMARY KEY (account_id, role_id)
);
```

Migrations: `000007_roles`, `000008_account_roles`, `000009_role_manage_perm`

## First-Run Setup Flow

On fresh install (no accounts), the first super account is created via:

| Endpoint | Auth | Behavior |
|----------|------|----------|
| `GET /v1/setup/status` | None | `{ "needs_setup": true/false }` |
| `POST /v1/setup` | None | Creates super account + issues session; 409 if any account exists |

Frontend `AppShell` checks setup status on every load: `needs_setup: true` redirects to `/setup`.

**CI bootstrap** (optional, pre-seed without UI):
```bash
BOOTSTRAP_SUPER_USER=admin    # Both must be set; omit for setup-flow-only
BOOTSTRAP_SUPER_PASS=secret
```

## Password Hashing

- **Algorithm**: Argon2id (`argon2 = "0.5"` crate), PHC string format
- API keys use BLAKE2b-256 (unchanged)

## JWT Properties

| Property | Value |
|----------|-------|
| Algorithm | HS256 |
| `sub` | `account.id` (UUID) |
| `role` | `"super"` / `"admin"` (legacy, kept for backward compat) |
| `jti` | `Uuid::now_v7()` -- unique per session, used for revocation |
| `exp` | now + 1 hour |
| `permissions` | Merged permission strings from all assigned roles |
| `menus` | Merged menu IDs from all assigned roles |
| `role_name` | Primary role name |
| Secret | `JWT_SECRET` env var |

## Sessions (`account_sessions` table)

```sql
CREATE TABLE account_sessions (
  id                 UUID PRIMARY KEY DEFAULT uuidv7(),
  account_id         UUID NOT NULL REFERENCES accounts(id),
  jti                UUID NOT NULL UNIQUE,
  refresh_token_hash VARCHAR(64),       -- BLAKE2b-256
  ip_address         VARCHAR(45),
  created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
  last_used_at       TIMESTAMPTZ,
  expires_at         TIMESTAMPTZ NOT NULL,
  revoked_at         TIMESTAMPTZ
);
```

Migration: consolidated in `000001_init.up.sql`

**Revocation flow**: Login inserts session with `jti`. Logout sets `revoked_at` + Valkey `SET veronex:revoked:{jti} 1 EX {remaining_ttl}`. Refresh revokes old session, creates new one with new `jti` and new refresh hash. Valkey entries auto-expire.

> **Note**: RefreshResponse returns `{ ok: bool }`. New tokens are set as HttpOnly cookies.

## Password Reset

One-time token in Valkey: `veronex:pwreset:{token} -> account_id`, TTL 24h.
- Super creates: `POST /v1/accounts/{id}/reset-link` returns `{ token }`
- User resets: `POST /v1/auth/reset-password { token, new_password }` -- token deleted immediately

→ Endpoints and environment variables: `jwt-sessions-endpoints.md`
