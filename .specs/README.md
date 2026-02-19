# SDD (Spec-Driven Development)

> WHAT to build | Policy: `docs/llm/policies/sdd.md` (via agentic-dev-protocol)

## Structure

```
.specs/inferq/
├── roadmap.md              # L1: Master direction (planning only)
├── scopes/                 # L2: Active scope
│   └── 2026-Q1.md
├── tasks/                  # L3: Detailed tasks (LLM executes)
│   └── 2026-Q1/
│       ├── index.md
│       └── *.md
├── references/             # Best practices & research
│   └── best-practices.md
└── history/                # Completed scopes
```

## Token Load Strategy

| Situation | Load | Skip |
|-----------|------|------|
| Planning | `roadmap.md` | scopes, tasks, history |
| Work start | `scopes/2026-Q1.md` + `tasks/2026-Q1/index.md` | roadmap, history |
| Working | `tasks/2026-Q1/*.md` | everything else |
