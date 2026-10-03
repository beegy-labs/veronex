# App Builder Requirements

> CDD Layer 2 — User-confirmed product constraints | **Last Updated**: 2026-10-03

## Confirmed Scope

| Area | Requirement |
|------|-------------|
| Product | Extend Veronex with a provider-independent development tool: web and app, remote workspaces, centralized prompts and organization-scoped work history |
| Existing features | Preserve existing inference APIs, providers, authentication, queue semantics, dashboards, analytics and deployment behavior; the builder is an additive opt-in feature |
| Current authorization | Commit existing implementation/SSOT cleanup and the revised SDD; push the current branch. New planned features and live deployment are separate work |
| Backend | All new platform backend code in Rust: PTY bridge, workspace management, CLI adapters, Pod lifecycle, history, handoff, process management, preview proxy |
| Web | Keep the existing Next.js frontend; the Rust requirement excludes web code |
| Execution | Run actual Codex CLI, Claude Code, Gemini CLI and future local-model/custom CLIs inside workspaces; third-party CLI runtimes are subprocess dependencies |
| Current increment | One active terminal per workspace; independent workspaces add Pods. Harness and expanded preview capabilities follow later |
| App | Share backend, contracts and terminal UI with web; desktop/mobile/PWA form remains undecided |
| Prompts | Immutable prompt revisions and actual submission snapshots bound to task/run and code/context evidence |
| Work history | Retain observable prompts, commands and outcomes with actor, source, completeness and organization/project access scope |
| Data services | PostgreSQL as durable authority; Valkey coordination; ClickHouse derived analytics; NFS working files and immutable archive storage |
| Terminal | Render the actual interactive CLI terminal in the browser; preserve input, output, resizing and reconnect behavior |
| Continuity | Continue the same repository and task across computers, browser reconnects, Pod replacements and CLI changes |
| CLI switching | Switch Codex to Claude Code and back, or to a future local-model/custom CLI, while retaining work history |
| Local models | Support future local-model CLIs through the same workspace/terminal/preview contract; platform implementation must not require a new agent reasoning loop |
| Preview | An Execute action starts an application instance; N instances expose N independently selectable previews, logs and stop controls |
| Runtime deployment | All server deployment belongs to platform-gitops; a runtime controller creates workspace Pods within deployed profiles and quotas |
| Initial storage | Single-node NFS server, shared RWX PVC, workspace-specific directories; task Pods may run on different nodes |
| Storage evolution | Allow migration to Longhorn RWX and subsequently CephFS through data migration and PVC configuration changes |

## Proposed Runtime Design

| Component | Responsibility |
|-----------|----------------|
| Workspace | Persistent repository identity, independent checkout/branch, stable container path, history and preview definitions |
| Runner | Rust process supervisor with PTY support; launches the selected real CLI and manages its process group |
| CLI adapter | Versioned executable/arguments, authentication references, native session capture/resume, handoff injection and capability reporting |
| Browser connection | Attach to an existing terminal; disconnecting the browser does not terminate the CLI |
| Agent execution | Preserve the active CLI process; suspend an idle workspace only after a consistent checkpoint |
| CLI replacement | End or quiesce the previous writer, capture state, then launch/resume the selected CLI in the same workspace |
| Application execution | Manage preview processes independently of the CLI; CLI switching preserves running previews when the Pod remains available |
| Resource policy | Bound concurrent workspaces; queue excess work; add Pods for independent work; increase resources for one oversized task |
| Existing workload isolation | Separate builder queue keys, concurrency budgets and Kubernetes resources; feature disabled by default via existing LabSettingsRepository/useLabSettings mechanisms |
| Preview routing | Unique instance ID, upstream port and URL; support HTTP and WebSocket forwarding for application development servers |
| Runtime separation | Keep CLI credentials and native history available only to the CLI container; preview processes run in a separate container within the workspace Pod with only required code/output mounts |
| Parallel development | One active agent writer per checkout; independent simultaneous coding tasks use separate checkouts |

## History and Context Contract

