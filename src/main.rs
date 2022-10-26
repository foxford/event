#[macro_use]
extern crate anyhow;

use std::env::var;

use anyhow::Result;
use svc_authz::cache::{create_pool, AuthzCache, RedisCache};
use tracing::warn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(feature = "dotenv")]
    dotenv::dotenv()?;

    tracing_log::LogTracer::init()?;

    let (non_blocking, _guard) = tracing_appender::non_blocking(std::io::stdout());
    let subscriber = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .json()
        .flatten_event(true);
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(subscriber);

    tracing::subscriber::set_global_default(subscriber)?;
    warn!(version = %APP_VERSION, "Launching event");

    let (db, maybe_ro_db) = {
        let url = var("DATABASE_URL").expect("DATABASE_URL must be specified");

        let size = var("DATABASE_POOL_SIZE")
            .map(|val| {
                val.parse::<u32>()
                    .expect("Error converting DATABASE_POOL_SIZE variable into u32")
            })
            .unwrap_or(5);

        let idle_size = var("DATABASE_POOL_IDLE_SIZE")
            .map(|val| {
                val.parse::<u32>()
                    .expect("Error converting DATABASE_POOL_IDLE_SIZE variable into u32")
            })
            .ok();

        let timeout = var("DATABASE_POOL_TIMEOUT")
            .map(|val| {
                val.parse::<u64>()
                    .expect("Error converting DATABASE_POOL_TIMEOUT variable into u64")
            })
            .unwrap_or(5);

        let max_lifetime = var("DATABASE_POOL_MAX_LIFETIME")
            .map(|val| {
                val.parse::<u64>()
                    .expect("Error converting DATABASE_POOL_MAX_LIFETIME variable into u64")
            })
            .unwrap_or(1800);

        let db = crate::db::create_pool(&url, size, idle_size, timeout, max_lifetime).await;

        let maybe_ro_db = match var("READONLY_DATABASE_URL") {
            Err(_) => None,
            Ok(ro_url) => {
                Some(crate::db::create_pool(&ro_url, size, idle_size, timeout, max_lifetime).await)
            }
        };

        (db, maybe_ro_db)
    };

    let (redis_pool, authz_cache) = if let Some("1") = var("CACHE_ENABLED").ok().as_deref() {
        let url = var("CACHE_URL").expect("CACHE_URL must be specified");

        let size = var("CACHE_POOL_SIZE")
            .map(|val| {
                val.parse::<u32>()
                    .expect("Error converting CACHE_POOL_SIZE variable into u32")
            })
            .unwrap_or_else(|_| 5);

        let idle_size = var("CACHE_POOL_IDLE_SIZE")
            .map(|val| {
                val.parse::<u32>()
                    .expect("Error converting CACHE_POOL_IDLE_SIZE variable into u32")
            })
            .ok();

        let timeout = var("CACHE_POOL_TIMEOUT")
            .map(|val| {
                val.parse::<u64>()
                    .expect("Error converting CACHE_POOL_TIMEOUT variable into u64")
            })
            .unwrap_or_else(|_| 5);

        let expiration_time = var("CACHE_EXPIRATION_TIME")
            .map(|val| {
                val.parse::<usize>()
                    .expect("Error converting CACHE_EXPIRATION_TIME variable into u64")
            })
            .unwrap_or_else(|_| 300);

        let pool = create_pool(&url, size, idle_size, timeout);
        let cache = Box::new(RedisCache::new(pool.clone(), expiration_time)) as Box<dyn AuthzCache>;
        (Some(pool), Some(cache))
    } else {
        (None, None)
    };

    match var("EVENT_MIGRATE_TO_BINARY") {
        Ok(_) => migration_to_binary_format::migrate_to_binary(db).await,
        Err(_) => match var("EVENT_MIGRATE_TO_JSON") {
            Ok(_) => migration_to_binary_format::migrate_to_json(db).await,
            Err(_) => app::run(db, maybe_ro_db, redis_pool, authz_cache).await,
        },
    }
}

mod app;
mod authz;
mod config;
mod db;
mod metrics;
mod migration_to_binary_format;
#[allow(unused_imports)]
mod serde;
#[cfg(test)]
mod test_helpers;
