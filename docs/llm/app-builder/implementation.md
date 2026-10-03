# App Builder Implementation

## Current Code

| Area | Implemented |
|------|-------------|
| Data | Additive PostgreSQL repository, workspace, event, preview and pull request tables; disabled builder lab flag and permission |
| API | Rust repository/workspace CRUD, Pod lifecycle, terminal relay, history ingestion, CLI switch, Git actions, preview and Gitea pull request routes |
| Runner | Rust PTY and preview process supervision, Codex/Claude/Gemini launch, native record capture, SQLite checkpoint and Git credential helper |
| Storage | Workspace-specific directories on a configurable RWX PVC; optional static single-node NFS PV/PVC |
| Preview | Multiple preview processes per workspace, logs, readiness, dedicated host forwarding with signed browser ticket |
| Web | Repository/workspace pages, browser terminal, CLI selector, history, Git controls and preview grid |
| Deployment | Disabled-by-default Helm resources, namespace-scoped Pod RBAC, separate CLI/preview Runner tokens |

## Current Boundaries

| Area | State |
|------|-------|
| Planned increment | Organization/project authorization, tasks/runs, central prompt revisions/submissions, durable outbox, controller and app client are not implemented; see [SDD](../../../.specs/veronex/app-builder/spec.md) |
| Orchestration | Existing Rust API creates Pods directly and applies a PostgreSQL advisory lock for admission; the target controller and dedicated Valkey builder queue are not implemented |
| Scaling | One Pod per active workspace and configurable active-workspace limit; create separate workspaces for parallel tasks; no automatic Pod scale-up on memory pressure |
| Continuity | Native files remain on the PVC, Codex SQLite is checkpointed to the PVC, and switch prompts point to a generated handoff; actual vendor-authenticated resume and delivery are not qualified |
| History | Native records and terminal logs are retained on the PVC, with a bounded indexed timeline in PostgreSQL; current capture limits can omit older indexed records |
| Previews | One preview container per workspace runs multiple commands against the same checkout; build output paths and process isolation between previews are not guaranteed |
| Git | Gitea pull request create/status/merge is implemented; CI policy and branch protection are enforced by the Git host configuration |
| Release | No cluster rollout or real CLI authentication test has been performed; Git publication does not qualify runtime behavior |

## Verification

| Check | Result |
|-------|--------|
| Rust workspace | `cargo test --workspace -- --test-threads=1`: 728 passed, 31 ignored; serial execution avoids existing environment-variable test interference |
| SSOT cleanup | Shared schema/UI/projections consolidated, unused paths removed; two consecutive scoped audits found no additional actionable duplication/dead code |
| Web | Production build, 148 tests and no-Tailwind lint pass; TypeScript unused locals/parameters checks enabled |
| Helm | Chart lint passes with builder disabled and enabled test values |
| Clippy | Standard workspace Clippy passes with warnings; strict `-D warnings` remains blocked by Builder style/test warnings |
| Runtime | Kubernetes, NFS and vendor-authenticated end-to-end paths remain unverified |

## Setup

| Setting | Purpose |
|---------|---------|
| `builder.enabled` | Generate builder RBAC, preview ingress and API environment |
| `builder.runnerImage` | Built image from `crates/veronex-builder-runner/Dockerfile` |
| `builder.pvc` | Existing RWX workspace PVC, or use `builder.storage.nfs` to create a static PV/PVC |
| `builder.browserOrigin` | Existing web origin allowed to embed previews |
| `builder.previewDomain` | Wildcard DNS suffix pointing to the Veronex ingress |
| `builder.previewTlsSecret` | TLS secret covering the wildcard preview host when the browser uses HTTPS |
| `builder.existingSecretName` | Secret with `RUNNER_TOKEN`, `PREVIEW_TOKEN` and `GIT_TOKEN` keys in the API and builder namespaces |
| `builder.gitHosts` | Optional comma-separated HTTPS Git host allowlist |

Enable the `builder_enabled` lab setting for the intended account after the Helm resources and image exist. The preview domain must have wildcard DNS, and an NFS export must permit UID/GID 1000 to write workspace directories.
