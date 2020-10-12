use std::sync::atomic::AtomicI64;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::json;
use slog::{Logger, OwnedKV, SendSyncRefUnwindSafeKV};
use sqlx::postgres::PgPool as Db;
use svc_agent::{queue_counter::QueueCounterHandle, AgentId};
use svc_authz::cache::ConnectionPool as RedisConnectionPool;
use svc_authz::ClientMap as Authz;

use crate::app::context::{Context, GlobalContext, MessageContext};
use crate::app::metrics::Metric;
use crate::app::metrics::ProfilerKeys;
use crate::config::Config;
use crate::profiler::Profiler;

use super::authz::TestAuthz;
use super::db::TestDb;
use super::SVC_AUDIENCE;

///////////////////////////////////////////////////////////////////////////////

fn build_config() -> Config {
    let id = format!("event.{}", SVC_AUDIENCE);
    let broker_id = format!("mqtt-gateway.{}", SVC_AUDIENCE);

    let config = json!({
        "id": id,
        "agent_label": "alpha",
        "broker_id": broker_id,
        "id_token": {
            "algorithm": "ES256",
            "key": "data/keys/svc.private_key.p8.der.sample",
        },
        "authz": {},
        "mqtt": {
            "uri": "mqtt://0.0.0.0:1883",
            "clean_session": false,
        }
    });

    serde_json::from_value::<Config>(config).expect("Failed to parse test config")
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct TestContext {
    config: Config,
    authz: Authz,
    db: TestDb,
    agent_id: AgentId,
    profiler: Arc<Profiler<(ProfilerKeys, Option<String>)>>,
    logger: Logger,
    start_timestamp: DateTime<Utc>,
}

impl TestContext {
    pub(crate) fn new(db: TestDb, authz: TestAuthz) -> Self {
        let config = build_config();
        let agent_id = AgentId::new(&config.agent_label, config.id.clone());

        Self {
            config,
            authz: authz.into(),
            db,
            agent_id,
            profiler: Arc::new(Profiler::<(ProfilerKeys, Option<String>)>::start()),
            logger: crate::LOG.new(o!()),
            start_timestamp: Utc::now(),
        }
    }
}

impl GlobalContext for TestContext {
    fn authz(&self) -> &Authz {
        &self.authz
    }

    fn config(&self) -> &Config {
        &self.config
    }

    fn db(&self) -> &Db {
        self.db.connection_pool()
    }

    fn ro_db(&self) -> &Db {
        self.db.connection_pool()
    }

    fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    fn queue_counter(&self) -> &Option<QueueCounterHandle> {
        &None
    }

    fn redis_pool(&self) -> &Option<RedisConnectionPool> {
        &None
    }

    fn profiler(&self) -> Arc<Profiler<(ProfilerKeys, Option<String>)>> {
        self.profiler.clone()
    }

    fn get_metrics(&self, _duration: u64) -> anyhow::Result<Vec<Metric>> {
        Ok(vec![])
    }

    fn running_requests(&self) -> Option<Arc<AtomicI64>> {
        None
    }
}

impl MessageContext for TestContext {
    fn start_timestamp(&self) -> DateTime<Utc> {
        self.start_timestamp
    }

    fn logger(&self) -> &Logger {
        &self.logger
    }

    fn add_logger_tags<T>(&mut self, tags: OwnedKV<T>)
    where
        T: SendSyncRefUnwindSafeKV + Sized + 'static,
    {
        self.logger = self.logger.new(tags);
    }
}

impl Context for TestContext {}
