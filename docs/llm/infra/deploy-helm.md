# Deploy: Helm

> SSOT | **Last Updated**: 2026-03-24 | Classification: Operational
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
| `weather-mcp-deployment.yaml` | Deployment | veronex-mcp server (`weatherMcp.enabled=true`) |
| `weather-mcp-service.yaml` | Service | ClusterIP, port 8080 |
| `weather-mcp-hpa.yaml` | HPA | CPU-based autoscaling (`autoscaling.keda=false`) |
| `weather-mcp-keda.yaml` | ScaledObject | KEDA autoscaling (`autoscaling.keda=true`) |
| `otel-collector-deployment.yaml` | Deployment | OTel Collector (optional) |
| `clickhouse-init-job.yaml` | Job (hook) | Applies ClickHouse schema on install/upgrade |
| `secret.yaml` | Secret | Chart-managed (skipped when ESO/CSI/existing) |
| `external-secret.yaml` | ExternalSecret | ESO mode |
| `secret-provider-class.yaml` | SecretProviderClass | CSI mode |
| `serviceaccount.yaml` | ServiceAccount | Optional (`serviceAccount.create`) |
| `hpa.yaml` | HPA | Optional (`autoscaling.enabled`) |
| `pdb.yaml` | PDB | Optional (`podDisruptionBudget.enabled`) |

### Ingress

```bash
helm install veronex deploy/helm/veronex/ \
  --set ingress.enabled=true \
  --set ingress.host=veronex.example.com \
  --set ingress.tls.enabled=true \
  --set ingress.tls.secretName=veronex-tls
```
