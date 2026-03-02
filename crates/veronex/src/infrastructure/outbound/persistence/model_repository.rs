use sqlx::PgPool;

/// PostgreSQL-backed implementation of `ModelRepository`.
///
/// Placeholder — implementation pending.
pub struct PostgresModelRepository {
    #[allow(dead_code)]
    pool: PgPool,
}

impl PostgresModelRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
