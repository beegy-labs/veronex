# Integrated Development Workspace Tasks

> Target increment plan | [Specification](spec.md) | [Current implementation](../../../docs/llm/app-builder/implementation.md)

## Execution Rules

| Rule | Contract |
|------|----------|
| Status | Existing terminal/Pod/preview code is the baseline, not proof of target qualification. Unchecked items represent remaining work or evidence |
| Scope | Single active PTY per workspace; shared app/web service; centralized prompts and task history |
| Delivery | Reviewable additive phases behind the builder flag; preserve existing inference and retained builder behavior |
| Architecture | Spec owns target contracts; CDD implementation status records actual code and measured checks |
| Deployment | Server changes deploy only through platform-gitops; this checklist does not authorize live rollout |
| Verification | Behavioral tests and real pinned-CLI qualification; no fixture-only claim of native continuity |

## P0: Baseline and Contract Decisions

- [ ] P0.1 Map current handlers/Runner/tables into domain ports and migration plan; identify reusable SSOT boundaries.
- [ ] P0.2 Decide app form and supported platforms; prototype shared static client packaging if desktop is selected.
- [ ] P0.3 Pin real CLI versions and document launch/readiness/capture/resume/handoff capabilities and login scopes.
- [ ] P0.4 Measure inference and representative workspace resource baseline; agree regression budgets and initial quotas.
- [ ] P0.5 Confirm GitOps catalog chart/image source, storage/object store, retention, backup and credential arrangements.
- [ ] P0.6 Freeze additive API/events and generated app/web contract; document existing-route compatibility.

Exit: decisions and baseline evidence recorded; no assumptions promoted to implemented behavior.

## P1: Domain, Authorization and Durable Intent

- [ ] P1.1 Add upgrade-safe migrations for projects/organization bindings, tasks/runs, sessions, operations, artifacts and outbox.
- [ ] P1.2 Migrate personal owner records without access expansion; enforce scoped roles and server-derived actors.
- [ ] P1.3 Persist idempotency key/request hash, operation intent and outbox transactionally before ACK.
- [ ] P1.4 Add builder-only Valkey queue and reconstruct delivery from PostgreSQL after queue loss.
- [ ] P1.5 Preserve disabled startup, inference queue and existing route/lab-setting contracts.

Exit: A1, database upgrade and scoped authorization evidence; async metadata does not require an active runtime.

## P2: Controller and Workspace Storage

- [ ] P2.1 Extract Pod ownership from HTTP handlers into watch/reconcile controller with generation-based adoption.
- [ ] P2.2 Provision isolated workspace subtree, stable mount path, ownership and restricted Pod identity/profile.
- [ ] P2.3 Enforce one writer; confirm termination/fencing before replacement; handle duplicate starts and controller restart.
- [ ] P2.4 Implement checkpoint manifests, local database backups, object upload verification and restore validation.
- [ ] P2.5 Add quotas/backpressure, queued/Pending reasons, suspend/drain and archive lifecycle.
- [ ] P2.6 Prepare GitOps deployment contract and pruning/ownership qualification without live deployment here.

Exit: A2, A4, A10 and restore portions of A12.

## P3: Single Terminal Across Web and App

- [ ] P3.1 Share terminal transport/types/UI; implement chosen app packaging without duplicating business rules.
- [ ] P3.2 Add authorized attachment, one input owner, ownership handoff and immediate revocation.
- [ ] P3.3 Implement bounded cursor replay, resize/signals and independent client/process lifetimes.
- [ ] P3.4 Display workspace/runtime/session state, capture gaps and reconnect without input replay.
- [ ] P3.5 Qualify a pinned real CLI through both clients.

Exit: A3 and attachment portions of A8/A9.

## P4: Central Prompts and Task Ledger

- [ ] P4.1 Add prompt library, drafts/publish permissions and immutable revisions.
- [ ] P4.2 Bind submissions to task/run, rendered prompt, variables, context/instruction versions and code snapshot.
- [ ] P4.3 Persist snapshot before adapter dispatch; readiness-gate input and record attempted/confirmed/failed/unknown states.
- [ ] P4.4 Reconcile ambiguous delivery without automatic resend; support ad hoc prompts using the same ledger.
- [ ] P4.5 Build shared prompt composer/library and execution timeline in web/app.

Exit: A5; historic submissions remain reproducible after template edits.

## P5: Durable History and Recovery

- [ ] P5.1 Implement versioned native/hook adapters with source IDs, sequences and explicit capture capabilities.
- [ ] P5.2 Replace bounded last-N history scans with incremental capture and cursor pagination.
- [ ] P5.3 Persist immutable output/native chunks and verified references; expose durable cursor, upload lag and capture gaps.
- [ ] P5.4 Add bounded outage spool, deduplicated ingestion, restricted artifacts and auditable exports/retention.
- [ ] P5.5 Qualify native resume and cross-CLI handoff using exact sessions, checkpoint evidence and delivery verification.
- [ ] P5.6 Exercise uncertain side-effect recovery without blindly repeating commands, push or merge.

Exit: A4, A6, A8, A9 and capture/storage portions of A7/A12.

## P6: Analytics and Release Qualification

- [ ] P6.1 Deliver durable outbox events through the existing analytics boundary with replay and idempotent projection.
- [ ] P6.2 Add ClickHouse usage/latency/error projections and deduplicated aggregates; unknown usage stays unknown.
- [ ] P6.3 Exercise database, queue, storage, network and analytics failures plus storage migration/backup restore.
- [ ] P6.4 Verify existing preview/Git behavior, credential isolation and policy-controlled remote side effects.
- [ ] P6.5 Measure loaded inference regression, terminal backpressure and workspace quotas.
- [ ] P6.6 Prepare platform-gitops rollout/rollback changes; enable only after applicable A1-A13 evidence and operational approval.

Exit: remaining A7, A10-A13 with documented limitations and rollback evidence.

## Deferred Backlog

| Work | Condition |
|------|-----------|
| Harness/multi-agent scheduler | Reuse task/run/event contracts after single-terminal reliability is proven |
| Expanded preview isolation and workflows | Separate scope; preserve current behavior and qualify enabled paths |
| Additional CLI/local-model adapters | Version-specific qualification against the same capability contract |
| Automatic merge | Explicit policy decision and remote-state reconciliation |
| Physical-node autoscaling | Infrastructure scope in platform-gitops; never an implicit terminal-side action |
