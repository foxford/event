use std::hash::Hash;
use std::sync::atomic::Ordering;

use async_std::stream;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_derive::Deserialize;
use svc_agent::mqtt::{
    IncomingEventProperties, IntoPublishableMessage, OutgoingEvent, ResponseStatus,
    ShortTermTimingProperties,
};

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::app::metrics::{Metric, Metric2, MetricValue};
use crate::config::TelemetryConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ProfilerKeys {
    AgentInsertQuery,
    AgentListQuery,
    ChangeDeleteQuery,
    ChangeInsertQuery,
    ChangeListQuery,
    EditionDeleteQuery,
    EditionFindWithRoomQuery,
    EditionFindQuery,
    EditionInsertQuery,
    EditionListQuery,
    EventInsertQuery,
    EventListQuery,
    EventOriginalEventQuery,
    RoomFindQuery,
    RoomInsertQuery,
    RoomUpdateQuery,
    StateTotalCountQuery,
    StateQuery,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PullPayload {
    #[serde(default = "default_duration")]
    duration: u64,
}

fn default_duration() -> u64 {
    5
}

pub(crate) struct PullHandler;

#[async_trait]
impl EventHandler for PullHandler {
    type Payload = PullPayload;

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result {
        match context.config().telemetry {
            TelemetryConfig {
                id: Some(ref account_id),
            } => {
                let now = Utc::now();
                let mut metrics = vec![];

                append_mqtt_stats(&mut metrics, context, now, payload.duration)
                    .status(ResponseStatus::INTERNAL_SERVER_ERROR)?;
                append_db_stats(&mut metrics, context, now);
                append_redis_pool_metrics(&mut metrics, context, now);
                append_db_pool_stats(&mut metrics, context, now);

                append_profiler_stats(&mut metrics, context, now)
                    .status(ResponseStatus::INTERNAL_SERVER_ERROR)?;

                if let Some(counter) = context.running_requests() {
                    metrics.push(Metric::RunningRequests(MetricValue::new(
                        counter.load(Ordering::SeqCst),
                        now,
                    )));
                }

                let metrics2 = metrics
                    .clone()
                    .into_iter()
                    .map(|m| m.into())
                    .collect::<Vec<Metric2>>();

                let short_term_timing = ShortTermTimingProperties::until_now(start_timestamp);
                let props = evp.to_event("metric.create", short_term_timing.clone());
                let props2 = evp.to_event("metric.create", short_term_timing);
                let outgoing_event = OutgoingEvent::multicast(metrics, props, account_id);
                let outgoing_event2 = OutgoingEvent::multicast(metrics2, props2, account_id);
                let boxed_events = vec![
                    Box::new(outgoing_event) as Box<dyn IntoPublishableMessage + Send>,
                    Box::new(outgoing_event2) as Box<dyn IntoPublishableMessage + Send>,
                ];
                Ok(Box::new(stream::from_iter(boxed_events)))
            }

            _ => Ok(Box::new(stream::empty())),
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

fn append_db_stats(metrics: &mut Vec<Metric>, context: &dyn Context, now: DateTime<Utc>) {
    let db_state = context.db().state();

    metrics.push(Metric::DbConnections(MetricValue::new(
        db_state.connections as u64,
        now,
    )));
    metrics.push(Metric::IdleDbConnections(MetricValue::new(
        db_state.idle_connections as u64,
        now,
    )));

    let db_state = context.ro_db().state();

    metrics.push(Metric::RoDbConnections(MetricValue::new(
        db_state.connections as u64,
        now,
    )));
    metrics.push(Metric::IdleRoDbConnections(MetricValue::new(
        db_state.idle_connections as u64,
        now,
    )));
}

fn append_redis_pool_metrics(metrics: &mut Vec<Metric>, context: &dyn Context, now: DateTime<Utc>) {
    if let Some(pool) = context.redis_pool() {
        let pool_state = pool.state();

        metrics.push(Metric::RedisConnections(MetricValue::new(
            pool_state.connections as u64,
            now,
        )));
        metrics.push(Metric::IdleRedisConnections(MetricValue::new(
            pool_state.idle_connections as u64,
            now,
        )));
    }
}

fn append_db_pool_stats(metrics: &mut Vec<Metric>, context: &dyn Context, now: DateTime<Utc>) {
    if let Some(db_pool_stats) = context.db_pool_stats() {
        let stats = db_pool_stats.get_stats();

        let m = [
            Metric::DbPoolCheckinAverage(MetricValue::new(stats.avg_checkin, now)),
            Metric::MaxDbPoolCheckin(MetricValue::new(stats.max_checkin, now)),
            Metric::DbPoolCheckoutAverage(MetricValue::new(stats.avg_checkout, now)),
            Metric::MaxDbPoolCheckout(MetricValue::new(stats.max_checkout, now)),
            Metric::DbPoolTimeoutAverage(MetricValue::new(stats.avg_timeout, now)),
            Metric::MaxDbPoolTimeout(MetricValue::new(stats.max_timeout, now)),
            Metric::DbPoolReleaseAverage(MetricValue::new(stats.avg_release, now)),
            Metric::MaxDbPoolRelease(MetricValue::new(stats.max_release, now)),
        ];

        metrics.extend_from_slice(&m);
    }

    if let Some(db_pool_stats) = context.ro_db_pool_stats() {
        let stats = db_pool_stats.get_stats();

        let m = [
            Metric::RoDbPoolCheckinAverage(MetricValue::new(stats.avg_checkin, now)),
            Metric::MaxRoDbPoolCheckin(MetricValue::new(stats.max_checkin, now)),
            Metric::RoDbPoolCheckoutAverage(MetricValue::new(stats.avg_checkout, now)),
            Metric::MaxRoDbPoolCheckout(MetricValue::new(stats.max_checkout, now)),
            Metric::RoDbPoolTimeoutAverage(MetricValue::new(stats.avg_timeout, now)),
            Metric::MaxRoDbPoolTimeout(MetricValue::new(stats.max_timeout, now)),
            Metric::RoDbPoolReleaseAverage(MetricValue::new(stats.avg_release, now)),
            Metric::MaxRoDbPoolRelease(MetricValue::new(stats.max_release, now)),
        ];

        metrics.extend_from_slice(&m);
    }
}

fn append_profiler_stats(
    metrics: &mut Vec<Metric>,
    context: &dyn Context,
    now: DateTime<Utc>,
) -> std::result::Result<(), String> {
    let profiler_report = context.profiler().flush().map_err(|err| err.to_string())?;

    for (key, entry_report) in profiler_report {
        let metric_value_p95 = MetricValue::new(entry_report.p95 as u64, now);
        let metric_value_p99 = MetricValue::new(entry_report.p99 as u64, now);
        let metric_value_max = MetricValue::new(entry_report.max as u64, now);

        match key {
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
            ProfilerKeys::ChangeDeleteQuery => {
                metrics.push(Metric::ChangeDeleteQueryP95(metric_value_p95));
                metrics.push(Metric::ChangeDeleteQueryP99(metric_value_p99));
                metrics.push(Metric::ChangeDeleteQueryMax(metric_value_max));
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
            ProfilerKeys::EditionFindQuery => {
                metrics.push(Metric::EditionFindQueryP95(metric_value_p95));
                metrics.push(Metric::EditionFindQueryP99(metric_value_p99));
                metrics.push(Metric::EditionFindQueryMax(metric_value_max));
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
