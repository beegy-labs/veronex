# SDD: MCP Server Edit (name, slug, url)

> Status: complete | Created: 2026-04-13 | Shipped: 2026-04-13 (#54 `9b6758a`)

## Problem

`PATCH /v1/mcp/servers/:id` currently allows updating `is_enabled`, `url`, and `name`
but not `slug`. Once registered, a slug cannot be changed without deleting and re-registering
the server — losing all analytics history.

## Solution

Extend PATCH to accept an optional `slug` field. Validate format and uniqueness.
Reconnect the MCP session with the new slug so tool namespaced names update correctly.

## API Change

```
PATCH /v1/mcp/servers/:id
{
  "name": "My Server",       // optional, max 128 chars
  "slug": "my_server",       // optional, [a-z][a-z0-9_]*, max 64 chars, must be unique
  "url": "http://...",       // optional
  "is_enabled": true         // optional
}
```

### Error cases
| Condition | Status |
|-----------|--------|
| slug format invalid | 400 |
| slug > 64 chars | 400 |
| slug already used by another server | 409 |

## Session reconnect

If slug changes and server is enabled → disconnect old session, reconnect with new slug.
Tool namespaced names are `{slug}__{tool_name}` — changing slug renames all tool references.

## Frontend

- Edit modal (name, slug, url) — reuse RegisterMcpModal layout
- Pencil icon button in server table actions column

## Tests

- Unit: slug validation logic (format, length, uniqueness)
- E2E (12-mcp.sh): PATCH slug → verify updated slug in response + list
