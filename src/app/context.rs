use std::sync::atomic::AtomicI64;
use std::sync::Arc;

use async_std::task::spawn_blocking;
use async_trait::async_trait;
use diesel::{
    r2d2::{ConnectionManager, PoolError, PooledConnection},
    PgConnection,
};
use sqlx::postgres::PgPool as SqlxDb;
use svc_agent::{queue_counter::QueueCounterHandle, AgentId};
use svc_authz::cache::ConnectionPool as RedisConnectionPool;
use svc_authz::ClientMap as Authz;

use crate::app::endpoint::metric::ProfilerKeys;
use crate::app::metrics::StatsCollector;
use crate::config::Config;
use crate::db::ConnectionPool as Db;
use crate::profiler::Profiler;

///////////////////////////////////////////////////////////////////////////////

#[async_trait]
pub(crate) trait Context: Sync {
    fn authz(&self) -> &Authz;
    fn config(&self) -> &Config;
    fn db(&self) -> &Db;
    fn ro_db(&self) -> &Db;
    fn sqlx_db(&self) -> &SqlxDb;
    fn agent_id(&self) -> &AgentId;
    fn queue_counter(&self) -> &Option<QueueCounterHandle>;
    fn redis_pool(&self) -> &Option<RedisConnectionPool>;
    fn profiler(&self) -> &Profiler<ProfilerKeys>;
    fn db_pool_stats(&self) -> &Option<StatsCollector>;
    fn ro_db_pool_stats(&self) -> &Option<StatsCollector>;
    fn running_requests(&self) -> Option<Arc<AtomicI64>>;

    async fn get_conn(
        &self,
    ) -> Result<PooledConnection<ConnectionManager<PgConnection>>, PoolError> {
        let db = self.db().clone();
        spawn_blocking(move || db.get()).await
    }

    async fn get_ro_conn(
        &self,
    ) -> Result<PooledConnection<ConnectionManager<PgConnection>>, PoolError> {
        let db = self.ro_db().clone();
        spawn_blocking(move || db.get()).await
    }
}

impl Context for AppContext {
    fn authz(&self) -> &Authz {
        &self.authz
    }

    fn config(&self) -> &Config {
        &self.config
    }

    fn db(&self) -> &Db {
        &self.db
    }

    fn ro_db(&self) -> &Db {
        self.ro_db.as_ref().unwrap_or(&self.db)
    }

    fn sqlx_db(&self) -> &SqlxDb {
        &self.sqlx_db
    }

    fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    fn queue_counter(&self) -> &Option<QueueCounterHandle> {
        &self.queue_counter
    }

    fn redis_pool(&self) -> &Option<RedisConnectionPool> {
        &self.redis_pool
    }

    fn profiler(&self) -> &Profiler<ProfilerKeys> {
        &self.profiler
    }

    fn db_pool_stats(&self) -> &Option<StatsCollector> {
        &self.db_pool_stats
    }

    fn ro_db_pool_stats(&self) -> &Option<StatsCollector> {
        &self.ro_db_pool_stats
    }

    fn running_requests(&self) -> Option<Arc<AtomicI64>> {
        self.running_requests.clone()
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Clone)]
pub(crate) struct AppContext {
    config: Arc<Config>,
    authz: Authz,
    db: Db,
    ro_db: Option<Db>,
    sqlx_db: SqlxDb,
    agent_id: AgentId,
    queue_counter: Option<QueueCounterHandle>,
    redis_pool: Option<RedisConnectionPool>,
    profiler: Arc<Profiler<ProfilerKeys>>,
    db_pool_stats: Option<StatsCollector>,
    ro_db_pool_stats: Option<StatsCollector>,
    running_requests: Option<Arc<AtomicI64>>,
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct AppContextBuilder {
    config: Config,
    authz: Authz,
    db: Db,
    agent_id: AgentId,
    ro_db: Option<Db>,
    sqlx_db: SqlxDb,
    queue_counter: Option<QueueCounterHandle>,
    redis_pool: Option<RedisConnectionPool>,
    db_pool_stats: Option<StatsCollector>,
    ro_db_pool_stats: Option<StatsCollector>,
    running_requests: Option<Arc<AtomicI64>>,
}

impl AppContextBuilder {
    pub(crate) fn new(config: Config, authz: Authz, db: Db, sqlx_db: SqlxDb) -> Self {
        let agent_id = AgentId::new(&config.agent_label, config.id.to_owned());

        Self {
            config,
            authz,
            db,
            agent_id,
            ro_db: None,
            sqlx_db,
            queue_counter: None,
            redis_pool: None,
            db_pool_stats: None,
            ro_db_pool_stats: None,
            running_requests: None,
        }
    }

    pub(crate) fn ro_db(self, ro_db: Db) -> Self {
        Self {
            ro_db: Some(ro_db),
            ..self
        }
    }

    pub(crate) fn queue_counter(self, qc: QueueCounterHandle) -> Self {
        Self {
            queue_counter: Some(qc),
            ..self
        }
    }

    pub(crate) fn running_requests(self, counter: Arc<AtomicI64>) -> Self {
        Self {
            running_requests: Some(counter),
            ..self
        }
    }

    pub(crate) fn redis_pool(self, pool: RedisConnectionPool) -> Self {
        Self {
            redis_pool: Some(pool),
            ..self
        }
    }

    pub(crate) fn db_pool_stats(self, stats: StatsCollector) -> Self {
        Self {
            db_pool_stats: Some(stats),
            ..self
        }
    }

    pub(crate) fn ro_db_pool_stats(self, stats: StatsCollector) -> Self {
        Self {
            ro_db_pool_stats: Some(stats),
            ..self
        }
    }

    pub(crate) fn build(self) -> AppContext {
        AppContext {
            config: Arc::new(self.config),
            authz: self.authz,
            db: self.db,
            sqlx_db: self.sqlx_db,
            ro_db: self.ro_db,
            agent_id: self.agent_id,
            queue_counter: self.queue_counter,
            redis_pool: self.redis_pool,
            profiler: Arc::new(Profiler::<ProfilerKeys>::start()),
            db_pool_stats: self.db_pool_stats,
            ro_db_pool_stats: self.ro_db_pool_stats,
            running_requests: self.running_requests,
        }
    }
}
