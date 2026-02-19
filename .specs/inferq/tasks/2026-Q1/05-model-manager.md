# Task 05: Model Manager (Greedy Allocation + LRU Eviction)

> Ref: best-practices.md → Ollama Integration → Model Loading Strategy
> APU: 96GB unified memory. Strategy: load if fits, evict LRU if needed.

## Steps

### Phase 1 — IModelRepository Port

- [ ] Define `IModelRepository(Protocol)`:

```python
class IModelRepository(Protocol):
    async def get(self, name: ModelName) -> Model | None: ...
    async def upsert(self, model: Model) -> None: ...
    async def list_all(self) -> list[Model]: ...
    async def list_loaded(self) -> list[Model]: ...
    async def update_status(self, name: ModelName, status: ModelStatus) -> None: ...
    async def increment_active_calls(self, name: ModelName) -> None: ...
    async def decrement_active_calls(self, name: ModelName) -> None: ...
```

### Phase 2 — ModelManager Use Case

- [ ] Implement `ModelManager` in `application/use_cases/`:

```python
class ModelManager:
    async def ensure_loaded(self, model_name: ModelName) -> None:
        model = await self.repo.get(model_name)
        if model is None:
            raise ModelNotFoundError(model_name)
        if model.status == ModelStatus.LOADED:
            return  # already in memory, done

        # Check if fits in available memory
        available_mb = await self._get_available_vram_mb()
        if available_mb >= model.vram_mb:
            await self._load(model)
            return

        # Evict LRU (non-active) until enough space
        evicted = await self._evict_for(model.vram_mb)
        if not evicted:
            raise ResourceExhaustedError("All models busy, cannot evict")
        await self._load(model)

    async def _evict_for(self, required_mb: int) -> bool:
        loaded = await self.repo.list_loaded()
        candidates = [m for m in loaded if m.active_calls == 0]
        candidates.sort(key=lambda m: m.last_used_at or datetime.min)  # LRU
        freed = 0
        for candidate in candidates:
            await self.gpu.unload_model(candidate.name)
            await self.repo.update_status(candidate.name, ModelStatus.AVAILABLE)
            freed += candidate.vram_mb
            if freed >= required_mb:
                return True
        return False
```

### Phase 3 — Model Sync

- [ ] `SyncModelsUseCase`: pull model list from Ollama, upsert to PostgreSQL
- [ ] Called at startup + every 60s (background task)

### Phase 4 — Valkey State (loaded models)

- [ ] Mirror loaded model state in Valkey for fast access:
  - `model:loaded:{name}` → `{vram_mb, last_used_at, active_calls}`
  - TTL: none (cleared on unload)

## Verify

- [ ] Start with 2 small models loaded, request 3rd model that fits → all 3 loaded
- [ ] Memory full + 3rd request → LRU evicted, 3rd loaded
- [ ] LRU model has active calls → not evicted, waits

## Done

- [ ] `IModelRepository` protocol defined
- [ ] `ModelManager.ensure_loaded()` implements greedy + LRU
- [ ] Active call counter prevents eviction of busy models
- [ ] Model sync from Ollama on startup
