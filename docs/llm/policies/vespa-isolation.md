# Vespa Isolation Strategy

> SSOT | **Last Updated**: 2026-04-10 | Classification: Operational

## Isolation Hierarchy

```
Vespa Instance (shared)
└── Schema  (physical boundary — equivalent to ES index)
    ├── schema: mcp_tools
    └── schema: search_docs  (future)
        └── environment  (field filter — logical)
            └── tenant_id  (field filter — logical)
```

## Key Definitions

| Key | Env Var | Default | Purpose |
|-----|---------|---------|---------|
| Schema | — | `mcp_tools` | Physical service boundary. Separate storage per schema. |
| `environment` | `VESPA_ENVIRONMENT` | `local-dev` (docker) / `prod` (helm) | Environment partition (prod/dev/local-dev). |
| `tenant_id` | `VESPA_TENANT_ID` | `default` | Team/org sub-partition. Reserved for multi-tenant use. |

## Isolation Mechanisms

| Mechanism | Type | Rule |
|-----------|------|------|
| Schema | Physical | One schema per document type (mcp_tools, search_docs). Not for environment or service isolation. |
| `environment` field | Logical | Stored as `attribute`. All queries must filter: `WHERE environment contains "prod"`. |
| `tenant_id` field | Logical | Stored as `attribute`. All queries must filter: `AND tenant_id contains "default"`. |

> **YQL string-attribute filter**: Use `contains`, NOT `=`. YQL's `=` is a numeric range operator — feeding a hyphenated string value like `local-dev` makes the parser read `-` as a sign character and fails with `Illegal embedded sign character`. `contains` resolves to attribute equality at the filter rank for `attribute`-indexed string fields. See `crates/veronex-mcp/src/vector/vespa_client.rs::search` and the regression test `vespa_search_uses_contains_for_string_attributes`.

## When to Use Separate Vespa Instances

| Scenario | Recommendation |
|----------|----------------|
| Same team, different environments | Single Vespa + `environment` field |
| Same team, different document types | Single Vespa + separate schemas |
| Different teams, independent ops | Separate Vespa instances |
| Different orgs, hard isolation required | Separate Vespa instances |

## Why Not Schema-per-Environment

| Option | Problem |
|--------|---------|
| `prod_mcp_tools` / `dev_mcp_tools` schemas | Duplicate schema maintenance — technical debt |
| Schema-per-environment | Violates Vespa design intent (schemas = document types, not environments) |
| Field filter | Vespa-idiomatic — validated by Vinted, Yahoo at scale |

## ES vs Vespa Comparison

| | Elasticsearch | Vespa |
|--|--|--|
| Index isolation | Physical — URL determines search scope | No equivalent — schema is closest |
| Multi-tenancy | Index-per-tenant or field filter | Field filter only |
| Environment isolation | Separate indices | `environment` field filter |
| Search boundary | Automatic (URL path) | Explicit (WHERE clause required) |

ES: `GET /prod-index/_search` auto-scopes to prod.
Vespa: `POST /search/` scans all — WHERE clause is mandatory.

## Document ID Format

```
tool_id = "{environment}:{tenant_id}:{server_id}:{tool_name}"
example = "prod:default:550e8400-...:get_weather"
```

## Environment Variable Reference

| Env Var | docker-compose default | Helm default |
|---------|----------------------|--------------|
| `VESPA_ENVIRONMENT` | `local-dev` | `prod` |
| `VESPA_TENANT_ID` | `default` | `default` |
