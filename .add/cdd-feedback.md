# CDD Feedback

> ADD Reference | **Last Updated**: 2026-03-25

## Trigger

After any task completion (feature, fix, refactor, migration) — when a new pattern or constraint was discovered.

## Execution

CDD sync steps are embedded in every workflow's final step. If a standalone CDD update is needed:

| Step | Action |
|------|--------|
| 1 | `git diff` the completed work — identify new patterns or constraints |
| 2 | Classify: Operational (accumulate freely) vs Constitutional (requires human approval) |
| 3 | Route to the correct doc — see `.add/README.md` CDD Sync Routing or `best-practices.md` Part 1 "Where to write what" |
| 4 | Update `Last Updated` date in the target doc |
| 5 | Constitutional change → flag for human approval before merging |

## Rules

| Rule | Detail |
|------|--------|
| Operational | Accumulate freely (patterns, conventions, component inventory) |
| Constitutional | Require human approval (identity, policy, security contracts) |
| No speculation | Only document what was actually built and verified |
| No duplication | One fact → one doc. If already documented, update in place |
