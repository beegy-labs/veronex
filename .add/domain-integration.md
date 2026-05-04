# Domain Integration

> Trigger: exposing a new app/service publicly, or moving an existing service between exposure paths.
> SSOT: `platform-gitops`. This repo's job ends at `Service` + `Deployment`.

## Two exposure paths

Pick by the longest streaming/response time the service must hold.

| Path | Hostname suffix | TLS | Idle cap | Use when |
| ---- | --------------- | --- | -------- | -------- |
| **CF-proxied** (default) | `*.verobee.com`, `*.girok.dev` (orange cloud) | CF Edge | **100s** (free/pro) | Short HTTP — UI, REST, auth, SSE that emits within 100s |
| **CF-bypass** (direct) | `*.girok.dev` only (gray cloud / DNS-only) | Cilium gateway (`girok-tls-secret`) | per-route `timeouts` (Gateway API v1.1) | Long cold-load + streaming (model load, big SSE/WS, anything > 100s idle) |

CF-bypass DNS resolves CNAME → `home-gw.girok.dev` (DDNS-managed A → home public IP) → home router NAT → `cilium-gateway-web-gateway` (192.168.1.251). Listener `*.girok.dev` HTTPS/443 already provisioned with wildcard cert.

## Topology

```
CF-proxied:   client → CF Edge (100s idle) → CF Tunnel → cilium-gateway-web-gateway:80
CF-bypass:    client → DNS → home public IP → router NAT 443 → cilium-gateway-web-gateway:443
```

In-cluster traffic between services (api → mcp, api → ollama) is L4 only — no gateway, no CF, no DNS. Internal calls don't need any of this.

## Adding a CF-bypass route (the long-streaming case)

Two changes in `platform-gitops`, ideally in one PR. **Both required** — DNS without route → 404; route without DNS → NXDOMAIN.

### 1. Cilium HTTPRoute

`clusters/<cluster>/values/cilium-gateway-values.yaml` — append under `httpRoutes:`:

```yaml
- name: <service>-direct-<env>-route
  enabled: true
  gateways:
    - web-gateway
  hostnames:
    - <service>-<env>.girok.dev
  requestHeaderModifier:
    set:
      - name: X-Forwarded-Proto
        value: "https"
  timeouts:                        # Gateway API v1.1, Cilium ≥1.18
    request: 1800s                 # ≥ longest expected end-to-end
    backendRequest: 1800s          # same; backend attempt cap
  backendRef:
    name: <service>
    namespace: <namespace>
    port: <port>
```

Template (`infrastructure/cilium-gateway/templates/httproutes.yaml`) honors `.timeouts.{request,backendRequest}` per-rule. Listener `*.girok.dev` already exists — no gateway change needed.

### 2. CF DNS record

`clusters/<cluster>/values/cloudflare-ddns-values.yaml` — append under `staticRecords:`:

```yaml
- type: CNAME
  name: <service>-<env>            # subdomain only; zone is girok.dev
  content: home-gw.girok.dev
  proxied: false                   # CRITICAL — `true` re-applies CF 100s idle
  ttl: 300
```

The `cloudflare-ddns` CronJob (every 5 min) reconciles via CF API token (Vault `secret/infrastructure/cloudflare#api_token`). Idempotent — created if missing, updated if drifted, never deleted.

## Adding a CF-proxied route (the default case)

Skip step 2 — CF DNS for `*.verobee.com` is wildcard-mapped through CF Tunnel, no per-record action. Just add the HTTPRoute with the `*.verobee.com` hostname. Omit `timeouts` (CF Edge 100s caps you anyway).

## Rules

| Allowed | Forbidden |
| ------- | --------- |
| Bypass for streaming `/v1/chat/completions`, MCP SSE, long polling | Bypass for UI / short REST — wastes the CF caching/DDoS layer |
| `proxied: false` on bypass CNAMEs | `proxied: true` on bypass CNAMEs (defeats the change) |
| Per-route `timeouts` matching the *application* timeout | Gateway timeout < application timeout (causes truncation) |
| Internal callers (api → mcp) on cluster DNS (`<svc>.<ns>.svc.cluster.local`) | Internal callers via the public hostname (extra hop, breaks on DNS outage) |
| Single bundled platform-gitops PR for route + DNS | Split PRs — leaves a broken intermediate state on either merge order |

## Pre-PR check

```bash
# Verify the timeouts field renders correctly
helm template infrastructure/cilium-gateway \
  -f clusters/home/values/cilium-gateway-values.yaml \
  | grep -A3 "name: <service>-direct-<env>-route" -A20 | grep timeouts -A2

# Verify the static record renders
helm template infrastructure/cloudflare-ddns \
  -f clusters/home/values/cloudflare-ddns-values.yaml \
  | grep "ensure_record.*<service>-<env>"
```

## After merge

| Check | How |
| ----- | --- |
| HTTPRoute Accepted | `kubectl -n system-network get httproute <name> -o jsonpath='{.status.parents[*].conditions[?(@.type=="Accepted")].status}'` |
| DNS resolves | `dig +short <host>` returns `home-gw.girok.dev` then a public A record |
| Reaches gateway | `curl -I https://<host>/healthz` returns expected status (not 524, not connection refused) |
| `proxied=false` confirmed | CF UI shows gray cloud, OR `dig <host>` returns 1 IP not the CF anycast block (104.x / 172.x) |

## Reference

- `infrastructure/cilium-gateway/` — gateway listeners + HTTPRoute template
- `infrastructure/cloudflare-ddns/` — DDNS A + static records reconciler
- `infrastructure/cloudflare-tunnel/` — CF Tunnel ingress (CF-proxied path only)
- `clusters/home/values/cilium-lbipam-values.yaml` — LB IP pool (web-gateway = 192.168.1.251)
