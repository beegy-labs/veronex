# Terminology Policy

> SSOT | **Last Updated**: 2026-02-28

This is the single source of truth for all user-facing and internal terminology across the Veronex codebase.
Every doc, component, and i18n key MUST follow these definitions.

---

## Core Terms

| Term | Definition | Where Used |
|------|------------|------------|
| **Provider** | An inference service provider type (Ollama, Gemini). A provider configuration record stores the URL, API key, and routing metadata for one service account. | Nav sidebar, `/providers` page, usage breakdown UI, i18n keys under `backends.*` |
| **Server** | A GPU hardware node registered with a `node_exporter_url` for live metrics scraping. Providers may be linked to a server via `server_id`. | `/servers` page, nav sidebar |
| **API Key** | A bearer token that authenticates inference requests, enforces RPM/TPM rate limits, and tracks per-key usage. | `/keys` page, `X-API-Key` header |
| **Job** | A single inference request lifecycle record: pending → running → completed/failed/cancelled. | `/jobs` page, `inference_jobs` table |
| **Backend** | Internal / API-layer term for a provider configuration record. Maps to the `llm_backends` DB table and `/v1/backends` API routes. **Not used in user-visible UI labels.** | Rust structs, DB schema, API routes, internal variable names |
| **Tenant** | A logical grouping owner of API keys. Not exposed in the UI today; reserved for multi-tenant mode. | `api_keys.tenant_id` DB column |

---

## UI Label Rules

| Context | Correct Term | Incorrect ❌ |
|---------|-------------|-------------|
| Nav sidebar entry for Ollama/Gemini | Providers | Backends |
| Section header in usage breakdown | By Provider | By Backend |
| Column header referencing Ollama/Gemini type | Provider | Backend |
| GPU hardware management page | Servers | Backends, Nodes |
| Card description for usage split | Split by provider, API key, and model | Split by backend, … |

---

## i18n Keys (SSOT mapping)

| Concept | en.json key | Value |
|---------|------------|-------|
| Provider (nav) | `nav.providers` | `"Providers"` |
| Server (nav) | `nav.servers` | `"Servers"` |
| By Provider (section label) | `usage.byProvider` | `"By Provider"` |
| Provider (column header) | `usage.providerCol` | `"Provider"` |
| Usage Breakdown card title | `usage.breakdownTitle` | `"Usage Breakdown"` |
| Usage Breakdown card desc | `usage.breakdownDesc` | `"Split by provider, API key, and model"` |

---

## References

- Nav structure: [`docs/llm/frontend/web.md`](../frontend/web.md) — Nav Sidebar section
- Providers page spec: [`docs/llm/frontend/web-providers.md`](../frontend/web-providers.md)
- Servers page spec: [`docs/llm/frontend/web-servers.md`](../frontend/web-servers.md)
- Architecture overview: [`docs/llm/policies/architecture.md`](./architecture.md)
