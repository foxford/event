use std::sync::Arc;
use std::time::Duration;

use crate::app::metrics::StatsCollector;
use diesel::pg::PgConnection;
use diesel::r2d2::event::HandleEvent;
use diesel::r2d2::{ConnectionManager, Pool};

pub(crate) type ConnectionPool = Arc<Pool<ConnectionManager<PgConnection>>>;
pub(crate) fn create_pool(
    url: &str,
    size: u32,
    idle_size: Option<u32>,
    timeout: u64,
    enable_stats: bool,
) -> (ConnectionPool, StatsCollector) {
    let manager = ConnectionManager::<PgConnection>::new(url);
    let (collector, transmitter) = StatsCollector::new();
    let builder = Pool::builder()
        .max_size(size)
        .min_idle(idle_size)
        .connection_timeout(Duration::from_secs(timeout));

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

pub(crate) mod sql {
    pub use super::agent::Agent_status;
    pub use super::change::Change_type;
    pub use svc_agent::sql::{Account_id, Agent_id};
}

pub(crate) mod adjustment;
pub(crate) mod agent;
pub(crate) mod change;
pub(crate) mod edition;
pub(crate) mod event;
pub(crate) mod room;
