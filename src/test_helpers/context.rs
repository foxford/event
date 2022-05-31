use std::sync::Arc;

use chrono::{DateTime, Utc};
use prometheus::Registry;
use serde_json::json;
use sqlx::postgres::PgPool as Db;
use svc_agent::{queue_counter::QueueCounterHandle, AgentId};
use svc_authz::cache::ConnectionPool as RedisConnectionPool;

use crate::app::broker_client::{BrokerClient, MockBrokerClient};
use crate::app::notification_puller::PullerHandle;
use crate::config::Config;
use crate::{
    app::context::{Context, GlobalContext, MessageContext},
    metrics::Metrics,
};
use crate::{app::s3_client::S3Client, authz::Authz};

use super::authz::{DbBanTestAuthz, TestAuthz};
use super::db::TestDb;
use super::SVC_AUDIENCE;

///////////////////////////////////////////////////////////////////////////////

fn build_config(payload_size: Option<usize>) -> Config {
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
        "http_addr": "0.0.0.0:8080",
        "constraint": {
            "payload_size": payload_size.unwrap_or(102400),
        },
        "authn": {},
        "authz": {},
        "mqtt": {
            "uri": "mqtt://0.0.0.0:1883",
            "clean_session": false,
        },
        "nats": {
            "namespace": "unittest"
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
    start_timestamp: DateTime<Utc>,
    s3_client: Option<S3Client>,
    broker_client: Arc<MockBrokerClient>,
}

impl TestContext {
    pub(crate) fn new(db: TestDb, authz: TestAuthz) -> Self {
        let config = build_config(None);
        let agent_id = AgentId::new(&config.agent_label, config.id.clone());

        let metrics = Arc::new(Metrics::new(&Registry::new()).unwrap());
        Self {
            config,
            authz: Authz::new(authz.into(), metrics.clone()),
            db,
            agent_id,
            metrics,
            start_timestamp: Utc::now(),
            s3_client: None,
            broker_client: Arc::new(MockBrokerClient::new()),
        }
    }

    pub(crate) fn new_with_data_size(db: TestDb, authz: TestAuthz, payload_size: usize) -> Self {
        let config = build_config(Some(payload_size));
        let agent_id = AgentId::new(&config.agent_label, config.id.clone());

        let metrics = Arc::new(Metrics::new(&Registry::new()).unwrap());
        Self {
            config,
            authz: Authz::new(authz.into(), metrics.clone()),
            db,
            agent_id,
            metrics,
            start_timestamp: Utc::now(),
            s3_client: None,
            broker_client: Arc::new(MockBrokerClient::new()),
        }
    }

    pub(crate) fn new_with_ban(db: TestDb, authz: DbBanTestAuthz) -> Self {
        let config = build_config(None);
        let agent_id = AgentId::new(&config.agent_label, config.id.clone());

        let metrics = Arc::new(Metrics::new(&Registry::new()).unwrap());
        Self {
            config,
            authz: Authz::new(authz.into(), metrics.clone()),
            db,
            agent_id,
            metrics,
            start_timestamp: Utc::now(),
            s3_client: None,
            broker_client: Arc::new(MockBrokerClient::new()),
        }
    }

    pub fn set_s3(&mut self, s3_client: S3Client) {
        self.s3_client = Some(s3_client)
    }

    pub fn broker_client_mock(&mut self) -> &mut MockBrokerClient {
        Arc::get_mut(&mut self.broker_client).expect("Failed to get broker client mock")
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

    fn s3_client(&self) -> Option<S3Client> {
        self.s3_client.clone()
    }

    fn broker_client(&self) -> &dyn BrokerClient {
        self.broker_client.as_ref()
    }

    fn puller_handle(&self) -> Option<&PullerHandle> {
        None
    }
}

impl MessageContext for TestContext {
    fn start_timestamp(&self) -> DateTime<Utc> {
        self.start_timestamp
    }
}

impl Context for TestContext {}
