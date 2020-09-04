use std::env::var;

use diesel::Connection;
use sqlx::postgres::PgPool;

use crate::db::{create_pool, create_sqlx_pool, ConnectionPool};

const TIMEOUT: u64 = 10;

#[derive(Clone)]
pub(crate) struct TestDb {
    connection_pool: ConnectionPool,
    sqlx_connection_pool: PgPool,
}

impl TestDb {
    pub(crate) fn new() -> Self {
        let url = var("DATABASE_URL").expect("DATABASE_URL must be specified");
        let (connection_pool, _) = create_pool(&url, 1, None, TIMEOUT, 1800, false);
        let sqlx_connection_pool = async_std::task::block_on(create_sqlx_pool(&url, 1, TIMEOUT));

        let conn = connection_pool
            .get()
            .expect("Failed to get connection from pool");

        conn.begin_test_transaction()
            .expect("Failed to begin test transaction");

        Self {
            connection_pool,
            sqlx_connection_pool,
        }
    }

    pub(crate) fn connection_pool(&self) -> &ConnectionPool {
        &self.connection_pool
    }

    pub(crate) fn sqlx_connection_pool(&self) -> &PgPool {
        &self.sqlx_connection_pool
    }
}
