# Code Patterns: Rust — Security Primitives

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns.md`](../patterns.md)

## RequirePermission Macro

`define_require_permission!` generates Axum `FromRequestParts` extractors that check JWT claims for a specific permission. Super-admin bypasses all checks.

```rust
// Definition (jwt_auth.rs)
macro_rules! define_require_permission {
    ($name:ident, $perm:expr) => { /* reads Claims, checks role==Super || permissions.contains($perm) */ };
}
define_require_permission!(RequireRoleManage, "role_manage");

// Usage in handlers
pub async fn list_roles(RequireRoleManage(_claims): RequireRoleManage, ...) { ... }
```

| Extractor | Permission | Used by |
|-----------|-----------|---------|
| `RequireRoleManage` | `role_manage` | Role CRUD |
| `RequireAccountManage` | `account_manage` | Account CRUD |
| `RequireProviderManage` | `provider_manage` | Provider CRUD |
| `RequireKeyManage` | `key_manage` | API key CRUD |
| `RequireAuditView` | `audit_view` | Audit log |
| `RequireSettingsManage` | `settings_manage` | System settings |
| `RequireApiTest` | `api_test` | Test inference |
| `RequireDashboardView` | `dashboard_view` | Dashboard data |

## Provider URL Validation (SSRF)

`validate_provider_url()` in `provider_handlers.rs` blocks:
- Non-HTTP schemes (file://, ftp://, gopher://)
- Cloud metadata endpoints (GCP `metadata.google.internal`)
- IPv4 link-local (`169.254.0.0/16` — AWS metadata)
- IPv6 link-local (`fe80::/10`)
- IPv4-mapped IPv6 (`::ffff:169.254.169.254`)
- IPv6 bracket notation parsed correctly (`[::ffff:...]:port`)

Called on provider register and update. See `auth/security.md` for full SSRF details.

