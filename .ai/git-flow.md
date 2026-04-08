# Git Flow

> Branch & commit strategy | **Last Updated**: 2026-03-07

## Branch Flow

```
feat/* ──squash──▶ develop ──merge──▶ main
                    (Dev)              (Prod)
```

## Merge Strategy

| Source → Target | Type   | Command                |
| --------------- | ------ | ---------------------- |
| feat → develop  | Squash | `gh pr merge --squash` |
| develop → main  | Merge  | `gh pr merge --merge`  |

## Feature Workflow

```bash
git checkout -b feat/new-feature
git commit -m "feat(scope): description"
gh pr create --base develop
gh pr merge --squash --delete-branch
```

## Commit Format

```
<type>(<scope>): <subject>
```

| Field | Rule |
| ----- | ---- |
| type | `feat` `fix` `refactor` `docs` `test` `chore` `perf` `ci` `build` `revert` `style` |
| scope | `[a-z0-9-]+` only — no spaces, no `+`, no `/` |
| subject | non-empty, lowercase start |

CI enforces: `^(type)\([a-z0-9-]+\): .+`

**SSOT**: `docs/llm/policies/git-flow.md`
