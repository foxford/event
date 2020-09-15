use std::sync::atomic::AtomicI64;
use std::sync::Arc;

use async_trait::async_trait;
use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgPool as Db, Postgres};
use sqlx::Error as SqlxError;
use svc_agent::{queue_counter::QueueCounterHandle, AgentId};
use svc_authz::cache::ConnectionPool as RedisConnectionPool;
use svc_authz::ClientMap as Authz;

use crate::app::endpoint::metric::ProfilerKeys;
use crate::config::Config;
use crate::profiler::Profiler;

///////////////////////////////////////////////////////////////////////////////

#[async_trait]
pub(crate) trait Context: Sync {
    fn authz(&self) -> &Authz;
    fn config(&self) -> &Config;
    fn db(&self) -> &Db;
    fn ro_db(&self) -> &Db;
    fn agent_id(&self) -> &AgentId;
    fn queue_counter(&self) -> &Option<QueueCounterHandle>;
    fn redis_pool(&self) -> &Option<RedisConnectionPool>;
    fn profiler(&self) -> Arc<Profiler<ProfilerKeys>>;
    fn running_requests(&self) -> Option<Arc<AtomicI64>>;

    async fn get_conn(&self) -> Result<PoolConnection<Postgres>, SqlxError> {
        self.profiler()
            .measure(ProfilerKeys::DbConnAcquisition, self.db().acquire())
            .await
    }

    async fn get_ro_conn(&self) -> Result<PoolConnection<Postgres>, SqlxError> {
        self.profiler()
            .measure(ProfilerKeys::RoDbConnAcquisition, self.ro_db().acquire())
            .await
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

    fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    fn queue_counter(&self) -> &Option<QueueCounterHandle> {
        &self.queue_counter
    }

    fn redis_pool(&self) -> &Option<RedisConnectionPool> {
        &self.redis_pool
    }

    fn profiler(&self) -> Arc<Profiler<ProfilerKeys>> {
        self.profiler.clone()
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
    agent_id: AgentId,
    queue_counter: Option<QueueCounterHandle>,
    redis_pool: Option<RedisConnectionPool>,
    profiler: Arc<Profiler<ProfilerKeys>>,
    running_requests: Option<Arc<AtomicI64>>,
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct AppContextBuilder {
    config: Config,
    authz: Authz,
    db: Db,
    ro_db: Option<Db>,
    agent_id: AgentId,
    queue_counter: Option<QueueCounterHandle>,
    redis_pool: Option<RedisConnectionPool>,
    running_requests: Option<Arc<AtomicI64>>,
}

impl AppContextBuilder {
    pub(crate) fn new(config: Config, authz: Authz, db: Db) -> Self {
        let agent_id = AgentId::new(&config.agent_label, config.id.to_owned());

        Self {
            config,
            authz,
            db,
            ro_db: None,
            agent_id,
            queue_counter: None,
            redis_pool: None,
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

    pub(crate) fn build(self) -> AppContext {
        AppContext {
            config: Arc::new(self.config),
            authz: self.authz,
            db: self.db,
            ro_db: self.ro_db,
            agent_id: self.agent_id,
            queue_counter: self.queue_counter,
            redis_pool: self.redis_pool,
            profiler: Arc::new(Profiler::<ProfilerKeys>::start()),
            running_requests: self.running_requests,
        }
    }
}
