# Commit Fix

> ADD Execution | **Last Updated**: 2026-03-25

## Trigger

CI "Validate commit messages" fails, or user requests commit message cleanup.

## Read Before Execution

- `docs/llm/policies/git-flow.md` — commit format rules and CI regex

## CI Regex

```
^(feat|fix|chore|docs|refactor|test|perf|ci|build|revert|style)\([a-z0-9-]+\): .+
```

Fails when:
- Scope missing: `docs: ...` instead of `docs(scope): ...`
- Scope has underscore: `fix(mcp_bridge): ...`
- Unknown type: `update(api): ...`
- AI co-author in body: `Co-Authored-By: Claude ...`
- `git revert` auto-message: `Revert "..."` → must be `revert(scope): ...`
- Non-standard prefix: `Reapply "..."`, `WIP: ...` → fix or drop

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Find failing commits: `git log --format="%H %s" origin/develop..HEAD` |
| 2 | Test each against CI regex to identify violations |
| 3 | Stash uncommitted changes: `git stash` |
| 4 | Create rebase todo script + editor script (see below) |
| 5 | Run: `GIT_SEQUENCE_EDITOR=<todo.sh> GIT_EDITOR=<msg.sh> git rebase -i origin/develop` |
| 6 | Pop stash: `git stash pop` |
| 7 | Validate all messages pass CI regex |
| 8 | Force push: `git push --force-with-lease origin <branch>` |

## Script Templates

**todo.sh** — mark target commit(s) as `reword`:

```bash
#!/bin/sh
sed -i.bak "s/^pick <hash>/reword <hash>/" "$1"
```

**msg.sh** — replace commit message:

```bash
#!/bin/sh
echo "<type>(<scope>): <subject>" > "$1"
```

**msg-strip-coauthor.sh** — remove AI co-author from body:

```bash
#!/bin/sh
grep -v "Co-Authored-By:.*[Cc]laude\|Co-Authored-By:.*[Gg][Pp][Tt]" "$1" > "$1.tmp" && mv "$1.tmp" "$1"
```

## Pre-Push Validation (run before every push)

```bash
git log --format="%H %s" origin/develop..HEAD | while read hash msg; do
  echo "$msg" | grep -qE '^(feat|fix|chore|docs|refactor|test|perf|ci|build|revert|style)\([a-z0-9-]+\): .+' \
    && echo "ok  $msg" || echo "BAD $msg"
done
```

Run this before `git push` to catch violations before CI does.

## Rules

| Rule | Detail |
| ---- | ------ |
| Use `--force-with-lease` | Never bare `--force` |
| Rebase from `origin/develop` | Not from local develop |
| Always validate after rebase | Re-run CI regex check before push |
| Multiple violations | Fix all in one rebase pass |
| `git revert` creates non-conforming messages | Always rename: `revert(scope): ...` |
| Drop Revert+Reapply pairs | They're a no-op — squash or drop both |
