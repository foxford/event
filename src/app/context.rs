use std::sync::Arc;

use svc_agent::{queue_counter::QueueCounterHandle, AgentId};
use svc_authz::cache::ConnectionPool as RedisConnectionPool;
use svc_authz::ClientMap as Authz;

use crate::app::endpoint::metric::ProfilerKeys;
use crate::app::metrics::StatsCollector;
use crate::config::Config;
use crate::db::ConnectionPool as Db;
use crate::profiler::Profiler;

///////////////////////////////////////////////////////////////////////////////

pub(crate) trait Context: Sync {
    fn authz(&self) -> &Authz;
    fn config(&self) -> &Config;
    fn db(&self) -> &Db;
    fn ro_db(&self) -> &Db;
    fn agent_id(&self) -> &AgentId;
    fn queue_counter(&self) -> &Option<QueueCounterHandle>;
    fn redis_pool(&self) -> &Option<RedisConnectionPool>;
    fn profiler(&self) -> &Profiler<ProfilerKeys>;
    fn db_pool_stats(&self) -> &Option<StatsCollector>;
    fn ro_db_pool_stats(&self) -> &Option<StatsCollector>;
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

    fn profiler(&self) -> &Profiler<ProfilerKeys> {
        &self.profiler
    }

    fn db_pool_stats(&self) -> &Option<StatsCollector> {
        &self.db_pool_stats
    }

    fn ro_db_pool_stats(&self) -> &Option<StatsCollector> {
        &self.ro_db_pool_stats
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
    db_pool_stats: Option<StatsCollector>,
    ro_db_pool_stats: Option<StatsCollector>,
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct AppContextBuilder {
    config: Config,
    authz: Authz,
    db: Db,
    agent_id: AgentId,
    ro_db: Option<Db>,
    queue_counter: Option<QueueCounterHandle>,
    redis_pool: Option<RedisConnectionPool>,
    db_pool_stats: Option<StatsCollector>,
    ro_db_pool_stats: Option<StatsCollector>,
}

impl AppContextBuilder {
    pub(crate) fn new(config: Config, authz: Authz, db: Db) -> Self {
        let agent_id = AgentId::new(&config.agent_label, config.id.to_owned());

        Self {
            config,
            authz,
            db,
            agent_id,
            ro_db: None,
            queue_counter: None,
            redis_pool: None,
            db_pool_stats: None,
            ro_db_pool_stats: None,
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
            ro_db: self.ro_db,
            agent_id: self.agent_id,
            queue_counter: self.queue_counter,
            redis_pool: self.redis_pool,
            profiler: Arc::new(Profiler::<ProfilerKeys>::start()),
            db_pool_stats: self.db_pool_stats,
            ro_db_pool_stats: self.ro_db_pool_stats,
        }
    }
}
