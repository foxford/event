use std::hash::Hash;

use async_std::stream;
use async_trait::async_trait;
use chrono::{serde::ts_seconds, DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use svc_agent::mqtt::{
    IncomingEventProperties, IntoPublishableMessage, OutgoingEvent, ResponseStatus,
    ShortTermTimingProperties,
};

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::config::TelemetryConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ProfilerKeys {
    StateQuery,
    StateTotalCountQuery,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PullPayload {
    #[serde(default = "default_duration")]
    duration: u64,
}

fn default_duration() -> u64 {
    5
}

#[derive(Serialize, Copy, Clone)]
pub(crate) struct MetricValue<T: serde::Serialize> {
    value: T,
    #[serde(with = "ts_seconds")]
    timestamp: DateTime<Utc>,
}

impl<T: serde::Serialize> MetricValue<T> {
    fn new(value: T, timestamp: DateTime<Utc>) -> Self {
        Self { value, timestamp }
    }
}

#[derive(Serialize, Copy, Clone)]
#[serde(tag = "metric")]
pub(crate) enum Metric {
    #[serde(rename(serialize = "apps.event.incoming_requests_total"))]
    IncomingQueueRequests(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.incoming_responses_total"))]
    IncomingQueueResponses(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.incoming_events_total"))]
    IncomingQueueEvents(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.outgoing_requests_total"))]
    OutgoingQueueRequests(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.outgoing_responses_total"))]
    OutgoingQueueResponses(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.outgoing_events_total"))]
    OutgoingQueueEvents(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.db_connections_total"))]
    DbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.idle_db_connections_total"))]
    IdleDbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.db_pool_checkin_average_total"))]
    DbPoolCheckinAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_db_pool_checkin_total"))]
    MaxDbPoolCheckin(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.db_pool_checkout_average_total"))]
    DbPoolCheckoutAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_db_pool_checkout_total"))]
    MaxDbPoolCheckout(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.db_pool_release_average_total"))]
    DbPoolReleaseAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_db_pool_release_total"))]
    MaxDbPoolRelease(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.db_pool_timeout_average_total"))]
    DbPoolTimeoutAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_db_pool_timeout_total"))]
    MaxDbPoolTimeout(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.redis_connections_total"))]
    RedisConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.idle_redis_connections_total"))]
    IdleRedisConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.state_total_count_query_p95_microseconds"))]
    StateTotalCountQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.state_total_count_query_p99_microseconds"))]
    StateTotalCountQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.state_total_count_query_max_microseconds"))]
    StateTotalCountQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.state_query_p95_microseconds"))]
    StateQueryP95(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.state_query_p99_microseconds"))]
    StateQueryP99(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.state_query_max_microseconds"))]
    StateQueryMax(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.ro_db_connections_total"))]
    RoDbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.idle_ro_db_connections_total"))]
    IdleRoDbConnections(MetricValue<u64>),
    #[serde(rename(serialize = "apps.event.ro_db_pool_checkin_average_total"))]
    RoDbPoolCheckinAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_ro_db_pool_checkin_total"))]
    MaxRoDbPoolCheckin(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.ro_db_pool_checkout_average_total"))]
    RoDbPoolCheckoutAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_ro_db_pool_checkout_total"))]
    MaxRoDbPoolCheckout(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.ro_db_pool_release_average_total"))]
    RoDbPoolReleaseAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_ro_db_pool_release_total"))]
    MaxRoDbPoolRelease(MetricValue<u128>),
    #[serde(rename(serialize = "apps.event.ro_db_pool_timeout_average_total"))]
    RoDbPoolTimeoutAverage(MetricValue<f64>),
    #[serde(rename(serialize = "apps.event.max_ro_db_pool_timeout_total"))]
    MaxRoDbPoolTimeout(MetricValue<u128>),
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

                let short_term_timing = ShortTermTimingProperties::until_now(start_timestamp);
                let props = evp.to_event("metric.create", short_term_timing);
                let outgoing_event = OutgoingEvent::multicast(metrics, props, account_id);
                let boxed_event =
                    Box::new(outgoing_event) as Box<dyn IntoPublishableMessage + Send>;
                Ok(Box::new(stream::once(boxed_event)))
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
            ProfilerKeys::StateQuery => {
                metrics.push(Metric::StateQueryP95(metric_value_p95));
                metrics.push(Metric::StateQueryP99(metric_value_p99));
                metrics.push(Metric::StateQueryMax(metric_value_max));
            }
            ProfilerKeys::StateTotalCountQuery => {
                metrics.push(Metric::StateTotalCountQueryP95(metric_value_p95));
                metrics.push(Metric::StateTotalCountQueryP99(metric_value_p99));
                metrics.push(Metric::StateTotalCountQueryMax(metric_value_max));
            }
        }
    }
    Ok(())
}
