#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate slog;

use std::env::var;

use anyhow::Result;
use slog::Drain;
use svc_authz::cache::{create_pool, AuthzCache, RedisCache};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

lazy_static! {
    static ref LOG: slog::Logger = {
        let drain = slog_json::Json::default(std::io::stdout()).fuse();
        let drain = slog_envlogger::new(drain).fuse();
        let drain = slog_async::Async::new(drain).build().fuse();
        slog::Logger::root(drain, o!("version" => APP_VERSION))
    };
}

#[tokio::main]
async fn main() -> Result<()> {
    warn!(crate::LOG, "Launching event, version: {}", APP_VERSION);

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

    app::run(db, maybe_ro_db, redis_pool, authz_cache).await
}

mod app;
mod authz;
mod config;
mod db;
mod metrics;
#[allow(unused_imports)]
mod serde;
#[cfg(test)]
mod test_helpers;
