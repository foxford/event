use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgPool as Db, Postgres};
use sqlx::Error as SqlxError;
use svc_agent::{queue_counter::QueueCounterHandle, AgentId};
use svc_authz::cache::ConnectionPool as RedisConnectionPool;
use svc_authz::ClientMap as Authz;

use crate::app::endpoint::metric::ProfilerKeys;
use crate::app::metrics::{Metric, MetricValue};
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
    fn get_metrics(&self, duration: u64) -> Result<Vec<crate::app::metrics::Metric>, String>;

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

    fn get_metrics(&self, duration: u64) -> Result<Vec<crate::app::metrics::Metric>, String> {
        let now = Utc::now();
        let mut metrics = vec![];

        append_mqtt_stats(&mut metrics, self, now, duration)?;
        append_db_pools_stats(&mut metrics, self, now);
        append_redis_pool_metrics(&mut metrics, self, now);

        append_profiler_stats(&mut metrics, self, now, duration)?;

        if let Some(counter) = self.running_requests.clone() {
            metrics.push(Metric::RunningRequests(MetricValue::new(
                counter.load(Ordering::SeqCst),
                now,
            )));
        }

        Ok(metrics)
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

fn append_mqtt_stats(
    metrics: &mut Vec<Metric>,
    context: &dyn Context,
    now: DateTime<Utc>,
    duration: u64,
) -> std::result::Result<(), String> {
    if let Some(qc) = context.queue_counter() {
        let stats = qc.get_stats(duration)?;

        let m = [
            Metric::IncomingQueueRequests(MetricValue::new(stats.incoming_requests, now)),
            Metric::IncomingQueueResponses(MetricValue::new(stats.incoming_responses, now)),
            Metric::IncomingQueueEvents(MetricValue::new(stats.incoming_events, now)),
            Metric::OutgoingQueueRequests(MetricValue::new(stats.outgoing_requests, now)),
            Metric::OutgoingQueueResponses(MetricValue::new(stats.outgoing_responses, now)),
            Metric::OutgoingQueueEvents(MetricValue::new(stats.outgoing_events, now)),
        ];

        metrics.extend_from_slice(&m);
    }

    Ok(())
}

fn append_db_pools_stats(metrics: &mut Vec<Metric>, context: &dyn Context, now: DateTime<Utc>) {
    metrics.extend_from_slice(&[
        Metric::DbConnections(MetricValue::new(context.db().size() as u64, now)),
        Metric::IdleDbConnections(MetricValue::new(context.db().num_idle() as u64, now)),
        Metric::RoDbConnections(MetricValue::new(context.ro_db().size() as u64, now)),
        Metric::IdleRoDbConnections(MetricValue::new(context.ro_db().num_idle() as u64, now)),
    ])
}

fn append_redis_pool_metrics(metrics: &mut Vec<Metric>, context: &dyn Context, now: DateTime<Utc>) {
    if let Some(pool) = context.redis_pool() {
        let state = pool.state();

        metrics.extend_from_slice(&[
            Metric::RedisConnections(MetricValue::new(state.connections as u64, now)),
            Metric::IdleRedisConnections(MetricValue::new(state.idle_connections as u64, now)),
        ]);
    }
}

fn append_profiler_stats(
    metrics: &mut Vec<Metric>,
    context: &dyn Context,
    now: DateTime<Utc>,
    duration: u64,
) -> std::result::Result<(), String> {
    let profiler_report = context
        .profiler()
        .flush(duration)
        .map_err(|err| err.to_string())?;

    for (key, entry_report) in profiler_report {
        let metric_value_count = MetricValue::new(entry_report.count as u64, now);
        let metric_value_p95 = MetricValue::new(entry_report.p95 as u64, now);
        let metric_value_p99 = MetricValue::new(entry_report.p99 as u64, now);
        let metric_value_max = MetricValue::new(entry_report.max as u64, now);

        match key {
            ProfilerKeys::DbConnAcquisition => {
                metrics.push(Metric::DbConnAcquisitionCount(metric_value_count));
                metrics.push(Metric::DbConnAcquisitionP95(metric_value_p95));
                metrics.push(Metric::DbConnAcquisitionP99(metric_value_p99));
                metrics.push(Metric::DbConnAcquisitionMax(metric_value_max));
            }
            ProfilerKeys::RoDbConnAcquisition => {
                metrics.push(Metric::RoDbConnAcquisitionCount(metric_value_count));
                metrics.push(Metric::RoDbConnAcquisitionP95(metric_value_p95));
                metrics.push(Metric::RoDbConnAcquisitionP99(metric_value_p99));
                metrics.push(Metric::RoDbConnAcquisitionMax(metric_value_max));
            }
            ProfilerKeys::AdjustmentInsertQuery => {
                metrics.push(Metric::AdjustmentInsertQueryP95(metric_value_p95));
                metrics.push(Metric::AdjustmentInsertQueryP99(metric_value_p99));
                metrics.push(Metric::AdjustmentInsertQueryMax(metric_value_max));
            }
            ProfilerKeys::AgentDeleteQuery => {
                metrics.push(Metric::AgentDeleteQueryP95(metric_value_p95));
                metrics.push(Metric::AgentDeleteQueryP99(metric_value_p99));
                metrics.push(Metric::AgentDeleteQueryMax(metric_value_max));
            }
            ProfilerKeys::AgentInsertQuery => {
                metrics.push(Metric::AgentInsertQueryP95(metric_value_p95));
                metrics.push(Metric::AgentInsertQueryP99(metric_value_p99));
                metrics.push(Metric::AgentInsertQueryMax(metric_value_max));
            }
            ProfilerKeys::AgentListQuery => {
                metrics.push(Metric::AgentListQueryP95(metric_value_p95));
                metrics.push(Metric::AgentListQueryP99(metric_value_p99));
                metrics.push(Metric::AgentListQueryMax(metric_value_max));
            }
            ProfilerKeys::AgentUpdateQuery => {
                metrics.push(Metric::AgentUpdateQueryP95(metric_value_p95));
                metrics.push(Metric::AgentUpdateQueryP99(metric_value_p99));
                metrics.push(Metric::AgentUpdateQueryMax(metric_value_max));
            }
            ProfilerKeys::ChangeDeleteQuery => {
                metrics.push(Metric::ChangeDeleteQueryP95(metric_value_p95));
                metrics.push(Metric::ChangeDeleteQueryP99(metric_value_p99));
                metrics.push(Metric::ChangeDeleteQueryMax(metric_value_max));
            }
            ProfilerKeys::ChangeFindWithRoomQuery => {
                metrics.push(Metric::ChangeFindWithRoomQueryP95(metric_value_p95));
                metrics.push(Metric::ChangeFindWithRoomQueryP99(metric_value_p99));
                metrics.push(Metric::ChangeFindWithRoomQueryMax(metric_value_max));
            }
            ProfilerKeys::ChangeInsertQuery => {
                metrics.push(Metric::ChangeInsertQueryP95(metric_value_p95));
                metrics.push(Metric::ChangeInsertQueryP99(metric_value_p99));
                metrics.push(Metric::ChangeInsertQueryMax(metric_value_max));
            }
            ProfilerKeys::ChangeListQuery => {
                metrics.push(Metric::ChangeListQueryP95(metric_value_p95));
                metrics.push(Metric::ChangeListQueryP99(metric_value_p99));
                metrics.push(Metric::ChangeListQueryMax(metric_value_max));
            }
            ProfilerKeys::EditionCloneEventsQuery => {
                metrics.push(Metric::EditionCloneEventsQueryP95(metric_value_p95));
                metrics.push(Metric::EditionCloneEventsQueryP99(metric_value_p99));
                metrics.push(Metric::EditionCloneEventsQueryMax(metric_value_max));
            }
            ProfilerKeys::EditionCommitTxnCommit => {
                metrics.push(Metric::EditionCommitTxnCommitP95(metric_value_p95));
                metrics.push(Metric::EditionCommitTxnCommitP99(metric_value_p99));
                metrics.push(Metric::EditionCommitTxnCommitMax(metric_value_max));
            }
            ProfilerKeys::EditionDeleteQuery => {
                metrics.push(Metric::EditionDeleteQueryP95(metric_value_p95));
                metrics.push(Metric::EditionDeleteQueryP99(metric_value_p99));
                metrics.push(Metric::EditionDeleteQueryMax(metric_value_max));
            }
            ProfilerKeys::EditionFindWithRoomQuery => {
                metrics.push(Metric::EditionFindWithRoomQueryP95(metric_value_p95));
                metrics.push(Metric::EditionFindWithRoomQueryP99(metric_value_p99));
                metrics.push(Metric::EditionFindWithRoomQueryMax(metric_value_max));
            }
            ProfilerKeys::EditionInsertQuery => {
                metrics.push(Metric::EditionInsertQueryP95(metric_value_p95));
                metrics.push(Metric::EditionInsertQueryP99(metric_value_p99));
                metrics.push(Metric::EditionInsertQueryMax(metric_value_max));
            }
            ProfilerKeys::EditionListQuery => {
                metrics.push(Metric::EditionListQueryP95(metric_value_p95));
                metrics.push(Metric::EditionListQueryP99(metric_value_p99));
                metrics.push(Metric::EditionListQueryMax(metric_value_max));
            }
            ProfilerKeys::EventDeleteQuery => {
                metrics.push(Metric::EventDeleteQueryP95(metric_value_p95));
                metrics.push(Metric::EventDeleteQueryP99(metric_value_p99));
                metrics.push(Metric::EventDeleteQueryMax(metric_value_max));
            }
            ProfilerKeys::EventInsertQuery => {
                metrics.push(Metric::EventInsertQueryP95(metric_value_p95));
                metrics.push(Metric::EventInsertQueryP99(metric_value_p99));
                metrics.push(Metric::EventInsertQueryMax(metric_value_max));
            }
            ProfilerKeys::EventListQuery => {
                metrics.push(Metric::EventListQueryP95(metric_value_p95));
                metrics.push(Metric::EventListQueryP99(metric_value_p99));
                metrics.push(Metric::EventListQueryMax(metric_value_max));
            }
            ProfilerKeys::EventOriginalEventQuery => {
                metrics.push(Metric::EventOriginalQueryP95(metric_value_p95));
                metrics.push(Metric::EventOriginalQueryP99(metric_value_p99));
                metrics.push(Metric::EventOriginalQueryMax(metric_value_max));
            }
            ProfilerKeys::RoomAdjustCloneEventsQuery => {
                metrics.push(Metric::RoomAdjustCloneEventsQueryP95(metric_value_p95));
                metrics.push(Metric::RoomAdjustCloneEventsQueryP99(metric_value_p99));
                metrics.push(Metric::RoomAdjustCloneEventsQueryMax(metric_value_max));
            }
            ProfilerKeys::RoomFindQuery => {
                metrics.push(Metric::RoomFindQueryP95(metric_value_p95));
                metrics.push(Metric::RoomFindQueryP99(metric_value_p99));
                metrics.push(Metric::RoomFindQueryMax(metric_value_max));
            }
            ProfilerKeys::RoomInsertQuery => {
                metrics.push(Metric::RoomInsertQueryP95(metric_value_p95));
                metrics.push(Metric::RoomInsertQueryP99(metric_value_p99));
                metrics.push(Metric::RoomInsertQueryMax(metric_value_max));
            }
            ProfilerKeys::RoomUpdateQuery => {
                metrics.push(Metric::RoomUpdateQueryP95(metric_value_p95));
                metrics.push(Metric::RoomUpdateQueryP99(metric_value_p99));
                metrics.push(Metric::RoomUpdateQueryMax(metric_value_max));
            }
            ProfilerKeys::StateTotalCountQuery => {
                metrics.push(Metric::StateTotalCountQueryP95(metric_value_p95));
                metrics.push(Metric::StateTotalCountQueryP99(metric_value_p99));
                metrics.push(Metric::StateTotalCountQueryMax(metric_value_max));
            }
            ProfilerKeys::StateQuery => {
                metrics.push(Metric::StateQueryP95(metric_value_p95));
                metrics.push(Metric::StateQueryP99(metric_value_p99));
                metrics.push(Metric::StateQueryMax(metric_value_max));
            }
        }
    }
    Ok(())
}
