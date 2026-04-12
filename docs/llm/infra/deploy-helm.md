# Deploy: Helm

> SSOT | **Last Updated**: 2026-04-11 | Classification: Operational
> Helm deployment configuration for Veronex services.

## Helm Deployment

Chart location: `deploy/helm/veronex/`

### Quick Start

```bash
# First-time setup
helm repo add bitnami https://charts.bitnami.com/bitnami
helm repo add redpanda https://charts.redpanda.com
helm repo update
helm dependency build deploy/helm/veronex/

# Install (all subcharts enabled by default)
helm install veronex deploy/helm/veronex/ \
  --set postgresql.auth.password="<pg-pass>" \
  --set postgresql.auth.username=veronex \
  --set postgresql.auth.database=veronex \
  --set veronex.cors.allowedOrigins="https://app.example.com"
```

> Secrets (`JWT_SECRET`, `ANALYTICS_SECRET`, `DATABASE_URL`, S3 keys) are managed via a chart-created K8s Secret by default. Passwords are **not** defaulted — you must provide them via `--set` or a values override file.

### External Infrastructure

Disable subcharts to use pre-existing services:

| Subchart | Disable flag | External config prefix |
|----------|-------------|------------------------|
| `postgresql` | `postgresql.enabled=false` | `externalPostgresql.{host,port,username,password,database}` |
| `valkey` | `valkey.enabled=false` | `externalValkey.{host,port,password}` |
| `minio` | `minio.enabled=false` | `externalMinio.{endpoint,accessKey,secretKey,bucket,region}` |
| `clickhouse` | `clickhouse.enabled=false` | `externalClickhouse.{host,port,username,password,database}` |
| `redpanda` | `redpandaEnabled=false` | `externalRedpanda.brokers` |

> **Note**: Redpanda uses top-level `redpandaEnabled` (not `redpanda.enabled`) due to Redpanda chart JSON schema restrictions.

### Secret Management

> **Policy**: Every credential env var in this chart MUST support all four secret modes (chart-managed, ESO, CSI, existingSecret). Adding a new secret requires updating **all three** secret templates: `secret.yaml`, `external-secret.yaml`, `secret-provider-class.yaml`.

#### Managed Secret Keys

All keys stored in the shared K8s Secret (`<release>-veronex-secrets`):

| Key | Required | Condition | Notes |
|-----|----------|-----------|-------|
| `JWT_SECRET` | ✓ | Always | Main API auth signing key |
| `ANALYTICS_SECRET` | ✓ | Always | Analytics service HMAC key |
| `DATABASE_URL` | ✓ | Always | PostgreSQL connection string |
| `S3_ACCESS_KEY` | ✓ | Always | Object storage access key |
| `S3_SECRET_KEY` | ✓ | Always | Object storage secret key |
| `GEMINI_ENCRYPTION_KEY` | ✓ | Always | Provider credential encryption key |
| `GEMINI_API_KEY` | Optional | `veronex.geminiApiKey` set | Gemini API access key |
| `CLICKHOUSE_USER` | Optional | `clickhouse.enabled` or `externalClickhouse.host` | ClickHouse username |
| `CLICKHOUSE_PASSWORD` | Optional | `clickhouse.enabled` or `externalClickhouse.host` | ClickHouse password |
| `KAFKA_USERNAME` | Optional | SASL enabled | Redpanda/Kafka SASL username |
| `KAFKA_PASSWORD` | Optional | SASL enabled | Redpanda/Kafka SASL password |
| `VERONEX_API_KEY` | Optional | `veronexMcp.veronexApiKey` set | MCP server API key |
| `BOOTSTRAP_SUPER_PASS` | Optional | `veronex.bootstrapSuperPass` set | Initial admin password |

#### Adding New Secrets (required checklist)

```
1. secret.yaml          — add stringData entry (conditional if optional)
2. external-secret.yaml — add optional remoteRef entry under eso.remoteRefs
3. secret-provider-class.yaml — add to secretObjects AND parameters.objects (same condition)
4. values.yaml          — add key under externalSecrets.eso.remoteRefs
5. Deployment template  — use secretKeyRef when ESO/CSI/existingSecret active, plain value otherwise
```

Three modes for production secret injection (mutually exclusive):

