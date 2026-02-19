# Task 07: PostgreSQL Job State

> Ref: best-practices.md → PostgreSQL section
> SQLAlchemy 2.0 async + Alembic + alembic-postgresql-enum

## Steps

### Phase 1 — DB Models (SQLAlchemy)

- [ ] `InferenceJobRow` (jobs table):

```python
class InferenceJobRow(Base):
    __tablename__ = "jobs"
    id            = Column(UUID(as_uuid=True), primary_key=True)
    prompt        = Column(Text, nullable=False)
    model_name    = Column(String(255), nullable=False)
    status        = Column(Enum(JobStatus), nullable=False, default=JobStatus.PENDING)
    backend       = Column(Enum(BackendType), nullable=False)
    created_at    = Column(DateTime(timezone=True), server_default=func.now())
    started_at    = Column(DateTime(timezone=True), nullable=True)
    completed_at  = Column(DateTime(timezone=True), nullable=True)
    error         = Column(Text, nullable=True)
```

- [ ] `ModelRow` (models table):

```python
class ModelRow(Base):
    __tablename__ = "models"
    name          = Column(String(255), primary_key=True)
    backend       = Column(Enum(BackendType), nullable=False)
    vram_mb       = Column(Integer, nullable=False, default=0)
    status        = Column(Enum(ModelStatus), nullable=False)
    last_used_at  = Column(DateTime(timezone=True), nullable=True)
    synced_at     = Column(DateTime(timezone=True), server_default=func.now())
```

### Phase 2 — Async Engine Setup

- [ ] `src/infrastructure/outbound/persistence/database.py`:

```python
engine = create_async_engine(
    settings.DATABASE_URL,
    pool_size=10,
    max_overflow=20,
    pool_timeout=30,
    pool_recycle=1800,
    pool_pre_ping=True,
)
async_session = async_sessionmaker(engine, class_=AsyncSession, expire_on_commit=False)
```

### Phase 3 — Repository Adapters

- [ ] `PostgresJobRepository(IJobRepository)`
- [ ] `PostgresModelRepository(IModelRepository)`

### Phase 4 — Alembic Setup

- [ ] `alembic init alembic`
- [ ] Configure `alembic.ini` with async URL
- [ ] Add `alembic-postgresql-enum` to `env.py`
- [ ] Create initial migration

**CAUTION:** Enum migrations require autocommit block:
```python
# In migration file
def upgrade():
    op.execute("ALTER TYPE jobstatus ADD VALUE 'CANCELLED'")  # outside transaction
```

## Verify

```bash
alembic upgrade head
python -c "from src.infrastructure.outbound.persistence import PostgresJobRepository"
```

## Done

- [ ] `jobs` and `models` tables created via migration
- [ ] `IJobRepository` + `IModelRepository` adapters implemented
- [ ] `pool_pre_ping=True` prevents stale connection errors
- [ ] Enum migration pattern documented
