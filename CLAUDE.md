# CLAUDE.md

> Claude Code config — derived from [AGENTS.md](AGENTS.md) | **Last Updated**: 2026-05-03

## Start

Read [.ai/README.md](.ai/README.md) — project overview, navigation, domain docs, tech stack.

## Frameworks

| Directory | Framework | Role |
| --------- | --------- | ---- |
| `.ai/` + `docs/llm/` | CDD | System SSOT — rules, patterns, architecture, constraints |
| `.specs/` | SDD | Change plans — specs, tasks, scope |
| `.add/` | ADD | Execution — workflow prompts, policy selection |

## Workflows (ADD)

| Action | Workflow |
| ------ | -------- |
| Code review / optimization | `.add/code-review.md` |
| Doc sync | `.add/doc-sync.md` |
| Dockerfile / docker workflow authoring | `.add/dockerfile-authoring.md` |
| Domain / public exposure (CF / Cilium / DDNS) | `.add/domain-integration.md` |
| Add design token (--vds-theme-*) | `.add/design-token-add.md` |
| Tune existing design token (color/contrast/comfort) | `.add/design-token-tune.md` |
| Add arbitrary-value vds-* class | `.add/design-arbitrary-utility.md` |
| Run WCAG contrast audit | `.add/design-contrast-audit.md` |
| Sync verodesign upstream bundle | `.add/design-verodesign-sync.md` |