| Mode | Enable | How it works |
|------|--------|-------------|
| **Chart-managed** (default) | No extra config | Renders `secret.yaml` with `stringData` from values |
| **External Secrets Operator** | `externalSecrets.eso.enabled=true` | Renders `ExternalSecret` CR; ESO syncs from vault |
| **CSI Secrets Store** | `externalSecrets.csi.enabled=true` | Renders `SecretProviderClass`; CSI driver mounts secrets |
| **Pre-existing Secret** | `externalSecrets.existingSecretName=<name>` | Deployments reference your existing K8s Secret directly |

ESO example:
```bash
helm install veronex deploy/helm/veronex/ \
  --set externalSecrets.eso.enabled=true \
  --set externalSecrets.eso.secretStoreRef.name=aws-secrets \
  --set externalSecrets.eso.remoteRefs.jwtSecret=prod/veronex/jwt-secret \
  --set externalSecrets.eso.remoteRefs.analyticsSecret=prod/veronex/analytics-secret \
  --set externalSecrets.eso.remoteRefs.databaseUrl=prod/veronex/database-url \
  --set externalSecrets.eso.remoteRefs.s3AccessKey=prod/veronex/s3-access-key \
  --set externalSecrets.eso.remoteRefs.s3SecretKey=prod/veronex/s3-secret-key
```

### Components

| Template | Resource | Notes |
|----------|----------|-------|
| `veronex-deployment.yaml` | Deployment | API server, `envFrom` secretRef |
| `veronex-analytics-deployment.yaml` | Deployment | ClickHouse analytics service |
| `veronex-web-deployment.yaml` | Deployment | Next.js dashboard |
| `veronex-agent-statefulset.yaml` | StatefulSet + headless Service | Agent (ordinal-based sharding) |
| `veronex-mcp-deployment.yaml` | Deployment | veronex-mcp server (`veronexMcp.enabled=true`) |
| `veronex-mcp-service.yaml` | Service | ClusterIP, port 8080 |
| `veronex-mcp-hpa.yaml` | HPA | CPU-based autoscaling (`veronexMcp.autoscaling.keda=false`) |
| `veronex-mcp-keda.yaml` | ScaledObject | KEDA Prometheus autoscaling (`veronexMcp.autoscaling.keda=true`) |
| `vespa-statefulset.yaml` | StatefulSet | Vespa vector DB (`vespa.enabled=true`) |
| `vespa-service.yaml` | Service + headless | ClusterIP port 8080 + headless for StatefulSet |
| `otel-collector-deployment.yaml` | Deployment | OTel Collector (optional) |
| `migrate-job.yaml` | Job (hook) | Applies Postgres + ClickHouse schema on install/upgrade (`clickhouse-client --multiquery`) |
| `secret.yaml` | Secret | Chart-managed (skipped when ESO/CSI/existing) |
| `external-secret.yaml` | ExternalSecret | ESO mode |
| `secret-provider-class.yaml` | SecretProviderClass | CSI mode |
| `serviceaccount.yaml` | ServiceAccount | Optional (`serviceAccount.create`) |
| `hpa.yaml` | HPA | CPU-based (`autoscaling.enabled`, disabled when KEDA active) |
| `veronex-keda.yaml` | ScaledObject | KEDA — Valkey queue depth (`autoscaling.keda.enabled`) |
| `pdb.yaml` | PDB | Optional (`podDisruptionBudget.enabled`) |

### Autoscaling (KEDA)

HPA (CPU-based) and KEDA (queue-based) are mutually exclusive. KEDA is preferred for LLM workloads where CPU doesn't correlate with actual load.

```yaml
autoscaling:
  enabled: true
  keda:
    enabled: true              # KEDA ScaledObject (disables HPA)
    pollingInterval: 15        # seconds between Valkey checks
    cooldownPeriod: 120        # seconds before scale-down
    pendingJobsThreshold: "5"  # scale up when queue ZCARD > this
```

KEDA reads `ZCARD veronex:queue:zset` from Valkey. Requires KEDA operator installed in cluster.

Agent pods use dynamic replica discovery via `SCARD veronex:agent:instances` — no KEDA needed for agent (auto-adapts to any replica count).

### Optional Environment Variables (veronex API pod)

