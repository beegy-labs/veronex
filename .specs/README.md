# SDD (Spec-Driven Development)

> CDD-derived change plan | Policy: `docs/llm/policies/sdd.md` | **Last Updated**: 2026-03-15

## Structure

```
.specs/veronex/
├── {feature}.md            # Spec (What to build)
├── roadmap.md              # L1: Master direction (planning only)
├── scopes/                 # L2: Active scope
│   └── {scope}.md
├── tasks/                  # L3: Detailed tasks (LLM executes)
│   └── {scope}.md
└── history/                # Completed scopes
```

## Token Load Strategy

| Situation | Load | Skip |
|-----------|------|------|
| Planning | `roadmap.md` | scopes, tasks, history |
| Work start | `scopes/{scope}.md` + `tasks/{scope}.md` | roadmap, history |
| Working | `tasks/{scope}.md` | everything else |