| Record | Contents and Behavior |
|--------|-----------------------|
| Workspace history | Append-only user instructions, observable CLI messages, tool results, lifecycle events and CLI identity; archived independently of vendor session cleanup |
| Terminal recording | Terminal output for replay; structured history comes from supported hooks, exports or versioned native-session readers |
| Native sessions | Preserve each CLI's own session artifacts and ID for resuming with that CLI; formats remain adapter-specific |
| Portable handoff | User requirements, decisions and reasons, completed/pending work, failed attempts, next action, source-event references |
| Code checkpoint | Base/HEAD commit, branch, tracked changes, untracked source files and workspace revision; associate verification results with that revision |
| Return to a previous CLI | Resume its native session when supported, then supply intervening work and current code state before further edits |
| Context limits | Keep original records retrievable; summaries are derived records, carry source coverage and cannot replace user requirements |
| Restoration limit | Preserve observable history and durable work state; do not claim transfer of hidden model state or lossless model attention |
| Future CLI support | Basic PTY execution plus declared resume/export/handoff capabilities; missing capabilities must be visible rather than silently assumed |
| Storage layout | Separate workspace code, each CLI's native state and portable checkpoints; credentials remain outside portable handoff records |
| Runtime databases | Keep SQLite/WAL state on a supported local filesystem; archive consistent backups for Pod replacement instead of sharing live databases over NFS |

## Proposed Switch Sequence

| Step | Action |
|------|--------|
| 1 | Record requested target CLI; complete or interrupt active tool execution and confirm the previous writer is stopped |
| 2 | Flush captured events/native state and save a checkpoint with code revision and event cursor |
| 3 | Build a versioned handoff from verified code/tool results, user instructions and referenced history |
| 4 | Start the target CLI or resume its native session with the same workspace path and applicable credentials |
| 5 | Deliver the handoff through the adapter's supported prompt/context mechanism; preserve existing repository instructions |
| 6 | Verify delivery at the supported interface; reconcile actual files and uncertain tool outcomes before continuing |
| 7 | Append the new CLI execution to the same workspace timeline and retain earlier native sessions |

## Acceptance Scenarios

| Scenario | Expected Result |
|----------|-----------------|
| Codex -> Claude Code | Same checkout and visible history; Claude receives current requirements, progress, code state and next action |
| Claude Code -> previous Codex session | Codex receives all intervening changes and revised instructions |
| Future local-model/custom CLI | Register an adapter; use the same terminal, workspace, handoff and preview paths |
| Browser reconnect | Reattach to the running CLI and application previews |
| Pod replacement | Restore the latest consistent checkpoint; report uncertain/incomplete work and restart previews from saved definitions |
| N application runs | N unique preview instances with independent status, logs, routing and stop controls |
| CLI history unavailable | Expose capture/resume limitations and use available portable records; never report full context restoration without evidence |
| Feature disabled | Existing routes, inference results, UI navigation, configuration defaults and workloads retain their behavior; no builder Pods are created |
| Feature enabled under load | Builder sessions and previews cannot bypass their resource limits or change existing inference queue behavior |
| Preview isolation | Application code cannot read CLI credentials or native session archives through its process environment or mounted filesystem |

## Open Decisions

| Decision | Status |
|----------|--------|
| App form | Desktop Tauri is a provisional proposal; client platform choice is not yet confirmed |
| Merge policy | User approval versus automatic merge after checks remains undecided |
| Runtime sizing | Initial Pod requests/limits, concurrency and idle timeout require workload measurements |
| History retention | Define archive retention and CLI cleanup settings before promising long-term recovery |
| CLI versions | Pin supported versions and test native state restoration and handoff delivery for each adapter |

## References

| Source | Purpose |
|--------|---------|
| [Codex session locations](https://learn.chatgpt.com/docs/reference/troubleshooting) | Native session transcripts and archives |
| [Claude Code application data](https://code.claude.com/docs/en/claude-directory) | Native transcripts, tool outputs and cleanup behavior |
| [Gemini CLI session management](https://geminicli.com/docs/cli/session-management/) | Project-specific sessions and resume behavior |
| [Context compression](../inference/context-compression.md) | Existing inference conversation storage and summaries; coding workspace recovery requires additional state |
| [Implementation scope](../../../.specs/veronex/app-builder/spec.md) | Planned architecture, interfaces, recovery, previews and regression acceptance |
| [Implementation tasks](../../../.specs/veronex/app-builder/tasks.md) | Target qualification and release gates |
| [Implementation status](implementation.md) | Current code, checks and remaining gaps |
