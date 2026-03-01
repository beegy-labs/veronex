# Web — API Keys Page (/keys)

> SSOT | **Last Updated**: 2026-03-02 (rev: name is non-unique label; UUIDv7 id is unique identifier; nameTaken removed)

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
Title: "API Keys"  Subtitle: "N keys"                              [+ Create Key]

┌────────────────────────────────────────────────────────────────────────────────┐
│ Name        Prefix         Tenant    Status   Toggle  RPM/TPM  Tier   Created … │
│ prod-key    vnx_abc123de…  default   ● Active ◉       10/1000  Paid   Feb 26  🗑 │
│ dev-key     vnx_xyz987fe…  default   ● Active ◉       ∞/∞      Free   Mar  1  🗑 │
└────────────────────────────────────────────────────────────────────────────────┘
```

- Single flat `DataTable` — no Standard/Test sections
- **[+ Create Key]** → `setShowCreate(true)` → `CreateKeyModal`
- Test keys (`key_type = 'test'`) are excluded server-side by `GET /v1/keys`; never shown here
- **Tier badge**: `'paid'` = info-colored filled badge; `'free'` = muted outlined badge
- **Toggle active** → Switch → `PATCH /v1/keys/{id}` `{ is_active: bool }`
- **Delete** (`🗑`) → `DeleteConfirmModal` → `DELETE /v1/keys/{id}` (soft-delete)
- Inactive rows: `opacity-50`
- Empty table → `DataTableEmpty` placeholder text

---

## CreateKeyModal

Fields:
- **Name** (required) — display label only; **duplicates are allowed** (unique identifier is UUIDv7 `id`)
- **Tenant ID** (default: "default")
- **Rate Limit RPM** (0 = unlimited)
- **Rate Limit TPM** (0 = unlimited)
- **Tier** — Select: `Paid` (default) | `Free`

On success → `KeyCreatedModal`: shows `CreateKeyResponse.key` plaintext with warning banner.
Query invalidation on success: `['keys']`.

## State

```ts
const [showCreate, setShowCreate] = useState(false)
```

`false` = no modal · `true` = create key modal open

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
"title",
"keysCount",            // "{count} keys" — page subtitle
"createKey", "createTitle",
"keyName", "keyNamePlaceholder",
"tenantId", "rateLimitRpm", "rateLimitTpm", "rateLimitPlaceholder",
"tier", "tierFree", "tierPaid",
"creating", "createdTitle", "createdWarning",
"deleteTitle", "deleteConfirm", "deleting",
"loadingKeys", "failedKeys", "actions", "deleteKey",
"rpmTpm", "prefix", "tenant", "name", "status", "activeToggle", "createdAt"
```
