# Web — API Docs Page

> SSOT | **Last Updated**: 2026-04-06
> API Test Panel: `frontend/pages/api-test.md`

## /api-docs Page

Landing page links to two embedded viewers:

| Route | Component | Notes |
|-------|-----------|-------|
| `/api-docs/swagger` | `SwaggerUiWrapper` (swagger-ui-react) | dynamic, ssr:false |
| `/api-docs/redoc` | `RedocWrapper` (redoc) | dynamic, ssr:false |

Both auto-select locale-aware spec: `${API_URL}/docs/openapi.json?lang={locale}`

## Locale-Aware OpenAPI Spec

| Path | Lang |
|------|------|
| `GET /docs/openapi.json` | English (default) |
| `GET /docs/openapi.json?lang=ko` | Korean overlay |
| `GET /docs/openapi.json?lang=ja` | Japanese overlay |

Overlays in `crates/veronex/src/infrastructure/inbound/http/openapi.overlay.{ko,ja}.json`.
Merge: recursive deep merge (objects merge key-by-key, arrays/scalars replaced).
Handler: `docs_handlers.rs`. No auth required.

## Key Files

| File | Purpose |
|------|---------|
| `web/components/swagger-ui-wrapper.tsx` | Swagger UI wrapper (CSS + theme) |
| `web/components/redoc-wrapper.tsx` | RedocStandalone wrapper (theme + labels) |
| `web/app/api-docs/page.tsx` | Landing page |
| `web/app/api-docs/swagger/page.tsx` | Swagger embedded |
| `web/app/api-docs/redoc/page.tsx` | ReDoc embedded |

## i18n Keys

`apiDocs.*`: title, swagger, swaggerDesc, redoc, redocDesc, openapi, openapiDesc, viewDocs, redocEnum, redocDefault, redocExample, redocDownload, redocNoResults, redocResponses, redocRequestSamples, redocResponseSamples
