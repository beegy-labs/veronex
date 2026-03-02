# Web — API Keys Page (/keys)

> SSOT | **Last Updated**: 2026-03-02 (rev2: KeyUsageModal — per-key hourly charts + model breakdown table)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to CreateKeyModal | `web/app/keys/page.tsx` modal form + `web/lib/api.ts` `createKey()` + backend `key_handlers.rs` `CreateKeyRequest` | Frontend form → API call → Rust struct → DB migration if new column |
| Change delete confirmation message | `web/app/keys/page.tsx` confirm dialog + `web/messages/en.json` `keys.deleteConfirm` | Update i18n key in all 3 locales |
| Add new column to keys table | `web/app/keys/page.tsx` table + `web/lib/types.ts` `KeySummary` | Add column header + cell + extend type |
| Change toggle optimistic behavior | `web/app/keys/page.tsx` Switch `onCheckedChange` + `useMutation` | Use `useOptimistic` for instant feedback |
| Add column to KeyUsageModal model table | `web/components/key-usage-modal.tsx` `<TableHead>` + `<TableCell>` + `web/lib/types.ts` `ModelBreakdown` | Extend `ModelBreakdown` type + backend `usage_handlers.rs` SQL |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/keys/page.tsx` | API keys management page |
| `web/components/key-usage-modal.tsx` | Per-key usage modal — KPI cards, model breakdown, hourly charts |
| `web/lib/api.ts` | `api.keys()`, `api.createKey()`, `api.deleteKey()`, `api.toggleKey()`, `api.keyModelBreakdown()` |
| `web/lib/types.ts` | `KeySummary`, `CreateKeyResponse`, `ModelBreakdown` |
| `web/lib/queries/usage.ts` | `keyUsageQuery`, `keyModelBreakdownQuery` |
| `web/messages/en.json` | i18n keys under `keys.*`, `usage.*` |

---

## KeyUsageModal (`web/components/key-usage-modal.tsx`)

Clicking any row in the keys table opens `KeyUsageModal` — a full-screen dialog showing per-key usage analytics.

### Layout

```
┌─ Dialog ── "{name}" usage ──────────────────────────────────────────────────┐
│  vnx_abc123de… [Free / Paid badge]                    [24h ▾ / 7d / 30d]   │
├─────────────────────────────────────────────────────────────────────────────┤
│  [Requests]  [Tokens]  [Success %]  [Errors]          ← KPI row (StatsCard) │
├─────────────────────────────────────────────────────────────────────────────┤
│  MODEL BREAKDOWN                                                             │
│  Model        Provider  Requests  Share  Tokens  Avg Latency                │
│  llama3.2:3b  ollama    142       63.4%  48,210  2.1s                       │
│  gemini-2.0   gemini     82       36.6%  22,050  1.4s                       │
├─────────────────────────────────────────────────────────────────────────────┤
│  TOKENS PER HOUR  [AreaChart — Prompt / Completion]                         │
│  REQUESTS PER HOUR [BarChart — requests / success / errors]                 │
└─────────────────────────────────────────────────────────────────────────────┘
```

- **Time range**: `TimeRangeSelector` — 24h (default) / 7d / 30d
- **KPI row**: aggregated from hourly data — `totalRequests`, `totalTokens`, `totalSuccess`, `totalErrors`, `successRate`
- **Model Breakdown table**: shown only when `models.length > 0`
- **Empty state**: dashed border box with `usage.noKeyData` when `chartData.length === 0`

### Model Breakdown Table Columns

| Column | Field | Notes |
|--------|-------|-------|
| Model | `model_name` | monospace font |
| Provider | `backend` | `<Badge>` capitalize |
| Requests | `request_count` | `fmtCompact()` |
| Share | `call_pct` | `toFixed(1)%` — muted |
| Tokens | `prompt_tokens + completion_tokens` | `fmtCompact()` |
| Avg Latency | `avg_latency_ms` | `(ms/1000).toFixed(1)s`; `'—'` when 0 |

### Data Queries

```typescript
// Hourly aggregates (used for KPI + charts)
const { data: hourly } = useQuery(keyUsageQuery(apiKey.id, hours))
// → GET /v1/usage/{key_id}?hours={hours}

// Model breakdown
const { data: models } = useQuery(keyModelBreakdownQuery(apiKey.id, hours))
// → GET /v1/usage/{key_id}/models?hours={hours}
```

`ModelBreakdown` type (`web/lib/types.ts`):
```typescript
export interface ModelBreakdown {
  model_name:         string
  backend:            string
  request_count:      number
  call_pct:           number    // share of this key's total requests
  prompt_tokens:      number
  completion_tokens:  number
  avg_latency_ms:     number
}
```

### State

```typescript
// web/app/keys/page.tsx
const [selectedKey, setSelectedKey] = useState<ApiKey | null>(null)

// Row click → setSelectedKey(key) → <KeyUsageModal> renders
// Dialog onOpenChange(false) / onClose → setSelectedKey(null)
```

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
keys:               () => req<KeySummary[]>('/v1/keys'),
createKey:          (body) => req<CreateKeyResponse>('/v1/keys', { method: 'POST', body: JSON.stringify(body) }),
deleteKey:          (id) => req<void>(`/v1/keys/${id}`, { method: 'DELETE' }),
toggleKey:          (id, is_active) => req<void>(`/v1/keys/${id}`, {
                      method: 'PATCH', body: JSON.stringify({ is_active }) }),
keyModelBreakdown:  (keyId, hours = 24) =>
                      req<ModelBreakdown[]>(`/v1/usage/${keyId}/models?hours=${hours}`),
```

Backend handler: `usage_handlers::key_model_breakdown`
→ `GET /v1/usage/{key_id}/models?hours={hours}`
→ Queries `inference_jobs GROUP BY model_name, backend` with LATERAL pricing join
→ Computes `call_pct` as share of total requests for that key in the window

---

## i18n Keys (messages/en.json)

### keys.*
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
"rpmTpm", "prefix", "tenant", "name", "status", "activeToggle", "createdAt",
// KeyUsageModal
"usageTitle",           // "{name} Usage" — dialog title
"modelBreakdown"        // "Model Breakdown" — section heading
```

### usage.* (KeyUsageModal columns + labels)
```json
"totalRequests",        // KPI card
"totalTokens",          // KPI card
"success",              // KPI card
"errors",               // KPI card
"requests",             // chart bar legend / table column
"tokensPerHour",        // AreaChart section heading
"requestsPerHour",      // BarChart section heading
"noKeyData",            // empty state message
"backend",              // model breakdown table column
"share",                // model breakdown table column (call_pct)
"avgLatency"            // model breakdown table column
```
