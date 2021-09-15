use std::sync::atomic::AtomicI64;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use prometheus::Registry;
use serde_json::json;
use slog::{Logger, OwnedKV, SendSyncRefUnwindSafeKV};
use sqlx::postgres::PgPool as Db;
use svc_agent::{queue_counter::QueueCounterHandle, AgentId};
use svc_authz::cache::ConnectionPool as RedisConnectionPool;
use svc_authz::ClientMap as Authz;

use crate::app::s3_client::S3Client;
use crate::config::Config;
use crate::{
    app::context::{Context, GlobalContext, MessageContext},
    metrics::Metrics,
};

use super::authz::{DbBanTestAuthz, TestAuthz};
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
    metrics: Arc<Metrics>,
    logger: Logger,
    start_timestamp: DateTime<Utc>,
    s3_client: Option<S3Client>,
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
            metrics: Arc::new(Metrics::new(&Registry::new()).unwrap()),
            logger: crate::LOG.new(o!()),
            start_timestamp: Utc::now(),
            s3_client: None,
        }
    }

    pub(crate) fn new_with_ban(db: TestDb, authz: DbBanTestAuthz) -> Self {
        let config = build_config();
        let agent_id = AgentId::new(&config.agent_label, config.id.clone());

        Self {
            config,
            authz: authz.into(),
            db,
            agent_id,
            metrics: Arc::new(Metrics::new(&Registry::new()).unwrap()),
            logger: crate::LOG.new(o!()),
            start_timestamp: Utc::now(),
            s3_client: None,
        }
    }

    pub fn set_s3(&mut self, s3_client: S3Client) {
        self.s3_client = Some(s3_client)
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

    fn metrics(&self) -> Arc<Metrics> {
        self.metrics.clone()
    }

    fn running_requests(&self) -> Option<Arc<AtomicI64>> {
        None
    }

    fn s3_client(&self) -> Option<S3Client> {
        self.s3_client.clone()
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