Set via `values.yaml` or `--set`. All optional — app falls back to built-in defaults when unset.

| values.yaml key | Env var | Default | Notes |
|-----------------|---------|---------|-------|
| `veronex.pgPoolMax` | `PG_POOL_MAX` | 10 | PostgreSQL connection pool size |
| `veronex.valkeyPoolSize` | `VALKEY_POOL_SIZE` | 16 | Valkey connection pool size per pod |
| `veronex.loginRateLimit` | `LOGIN_RATE_LIMIT` | 10 | Max login attempts per IP per 5-min window. `0` = disabled |
| `veronex.visionFallbackModel` | `VISION_FALLBACK_MODEL` | — | Model for vision requests on non-image providers (e.g. `llava:13b`) |
| `veronex.mcpVectorTopK` | `MCP_VECTOR_TOP_K` | 8 | Vespa ANN top-K for MCP tool selection |
| `veronex.vespaEnvironment` | `VESPA_ENVIRONMENT` | `"prod"` | Vespa environment partition key — isolates documents per environment (prod, dev, local-dev) on a shared Vespa instance |
| `veronex.vespaTenantId` | `VESPA_TENANT_ID` | `"default"` | Vespa tenant partition key — sub-partitions documents within an environment by team/org |
| `veronex.valkeyKeyPrefix` | `VALKEY_KEY_PREFIX` | `""` | Valkey key namespace prefix — isolates deployments sharing a single Valkey instance. Not injected when empty. Example: `"prod:"` → keys become `"prod:veronex:queue:zset"` |

Auto-injected (no values.yaml key needed):

| Env var | Source | Notes |
|---------|--------|-------|
| `VERONEX_INSTANCE_ID` | Pod name (downward API `metadata.name`) | Multi-pod health key isolation |
| `KAFKA_BROKER` | `veronex.redpandaBroker` helper | Injected when `redpandaEnabled=true` or `externalRedpanda.brokers` set |
| `CLICKHOUSE_HTTP_URL` | `veronex.clickhouseUrl` helper | Injected when `clickhouse.enabled=true` or `externalClickhouse.host` set |
| `CLICKHOUSE_USER` | `veronex.clickhouseUser` helper | Same condition as above |
| `CLICKHOUSE_PASSWORD` | `veronex.clickhousePassword` helper | Same condition as above |
| `CLICKHOUSE_DB` | `veronex.clickhouseDb` helper | Same condition as above |

### Vespa + Embed (MCP Vector Selection)

Single-node Vespa for MCP tool ANN search. Enabled via `vespa.enabled=true` or via external URL.

When `vespa.enabled=true`, `veronex-embed` is automatically co-deployed (embedding sidecar, port 3200).
`EMBED_URL` is auto-set to `http://{release}-veronex-embed:3200`; override with `embed.url` for external instances.
The embed service is probed by the health checker (`GET EMBED_URL/health`) and reported via `GET /v1/dashboard/services`.

```bash
# Internal Vespa (chart-managed)
helm install veronex . \
  --set vespa.enabled=true \
  --set veronex.vespaEnvironment=prod \
  --set veronex.vespaTenantId=default \
  --set embed.url=http://veronex-embed:3200

# External Vespa (pre-existing)
helm install veronex . \
  --set vespa.url=http://vespa.infra.svc:8080 \
  --set veronex.vespaEnvironment=prod \
  --set veronex.vespaTenantId=default \
  --set embed.url=http://veronex-embed:3200
```

**Multi-environment isolation**: `environment` and `tenant_id` are stored in every Vespa document. All queries filter on both keys — environments and tenants sharing a single Vespa instance never see each other's documents.

| Environment | `veronex.vespaEnvironment` | `veronex.vespaTenantId` |
|-------------|----------------------------|--------------------------|
| Production  | `prod` | `default` |
| Dev         | `dev` | `default` |
| Local dev   | `local-dev` (docker-compose default) | `default` |

> Changing `vespaEnvironment` creates a new partition — old documents remain until manually purged.

### Ingress

```bash
helm install veronex deploy/helm/veronex/ \
  --set ingress.enabled=true \
  --set ingress.host=veronex.example.com \
  --set ingress.tls.enabled=true \
  --set ingress.tls.secretName=veronex-tls
```
