# Web — API Keys Page (/keys)

> SSOT | **Last Updated**: 2026-02-27

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to CreateKeyModal | `web/app/keys/page.tsx` modal form + `web/lib/api.ts` `createKey()` + backend `key_handlers.rs` `CreateKeyRequest` | Frontend form → API call → Rust struct → DB migration if new column |
| Change delete confirmation message | `web/app/keys/page.tsx` confirm dialog + `web/messages/en.json` `keys.deleteConfirm` | Update i18n key in all 3 locales |
| Add new column to keys table | `web/app/keys/page.tsx` table + `web/lib/types.ts` `KeySummary` | Add column header + cell + extend type |
| Change toggle optimistic behavior | `web/app/keys/page.tsx` Switch `onCheckedChange` + `useMutation` | Use `useOptimistic` for instant feedback |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/keys/page.tsx` | API keys management page |
| `web/lib/api.ts` | `api.keys()`, `api.createKey()`, `api.deleteKey()`, `api.toggleKey()` |
| `web/lib/types.ts` | `KeySummary`, `CreateKeyResponse` |
| `web/messages/en.json` | i18n keys under `keys.*` |

---

## Page Layout

```
[+ Create Key]

┌──────────────────────────────────────────────────────────────────────────────┐
│ Name        Prefix         Tenant    Active  RPM/TPM   Created   Actions     │
│ prod-key    vnx_abc123de…  default   ●       10/1000   Feb 26    [●][🗑]     │
│ dev-key     vnx_xyz987fe…  default   ○       0/0       Feb 25    [●][🗑]     │
└──────────────────────────────────────────────────────────────────────────────┘
```

- **Create button** → opens `CreateKeyModal`
- **Toggle active** (`●/○`) → Switch → `PATCH /v1/keys/{id}` `{ is_active: bool }`
- **Delete** (`🗑`) → confirm dialog → `DELETE /v1/keys/{id}` (soft-delete)
  - Confirm message: "Historical usage data is preserved."
- Soft-deleted keys disappear from list immediately (optimistic update)

---

## CreateKeyModal

Fields:
- **Name** (required)
- **Tenant ID** (default: "default")
- **Rate Limit RPM** (0 = unlimited)
- **Rate Limit TPM** (0 = unlimited)
- **Expires At** (optional date picker)

On success: shows `CreateKeyResponse.key` (plaintext) with warning "Save this key now — it will never be shown again."
Query invalidation: `['keys']`

---

## API Calls (api.ts)

```typescript
keys:        () => req<KeySummary[]>('/v1/keys'),
createKey:   (body) => req<CreateKeyResponse>('/v1/keys', { method: 'POST', body: JSON.stringify(body) }),
deleteKey:   (id) => req<void>(`/v1/keys/${id}`, { method: 'DELETE' }),
toggleKey:   (id, is_active) => req<void>(`/v1/keys/${id}`, {
               method: 'PATCH', body: JSON.stringify({ is_active }) }),
```

---

## i18n Keys (messages/en.json → `keys.*`)

```json
"createKey", "createTitle", "keyName", "keyNamePlaceholder",
"tenantId", "rateLimitRpm", "rateLimitTpm", "rateLimitPlaceholder",
"creating", "createdTitle", "createdWarning",
"deleteTitle", "deleteConfirm", "deleting",
"loadingKeys", "failedKeys", "noKeys", "actions", "deleteKey",
"rpmTpm", "prefix", "tenant", "expiresAt"
```
