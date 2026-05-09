# Thermal Protection Flow

> **Last Updated**: 2026-03-26
> Target hardware: AMD Ryzen AI 395+ APU (Vulkan/iGPU, CPU thermal profile)

---

## State Machine

```
              temp < normal_below
    ┌─────────────────────────────────────┐
    │                                     │
    ▼           temp rises                │
  Normal ──────────────────────► Soft    │
  full concurrency          new requests  │
                            blocked (503) │
                                  │       │
                            temp rises    │
                                  ▼       │
                               Hard       │
                         all blocked +    │
                         forced drain     │
                         after 60s        │
                                  │       │
                         active == 0      │
                                  ▼       │
                            Cooldown      │
                         no new requests  │
                         wait cooldown_s  │
                                  │       │
                         timeout elapsed  │
                                  ▼       │
                            RampUp ───────┘
                         max_concurrent=1
                         gradual restore
```

---

## Threshold Triggers

```
GPU vendor:
  nvidia → monitor GPU temp (node-exporter gpu_metrics)
  amd    → monitor CPU temp (APU = CPU-integrated GPU, CPU thermal dominates)
           auto-detected from veronex-agent gpu_vendor label

Per-provider thresholds (configurable):
  normal_below   < 75°C  — full concurrency
  soft_at        ≥ 75°C  — new requests blocked (503)
  hard_at        ≥ 82°C  — all blocked, drain triggered
  (cooldown threshold)   ≥ 90°C edge — stay in cooldown until temp drops
```

---

## Soft State

```
thermal.get_level(provider_id) == Soft

Effect at dispatch (select_provider):
  └── provider skipped for new requests
      active in-flight jobs continue (not cancelled)

Effect on perf_factor:
  └── score multiplied by perf_factor < 1.0
      (reduces effective queue priority for hot servers)
```

---

## Hard State + Forced Drain

```
thermal.get_level(provider_id) == Hard

Immediate effect:
  └── all new requests blocked
      active in-flight jobs continue (not immediately cancelled)

Hard → Cooldown watchdog: currently NOT wired in the post-Ollama
process_manager build. `ThermalPort::set_cooldown` and
`ThermalDrainPort::cancel_jobs_for_provider` still exist on the trait
+ adapters; the legacy 5s placement_planner that drove them was deleted
along with the rest of the Ollama-era preload/evict path. Re-wiring
into the IdleManager tick (or a dedicated thermal-watchdog task) is
tracked under Phase 4 follow-up.

Until then, Hard state is sticky: providers stuck in Hard need either
`ProcessManager.stop()` (admin) or temperature drop below
`hysteresis_below`, which the live thermal monitor already handles for
the hardware-recovery path.
```

---

## Cooldown State

```
thermal.get_level(provider_id) == Cooldown

  no new requests dispatched
  wait cooldown_secs (hardware cooling)

  after cooldown_secs elapsed:
    → transition to RampUp
```

---

## RampUp State

```
thermal.get_level(provider_id) == RampUp

  max_concurrent = 1   (single request at a time)
  new requests allowed

  if temp stays below normal_below for stabilization period:
    → transition back to Normal (full concurrency restored)

  if temp rises again during ramp-up:
    → re-enter Soft or Hard
```

---

## Queue Scoring During Thermal Events

```
score = age_ms + tier_bonus - thermal_penalty

thermal.perf_factor(provider_id):
  Normal  → 1.0  (no penalty)
  RampUp  → 0.8
  Soft    → 0.5
  Hard    → 0.0  (provider excluded entirely at dispatch)
  Cooldown→ 0.0

thermal.global_perf_factor():
  min(perf_factor across all providers)
  used for conservative queue window sizing
```

---

## Files

| File | Purpose |
|------|---------|
| `domain/enums.rs` | `ThrottleLevel` enum definition |
| `application/ports/outbound/thermal_port.rs` | `ThermalPort` trait |
| `application/ports/outbound/thermal_drain_port.rs` | `ThermalDrainPort` trait |
| `application/use_cases/inference/use_case.rs` | `ThermalDrainAdapter` impl |
| `infrastructure/outbound/capacity/thermal.rs` | Thermal monitor, threshold config (Hard→Cooldown watchdog: see note above) |
