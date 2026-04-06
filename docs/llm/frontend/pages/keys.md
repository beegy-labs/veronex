# Web — API Keys Page (/keys)

> SSOT | **Last Updated**: 2026-04-06

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to CreateKeyModal | `page.tsx` modal + `api.ts` `createKey()` + `key_handlers.rs` | Form field + API call + Rust struct + DB migration |
| Change delete confirmation | `page.tsx` confirm dialog + `en.json` `keys.deleteConfirm` | Update i18n key in all 3 locales |
| Add column to keys table | `page.tsx` table + `types.ts` `KeySummary` | Add header + cell + extend type |
| Change toggle optimistic behavior | `page.tsx` Switch `onCheckedChange` | Use `useOptimistic` for instant feedback |
| Add column to usage modal table | `key-usage-modal.tsx` + `types.ts` `ModelBreakdown` | Extend type + `usage_handlers.rs` SQL |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/keys/page.tsx` | API keys management page |
| `web/components/key-usage-modal.tsx` | Per-key usage modal (KPIs, model breakdown, hourly charts) |
| `web/lib/api.ts` | `keys()`, `createKey()`, `deleteKey()`, `toggleKey()`, `keyModelBreakdown()`, `keyMcpAccess()`, `updateKeyMcpAccess()` |
| `web/lib/types.ts` | `KeySummary`, `CreateKeyResponse`, `ModelBreakdown`, `McpAccessEntry` |
| `web/lib/queries/usage.ts` | `keyUsageQuery`, `keyModelBreakdownQuery` |
| `web/lib/queries/mcp.ts` | `keyMcpAccessQuery` |

## Page Layout

```
Title: "API Keys"  "N keys"                                [+ Create Key]
| Name | Prefix | Tenant | Status | Toggle | RPM/TPM | Tier | Created | MCP | Del |
```

- Single flat `DataTable`; test keys (`key_type='test'`) excluded server-side
- Tier badge: `paid` = info filled, `free` = muted outlined
- Toggle active: Switch -> `PATCH /v1/keys/{id} { is_active }`. Inactive rows: `opacity-50`
- Delete: `DeleteConfirmModal` -> `DELETE /v1/keys/{id}` (soft-delete)
- Row click -> `setSelectedKey(key)` -> opens `KeyUsageModal`
- MCP button (Server icon) -> opens `KeyMcpAccessModal`

## KeyMcpAccessModal

Server icon button in action column opens MCP access configuration dialog.

```
Dialog: "MCP Access — {key name}"
MCP Cap Points: [number input 0-10, default 3]
Per-server access table:
| Server | Slug | Status | Top-K | Grant/Revoke |
```

- `mcp_cap_points`: max agentic loop rounds for this key. `0` = MCP disabled. Saved via `PATCH /v1/keys/{id}`.
- Grant/Revoke: `POST /v1/keys/{id}/mcp/{server_id}` / `DELETE /v1/keys/{id}/mcp/{server_id}`
- `top_k`: per-server Vespa ANN override (optional). Saved on grant/update.
- Data: `keyMcpAccessQuery(keyId)` → `GET /v1/keys/{id}/mcp`

## CreateKeyModal

| Field | Notes |
|-------|-------|
| Name (required) | Display label; duplicates allowed (UUID is unique ID) |
| Tenant ID | Default: `"default"` |
| Rate Limit RPM | 0 = unlimited |
| Rate Limit TPM | 0 = unlimited |
| Tier | Select: `Paid` (default) / `Free` |

On success: shows `KeyCreatedModal` with plaintext key + warning banner. Invalidates `['keys']`.

State: `const [showCreate, setShowCreate] = useState(false)`

## KeyUsageModal

Clicking any row opens a full-screen dialog with per-key usage analytics.

```
Dialog: "{name}" usage | vnx_abc123... [Tier badge] [24h/7d/30d]
KPI row: [Requests] [Tokens] [Success %] [Errors]
Model Breakdown table (when models.length > 0)
Tokens Per Hour (AreaChart) | Requests Per Hour (BarChart)
Empty state: dashed box with usage.noKeyData when chartData empty
```

### Model Breakdown Columns

| Column | Field | Format |
|--------|-------|--------|
| Model | `model_name` | monospace |
| Provider | `provider_type` | `<Badge>` capitalize |
| Requests | `request_count` | `fmtCompact()` |
| Share | `call_pct` | `toFixed(1)%` muted |
| Tokens | `prompt_tokens + completion_tokens` | `fmtCompact()` |
| Avg Latency | `avg_latency_ms` | `(ms/1000).toFixed(1)s`; `'--'` when 0 |

### Data Queries

| Query | Endpoint |
|-------|----------|
| `keyUsageQuery(id, hours)` | `GET /v1/usage/{key_id}?hours={hours}` |
| `keyModelBreakdownQuery(id, hours)` | `GET /v1/usage/{key_id}/models?hours={hours}` |

`ModelBreakdown` type: `model_name`, `provider_type`, `request_count`, `call_pct`, `prompt_tokens`, `completion_tokens`, `avg_latency_ms`

## API Calls

| Function | Method | Path |
|----------|--------|------|
| `keys()` | GET | `/v1/keys` |
| `createKey(body)` | POST | `/v1/keys` |
| `deleteKey(id)` | DELETE | `/v1/keys/{id}` |
| `toggleKey(id, is_active)` | PATCH | `/v1/keys/{id}` |
| `keyModelBreakdown(keyId, hours)` | GET | `/v1/usage/{keyId}/models?hours={hours}` |
| `keyMcpAccess(keyId)` | GET | `/v1/keys/{keyId}/mcp` |
| `updateKeyMcpAccess(keyId, serverId, body)` | POST/DELETE | `/v1/keys/{keyId}/mcp/{serverId}` |

Server handler: `usage_handlers::key_model_breakdown` -- queries `inference_jobs GROUP BY model_name, provider_type` with LATERAL pricing join; computes `call_pct` as share of total reqs.

## i18n Keys

`keys.*`: title, description, name, prefix, tenant, status, activeToggle, rpmTpm, createdAt, expiresAt, noKeys, createKey, createTitle, keyName, keyNamePlaceholder, tenantId, tenantIdPlaceholder, rateLimitRpm, rateLimitTpm, rateLimitPlaceholder, tier, tierFree, tierPaid, creating, createdTitle, createdWarning, deleteTitle, deleteConfirm, deleting, actions, deleteKey, loadingKeys, failedKeys, registered, keysCount, viewUsage, usageTitle, modelBreakdown, createdBy, regenerateKey, regenerateTitle, regenerateConfirm, regenerating, keySavedAck, viewHistory, historyTitle, mcpAccess, mcpAccessTitle, mcpAccessDesc, mcpGranted, mcpNotGranted, mcpGrant, mcpRevoke, mcpLoadError, mcpNoServers, mcpCapPoints

`usage.*` (modal): totalRequests, totalTokens, success, errors, requests, tokensPerHour, requestsPerHour, noKeyData, provider, share, avgLatency
