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

#[derive(Serialize)]
pub(crate) struct MetricValue {
    value: u64,
    #[serde(with = "ts_seconds")]
    timestamp: DateTime<Utc>,
}

impl MetricValue {
    fn new(value: u64, timestamp: DateTime<Utc>) -> Self {
        Self { value, timestamp }
    }
}

#[derive(Serialize)]
#[serde(tag = "metric")]
pub(crate) enum Metric {
    #[serde(rename(serialize = "apps.event.incoming_requests_total"))]
    IncomingQueueRequests(MetricValue),
    #[serde(rename(serialize = "apps.event.incoming_responses_total"))]
    IncomingQueueResponses(MetricValue),
    #[serde(rename(serialize = "apps.event.incoming_events_total"))]
    IncomingQueueEvents(MetricValue),
    #[serde(rename(serialize = "apps.event.outgoing_requests_total"))]
    OutgoingQueueRequests(MetricValue),
    #[serde(rename(serialize = "apps.event.outgoing_responses_total"))]
    OutgoingQueueResponses(MetricValue),
    #[serde(rename(serialize = "apps.event.outgoing_events_total"))]
    OutgoingQueueEvents(MetricValue),
    #[serde(rename(serialize = "apps.event.db_connections_total"))]
    DbConnections(MetricValue),
    #[serde(rename(serialize = "apps.event.idle_db_connections_total"))]
    IdleDbConnections(MetricValue),
    #[serde(rename(serialize = "apps.event.redis_connections_total"))]
    RedisConnections(MetricValue),
    #[serde(rename(serialize = "apps.event.idle_redis_connections_total"))]
    IdleRedisConnections(MetricValue),
    #[serde(rename(serialize = "apps.event.state_total_count_query_p95_microseconds"))]
    StateTotalCountQueryP95(MetricValue),
    #[serde(rename(serialize = "apps.event.state_total_count_query_p99_microseconds"))]
    StateTotalCountQueryP99(MetricValue),
    #[serde(rename(serialize = "apps.event.state_total_count_query_max_microseconds"))]
    StateTotalCountQueryMax(MetricValue),
    #[serde(rename(serialize = "apps.event.state_query_p95_microseconds"))]
    StateQueryP95(MetricValue),
    #[serde(rename(serialize = "apps.event.state_query_p99_microseconds"))]
    StateQueryP99(MetricValue),
    #[serde(rename(serialize = "apps.event.state_query_max_microseconds"))]
    StateQueryMax(MetricValue),
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

                let mut metrics = if let Some(qc) = context.queue_counter() {
                    let stats = qc
                        .get_stats(payload.duration)
                        .status(ResponseStatus::BAD_REQUEST)?;

                    vec![
                        Metric::IncomingQueueRequests(MetricValue::new(
                            stats.incoming_requests,
                            now,
                        )),
                        Metric::IncomingQueueResponses(MetricValue::new(
                            stats.incoming_responses,
                            now,
                        )),
                        Metric::IncomingQueueEvents(MetricValue::new(stats.incoming_events, now)),
                        Metric::OutgoingQueueRequests(MetricValue::new(
                            stats.outgoing_requests,
                            now,
                        )),
                        Metric::OutgoingQueueResponses(MetricValue::new(
                            stats.outgoing_responses,
                            now,
                        )),
                        Metric::OutgoingQueueEvents(MetricValue::new(stats.outgoing_events, now)),
                    ]
                } else {
                    vec![]
                };

                let db_state = context.db().state();

                let metric_value = MetricValue::new(db_state.connections as u64, now);
                metrics.push(Metric::DbConnections(metric_value));

                let metric_value = MetricValue::new(db_state.idle_connections as u64, now);
                metrics.push(Metric::IdleDbConnections(metric_value));

                if let Some(pool) = context.redis_pool() {
                    let pool_state = pool.state();

                    let metric_value = MetricValue::new(pool_state.connections as u64, now);
                    metrics.push(Metric::RedisConnections(metric_value));

                    let metric_value = MetricValue::new(pool_state.idle_connections as u64, now);
                    metrics.push(Metric::IdleRedisConnections(metric_value));
                }

                let profiler_report = context
                    .profiler()
                    .flush()
                    .map_err(|err| err.to_string())
                    .status(ResponseStatus::INTERNAL_SERVER_ERROR)?;

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
