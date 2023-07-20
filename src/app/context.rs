use std::sync::Arc;

use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgPool as Db, Postgres};
use svc_agent::{queue_counter::QueueCounterHandle, AgentId};
use svc_authz::cache::ConnectionPool as RedisConnectionPool;
use svc_nats_client::NatsClient;

use crate::config::Config;
use crate::{
    app::error::{Error as AppError, ErrorExt, ErrorKind as AppErrorKind},
    metrics::Metrics,
};
use crate::{app::s3_client::S3Client, authz::Authz};

use super::broker_client::BrokerClient;

///////////////////////////////////////////////////////////////////////////////

pub trait Context: GlobalContext + MessageContext {}

#[async_trait]
pub trait GlobalContext {
    fn authz(&self) -> &Authz;
    fn config(&self) -> &Config;
    fn db(&self) -> &Db;
    fn ro_db(&self) -> &Db;
    fn agent_id(&self) -> &AgentId;
    fn queue_counter(&self) -> &Option<QueueCounterHandle>;
    fn redis_pool(&self) -> &Option<RedisConnectionPool>;
    fn metrics(&self) -> Arc<Metrics>;
    fn s3_client(&self) -> Option<S3Client>;
    fn broker_client(&self) -> &dyn BrokerClient;
    fn nats_client(&self) -> Option<Arc<dyn NatsClient>>;

    async fn get_conn(&self) -> Result<PoolConnection<Postgres>, AppError> {
        self.db()
            .acquire()
            .await
            .context("Failed to acquire DB connection")
            .error(AppErrorKind::DbConnAcquisitionFailed)
    }

    async fn get_ro_conn(&self) -> Result<PoolConnection<Postgres>, AppError> {
        self.ro_db()
            .acquire()
            .await
            .context("Failed to acquire read-only DB connection")
            .error(AppErrorKind::DbConnAcquisitionFailed)
    }
}

pub trait MessageContext {
    fn start_timestamp(&self) -> DateTime<Utc>;
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Clone)]
pub struct AppContext {
    config: Arc<Config>,
    authz: Authz,
    db: Db,
    ro_db: Option<Db>,
    agent_id: AgentId,
    queue_counter: Option<QueueCounterHandle>,
    redis_pool: Option<RedisConnectionPool>,
    metrics: Arc<Metrics>,
    s3_client: Option<S3Client>,
    broker_client: Arc<dyn BrokerClient>,
    nats_client: Option<Arc<dyn NatsClient>>,
}

impl AppContext {
    pub fn start_message(&self) -> AppMessageContext<'_, Self> {
        AppMessageContext::new(self, Utc::now())
    }
}

impl GlobalContext for AppContext {
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

    fn metrics(&self) -> Arc<Metrics> {
        self.metrics.clone()
    }

    fn s3_client(&self) -> Option<S3Client> {
        self.s3_client.clone()
    }

    fn broker_client(&self) -> &dyn BrokerClient {
        self.broker_client.as_ref()
    }

    fn nats_client(&self) -> Option<Arc<dyn NatsClient>> {
        self.nats_client.clone()
    }
}

///////////////////////////////////////////////////////////////////////////////

pub struct AppMessageContext<'a, C: GlobalContext> {
    global_context: &'a C,
    start_timestamp: DateTime<Utc>,
}

impl<'a, C: GlobalContext> AppMessageContext<'a, C> {
    pub fn new(global_context: &'a C, start_timestamp: DateTime<Utc>) -> Self {
        Self {
            global_context,
            start_timestamp,
        }
    }
}

impl<'a, C: GlobalContext> GlobalContext for AppMessageContext<'a, C> {
    fn authz(&self) -> &Authz {
        self.global_context.authz()
    }

    fn config(&self) -> &Config {
        self.global_context.config()
    }

    fn db(&self) -> &Db {
        self.global_context.db()
    }

    fn ro_db(&self) -> &Db {
        self.global_context.ro_db()
    }

    fn agent_id(&self) -> &AgentId {
        self.global_context.agent_id()
    }

    fn queue_counter(&self) -> &Option<QueueCounterHandle> {
        self.global_context.queue_counter()
    }

    fn redis_pool(&self) -> &Option<RedisConnectionPool> {
        self.global_context.redis_pool()
    }

    fn metrics(&self) -> Arc<Metrics> {
        self.global_context.metrics()
    }

    fn s3_client(&self) -> Option<S3Client> {
        self.global_context.s3_client()
    }

    fn broker_client(&self) -> &dyn BrokerClient {
        self.global_context.broker_client()
    }

    fn nats_client(&self) -> Option<Arc<dyn NatsClient>> {
        self.global_context.nats_client()
    }
}

impl<'a, C: GlobalContext> MessageContext for AppMessageContext<'a, C> {
    fn start_timestamp(&self) -> DateTime<Utc> {
        self.start_timestamp
    }
}

impl<'a, C: GlobalContext> Context for AppMessageContext<'a, C> {}

///////////////////////////////////////////////////////////////////////////////

pub struct AppContextBuilder {
    config: Config,
    authz: Authz,
    db: Db,
    broker_client: Arc<dyn BrokerClient>,
    ro_db: Option<Db>,
    agent_id: AgentId,
    queue_counter: Option<QueueCounterHandle>,
    redis_pool: Option<RedisConnectionPool>,
    nats_client: Option<Arc<dyn NatsClient>>,
}

impl AppContextBuilder {
    pub fn new(config: Config, authz: Authz, db: Db, broker_client: Arc<dyn BrokerClient>) -> Self {
        let agent_id = AgentId::new(&config.agent_label, config.id.to_owned());

        Self {
            config,
            authz,
            db,
            broker_client,
            ro_db: None,
            agent_id,
            queue_counter: None,
            redis_pool: None,
            nats_client: None,
        }
    }

    pub fn ro_db(self, ro_db: Db) -> Self {
        Self {
            ro_db: Some(ro_db),
            ..self
        }
    }

    pub fn queue_counter(self, qc: QueueCounterHandle) -> Self {
        Self {
            queue_counter: Some(qc),
            ..self
        }
    }

    pub fn redis_pool(self, pool: RedisConnectionPool) -> Self {
        Self {
            redis_pool: Some(pool),
            ..self
        }
    }

    pub fn add_nats_client(self, nats_client: impl NatsClient + 'static) -> Self {
        Self {
            nats_client: Some(Arc::new(nats_client)),
            ..self
        }
    }

    pub fn build(self, metrics: Arc<Metrics>) -> AppContext {
        AppContext {
            config: Arc::new(self.config),
            authz: self.authz,
            db: self.db,
            ro_db: self.ro_db,
            broker_client: self.broker_client,
            agent_id: self.agent_id,
            queue_counter: self.queue_counter,
            redis_pool: self.redis_pool,
            metrics,
            s3_client: S3Client::new(),
            nats_client: self.nats_client,
        }
    }
}
