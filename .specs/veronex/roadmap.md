# Roadmap — Veronex

> L1: Master direction | Load on planning only | **Last Updated**: 2026-03-19

## 2026

| Q | Priority | Feature | Change Type | CDD Reference | Status |
|---|----------|---------|-------------|---------------|--------|
| Q1 | P0 | Intelligence Scheduler | Add | inference/capacity.md | Done |
| Q2 | P1 | Whisper STT Provider | Add | providers/whisper-stt.md | Active |
| Q2 | P1 | Multi-server Scale-Out (real) | Improve | infra/distributed.md | Pending |
| Q2 | P2 | NVIDIA GPU support | Add | providers/hardware.md | Pending |
| Q3 | P2 | Agent OTLP push enhancements | Improve | infra/otel-pipeline.md | Pending |

## Dependencies

| From | To | Reason |
|------|----|--------|
| Intelligence Scheduler | Multi-server Scale-Out | Expand after single-server validation |
| Intelligence Scheduler | NVIDIA support | Thermal profile extension |
| Whisper STT Provider | Multi-server Scale-Out | STT instances join provider registry |
