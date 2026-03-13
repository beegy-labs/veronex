# Code Review

> CDD Tier 1 — Pointer only | **Last Updated**: 2026-03-12

## Policy SSOT

**Common review policy**: `docs/llm/policies/code-review.md`
Covers: philosophy (consistent / concise / simple / O(1)), SSOT map, architecture, security, patterns, TDD.

## Domain-Specific Review

Run alongside the common policy when reviewing the scheduler / capacity stack.

**Spec (primary truth)**: `.specs/veronex/scheduler.md`

Source files to read first:

| Role | Path |
|------|------|
| VRAM / AIMD / Thermal | `docs/llm/inference/capacity.md` |
| Thermal state machine | `docs/llm/providers/hardware.md` |
| Job lifecycle / Path A–B | `docs/llm/inference/job-lifecycle.md` |

Implementation targets:

```
application/ports/outbound/concurrency_port.rs
application/ports/outbound/thermal_port.rs
infrastructure/outbound/capacity/vram_pool.rs
infrastructure/outbound/capacity/distributed_vram_pool.rs
infrastructure/outbound/capacity/thermal.rs
infrastructure/outbound/capacity/analyzer.rs
infrastructure/outbound/health_checker.rs
application/use_cases/inference/dispatcher.rs
application/use_cases/placement_planner.rs
infrastructure/outbound/queue_maintenance.rs
bootstrap/background.rs
```

Key invariants to verify:

- Soft→Normal: `temp < normal_below AND provider_active == 0`
- Hard→Cooldown: `provider_active == 0` (placement_planner.set_cooldown()) OR 300s elapsed (자동 fallback); 90s는 watchdog 로그 전용
- AIMD baseline: update only after `stable_cycle_count >= 3`
- LLM correction: gated when `stable_cycle_count < 3`
- Dispatcher empty candidates (no eligible provider) → atomically ZSET-claim and fail job ("no eligible provider for this model"); VRAM-blocked (score_and_claim returns None) → skip job, keep in ZSET
- Provisional VRAM: uses `model_weight_mb()`, fallback 2048 only when 0
