# Git Flow & Commit Policy

> SSOT for branch strategy and commit conventions | **Last Updated**: 2026-02-19

## Branch Strategy

```
feat/* ──squash──▶ develop ──merge──▶ main
                    (Dev)              (Prod)
```

| Branch    | Purpose                     | Protected |
| --------- | --------------------------- | --------- |
| `main`    | Stable production releases  | Yes       |
| `develop` | Active development          | Yes       |
| `feat/*`  | Feature work (short-lived)  | No        |
| `fix/*`   | Bug fixes (short-lived)     | No        |

> **`release` branch**: Not used at current scale.
> Will be introduced when a separate staging environment or multi-contributor QA freeze is needed.

## Merge Strategy

| Source → Target | Strategy | Rationale |
| --------------- | -------- | --------- |
| `feat/*` → `develop` | **Squash** | Clean linear history on develop |
| `develop` → `main` | **Merge** | Preserve develop history in main |

```bash
# feat to develop
gh pr merge --squash --delete-branch

# develop to main
gh pr merge --merge
```

## Commit Format

```
<type>(<scope>): <subject>
```

### Types

| Type | When to Use |
| ---- | ----------- |
| `feat` | New feature or capability |
| `fix` | Bug fix |
| `refactor` | Code change without behavior change |
| `docs` | Documentation only |
| `test` | Adding or fixing tests |
| `chore` | Build, CI, dependency updates |
| `perf` | Performance improvement |

### Scopes

| Scope | Area |
| ----- | ---- |
| `domain` | Domain entities / value objects |
| `queue` | Queue adapter or logic |
| `gpu` | GPU worker / inference adapter |
| `sse` | SSE streaming |
| `api` | HTTP API adapter |
| `ci` | CI/CD workflows |
| `policy` | Submodule / protocol updates |

### Examples

```
feat(queue): add priority queue support
fix(gpu): handle OOM error during inference
refactor(domain): extract StreamToken value object
docs(api): add SSE endpoint specification
chore(ci): update GitHub Actions to v4
test(queue): add unit tests for RedisQueueAdapter
```

### Rules

| Rule | Detail |
| ---- | ------ |
| Subject case | lowercase |
| Subject tense | imperative ("add", not "added") |
| Max subject length | 72 characters |
| No period at end | `feat: add X` not `feat: add X.` |
| No AI mention | Never reference Claude, GPT, AI in commits |
| Language | English only |

## Feature Branch Workflow

```bash
# 1. Create feature branch from develop
git checkout develop
git pull origin develop
git checkout -b feat/queue-priority

# 2. Commit changes
git commit -m "feat(queue): add priority field to InferenceJob"

# 3. Open PR to develop
gh pr create --base develop --title "feat(queue): add priority queue support"

# 4. Merge (squash)
gh pr merge --squash --delete-branch
```

## Release Workflow

```bash
# When develop is stable and ready for production
gh pr create --base main --head develop --title "release: vX.Y.Z"
gh pr merge --merge
git tag vX.Y.Z
```
