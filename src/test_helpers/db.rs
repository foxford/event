use std::env::var;

use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgPool, PgPoolOptions, Postgres};

pub(crate) struct TestDb {
    pool: PgPool,
}

impl TestDb {
    pub(crate) async fn new() -> Self {
        let url = var("DATABASE_URL").expect("DATABASE_URL must be specified");

        let pool = PgPoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect(&url)
            .await
            .expect("Failed to connect to the DB");

        Self { pool }
    }

    pub(crate) fn connection_pool(&self) -> &PgPool {
        &self.pool
    }

    pub(crate) async fn get_conn(&self) -> PoolConnection<Postgres> {
        self.pool
            .acquire()
            .await
            .expect("Failed to get DB connection")
    }
}
