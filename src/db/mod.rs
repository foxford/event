use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};

pub async fn create_pool(
    url: &str,
    size: u32,
    idle_size: Option<u32>,
    timeout: u64,
    max_lifetime: u64,
) -> PgPool {
    PgPoolOptions::new()
        .max_connections(size)
        .min_connections(idle_size.unwrap_or(1))
        .acquire_timeout(Duration::from_secs(timeout))
        .max_lifetime(Duration::from_secs(max_lifetime))
        .connect(url)
        .await
        .expect("Failed to create sqlx database pool")
}

pub mod adjustment;
pub mod agent;
pub mod change;
pub mod edition;
pub mod event;
pub mod room;
pub mod room_ban;
pub mod room_time;
