use std::sync::Arc;
use std::time::Duration;

use diesel::pg::PgConnection;
use diesel::r2d2::event::HandleEvent;
use diesel::r2d2::{ConnectionManager, Pool};

use crate::app::metrics::StatsCollector;

pub(crate) type ConnectionPool = Arc<Pool<ConnectionManager<PgConnection>>>;

pub(crate) fn create_pool(
    url: &str,
    size: u32,
    idle_size: Option<u32>,
    timeout: u64,
    max_lifetime: u64,
    enable_stats: bool,
) -> (ConnectionPool, StatsCollector) {
    let manager = ConnectionManager::<PgConnection>::new(url);
    let (collector, transmitter) = StatsCollector::new();

    let builder = Pool::builder()
        .max_size(size)
        .min_idle(idle_size)
        .connection_timeout(Duration::from_secs(timeout))
        .max_lifetime(Some(Duration::from_secs(max_lifetime)));

    let builder = if enable_stats {
        builder.event_handler(Box::new(transmitter) as Box<dyn HandleEvent>)
    } else {
        builder
    };

    let pool = builder
        .build(manager)
        .expect("Error creating a database pool");

    (Arc::new(pool), collector)
}

pub(crate) async fn create_sqlx_pool(url: &str, size: u32, timeout: u64) -> sqlx::postgres::PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(size)
        .connect_timeout(Duration::from_secs(timeout))
        .connect(url)
        .await
        .expect("Failed to create sqlx database pool")
}

pub(crate) mod sql {
    #[derive(Clone, Copy, Debug, DbEnum, PartialEq)]
    #[PgType = "agent_status"]
    #[DieselType = "Agent_status"]
    pub enum AgentStatus {
        InProgress,
        Ready,
    }

    #[derive(DbEnum, Clone, Copy, Debug, PartialEq)]
    #[PgType = "change_type"]
    #[DieselType = "Change_type"]
    pub enum ChangeType {
        Addition,
        Modification,
        Removal,
    }

    pub use svc_agent::sql::{Account_id, Agent_id};
}

pub(crate) mod adjustment;
pub(crate) mod agent;
pub(crate) mod change;
pub(crate) mod edition;
pub(crate) mod event;
pub(crate) mod room;
