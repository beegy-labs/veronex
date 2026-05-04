# Agents Customization: Sync & Migration

> SSOT | **Last Updated**: 2026-03-24 | Classification: Operational
> Sync behavior, migration guide, best practices, validation, and anti-patterns.

## Sync Behavior

### Automatic Sync (CI/CD)

When `llm-dev-protocol/AGENTS.md` changes:

1. **Read project's AGENTS.md**: Extract custom section
2. **Replace standard section**: Update with new standard from llm-dev-protocol
3. **Preserve custom section**: Keep project-specific content intact
4. **Write merged file**: Save to project's AGENTS.md

### Manual Sync

```bash
# From llm-dev-protocol
./scripts/sync-standards.sh

# Dry run to preview changes
./scripts/sync-standards.sh --dry-run
```

## Migration Guide

### Converting Existing AGENTS.md

If you have an existing `AGENTS.md` without markers:

```bash
# Run migration script
./scripts/migrate-agents-md.sh /path/to/project

# This will:
# 1. Backup existing AGENTS.md
# 2. Extract custom content (heuristic-based)
# 3. Add standard section with markers
# 4. Add custom section with markers
# 5. Merge and save
```

### Manual Migration

1. **Backup**: `cp AGENTS.md AGENTS.md.backup`

2. **Identify custom content**: Find sections unique to your project

3. **Restructure**:

   ```markdown
   <!-- BEGIN: STANDARD POLICY -->

   ... copy from llm-dev-protocol/AGENTS.md ...

   <!-- END: STANDARD POLICY -->

   ---

   <!-- BEGIN: PROJECT CUSTOM -->

   ... your custom sections ...

   <!-- END: PROJECT CUSTOM -->
   ```

4. **Validate**: Run `./scripts/validate-structure.sh /path/to/project`

## Best Practices

| Practice            | Description                                                      |
| ------------------- | ---------------------------------------------------------------- |
| **Standard First**  | Follow standard policy, add customizations only when necessary   |
| **Document Why**    | Explain why custom rules exist (regulatory, business, technical) |
| **Keep Current**    | Review custom section quarterly, remove outdated rules           |
| **Table Format**    | Use tables for structured data (easier for LLM parsing)          |
| **Cross-Reference** | Link to detailed docs in `.ai/` or `docs/llm/` when needed       |

## Validation

The validation script checks:

```bash
./scripts/validate-structure.sh /path/to/project

# Checks:
# [x] STANDARD POLICY markers present
# [x] PROJECT CUSTOM markers present
# [x] Standard section matches llm-dev-protocol version
# [x] No edits in standard section
# [!] Large custom section (>200 lines) - consider moving to docs/llm/
```

## Anti-Patterns

### [N] Don't

```markdown
<!-- BEGIN: PROJECT CUSTOM -->

## Complete Tech Stack Documentation

... 500 lines of detailed tech docs ...

<!-- END: PROJECT CUSTOM -->
```

**Problem**: Custom section too large, harder for LLM to parse

**Solution**: Move to `docs/llm/policies/architecture.md`, add summary table in custom section

### [N] Don't

```markdown
<!-- BEGIN: PROJECT CUSTOM -->

We use React. Also we have some API endpoints.

<!-- END: PROJECT CUSTOM -->
```

**Problem**: Unstructured text, low information density

**Solution**: Use tables and structured formats

### [Y] Do

```markdown
<!-- BEGIN: PROJECT CUSTOM -->

## Architecture & Stack

| Layer    | Tech               | Version | Notes                |
| -------- | ------------------ | ------- | -------------------- |
| Frontend | React              | 19.2    | See `.ai/apps/`      |
| API      | GraphQL Federation | 2.9     | Gateway at port 4000 |

**Full Documentation**: `docs/llm/policies/architecture.md`

<!-- END: PROJECT CUSTOM -->
```

## Version Control

### Git Conflicts

If sync causes merge conflicts in AGENTS.md:

```bash
# 1. Accept incoming standard section (from llm-dev-protocol)
git checkout --theirs AGENTS.md

# 2. Restore your custom section
git show HEAD:AGENTS.md | sed -n '/BEGIN: PROJECT CUSTOM/,/END: PROJECT CUSTOM/p' > custom.tmp

# 3. Replace custom section
# (Automated by sync script, or manually edit)

# 4. Verify
./scripts/validate-structure.sh .
```

### Standard Version Tracking

Custom section can reference standard version:

```markdown
<!-- BEGIN: PROJECT CUSTOM -->

> **Based on Standard Version**: 1.0.0 | **Last Project Update**: 2026-01-23

## Project-Specific Configuration

...

<!-- END: PROJECT CUSTOM -->
```

---

## References

- ADD Policy: `docs/llm/policies/add.md`
- CDD Policy: `docs/llm/policies/cdd.md`
- Token Optimization: `docs/llm/policies/token-optimization.md`
- Methodology: `docs/llm/policies/development-methodology.md`
- Setup Script: `scripts/setup-policy-links.sh`
- Source: `docs/llm/policies/agents-customization.md`
