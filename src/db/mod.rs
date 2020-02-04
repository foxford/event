use std::sync::Arc;
use std::time::Duration;

use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool};

pub(crate) type ConnectionPool = Arc<Pool<ConnectionManager<PgConnection>>>;

pub(crate) fn create_pool(url: &str, size: u32, timeout: u64) -> ConnectionPool {
    let manager = ConnectionManager::<PgConnection>::new(url);

    let pool = Pool::builder()
        .max_size(size)
        .connection_timeout(Duration::from_secs(timeout))
        .build(manager)
        .expect("Error creating a database pool");

    Arc::new(pool)
}

pub(crate) mod sql {
    pub use svc_agent::sql::{Account_id, Agent_id};
}

pub(crate) mod adjustment;
pub(crate) mod room;
