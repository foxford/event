use enum_iterator::IntoEnumIterator;
use futures::Future;
use parking_lot::RwLock;
use prometheus::{
    Histogram, HistogramOpts, HistogramTimer, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    Opts, Registry,
};
use serde::Serialize;
use std::{collections::HashMap, sync::Arc};

use crate::app::error::ErrorKind;

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Eq, PartialEq, Hash, Serialize, IntoEnumIterator)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QueryKey {
    AdjustmentInsertQuery,
    AgentDeleteQuery,
    AgentFindWithBanQuery,
    AgentInsertQuery,
    AgentListQuery,
    AgentUpdateQuery,
    BanDeleteQuery,
    BanInsertQuery,
    ChangeDeleteQuery,
    ChangeFindWithRoomQuery,
    ChangeInsertQuery,
    ChangeListQuery,
    EditionCloneEventsQuery,
    EditionCommitTxnCommit,
    EditionDeleteQuery,
    EditionFindWithRoomQuery,
    EditionInsertQuery,
    EditionListQuery,
    EventDeleteQuery,
    EventDumpQuery,
    EventInsertQuery,
    EventListQuery,
    EventOriginalEventQuery,
    EventVacuumQuery,
    RoomAdjustCloneEventsQuery,
    RoomFindQuery,
    RoomInsertQuery,
    RoomUpdateQuery,
    StateTotalCountQuery,
    StateQuery,
}

pub(crate) struct Metrics {
    pub request_duration: RwLock<HashMap<String, Option<Histogram>>>,
    pub request_duration_vec: HistogramVec,
    pub db_duration: HashMap<QueryKey, Histogram>,
    pub app_result_ok: IntCounter,
    pub app_results_errors: HashMap<ErrorKind, IntCounter>,
    pub mqtt_reconnection: IntCounter,
    pub mqtt_disconnect: IntCounter,
    pub mqtt_connection_error: IntCounter,
    pub total_requests: IntCounter,
    pub running_requests_total: IntGauge,
}

impl Metrics {
    pub fn new(registry: &Registry) -> anyhow::Result<Self> {
        let request_duration = HistogramVec::new(
            HistogramOpts::new("request_duration", "Request duration"),
            &["method"],
        )?;
        let db_duration = HistogramVec::new(
            HistogramOpts::new("db_duration", "DB duration"),
            &["method"],
        )?;
        let request_stats =
            IntCounterVec::new(Opts::new("request_stats", "Request stats"), &["status"])?;
        let total_requests = IntCounter::new("incoming_requests_total", "Total requests")?;
        let running_requests_total =
            IntGauge::new("running_requests_total", "Total running requests")?;
        let mqtt_errors = IntCounterVec::new(
            Opts::new("mqtt_messages", "Mqtt message types"),
            &["status"],
        )?;
        registry.register(Box::new(mqtt_errors.clone()))?;
        registry.register(Box::new(request_duration.clone()))?;
        registry.register(Box::new(db_duration.clone()))?;
        registry.register(Box::new(request_stats.clone()))?;
        registry.register(Box::new(total_requests.clone()))?;
        registry.register(Box::new(running_requests_total.clone()))?;
        Ok(Self {
            request_duration: RwLock::new(HashMap::new()),
            request_duration_vec: request_duration,
            total_requests,
            app_result_ok: request_stats.get_metric_with_label_values(&["ok"])?,
            app_results_errors: ErrorKind::into_enum_iter()
                .map(|kind| {
                    Ok((
                        kind,
                        request_stats.get_metric_with_label_values(&[kind.kind()])?,
                    ))
                })
                .collect::<anyhow::Result<_>>()?,
            running_requests_total,
            mqtt_connection_error: mqtt_errors
                .get_metric_with_label_values(&["connection_error"])?,
            mqtt_disconnect: mqtt_errors.get_metric_with_label_values(&["disconnect"])?,
            mqtt_reconnection: mqtt_errors.get_metric_with_label_values(&["reconnect"])?,
            db_duration: QueryKey::into_enum_iter()
                .map(|kind| {
                    Ok((
                        kind,
                        db_duration
                            .get_metric_with_label_values(&[&serde_json::to_string(&kind)?])?,
                    ))
                })
                .collect::<anyhow::Result<_>>()?,
        })
    }

    pub async fn measure_query<F>(&self, key: QueryKey, func: F) -> F::Output
    where
        F: Future,
    {
        let _timer = self.db_duration.get(&key).map(|m| m.start_timer());
        func.await
    }

    pub async fn start_request(&self, request: &str) -> Option<HistogramTimer> {
        {
            let request_duration = self.request_duration.read();
            if let Some(metric) = request_duration.get(request) {
                return Some(metric.as_ref()?.start_timer());
            }
        }
        {
            let mut request_duration = self.request_duration.write();
            Some(
                request_duration
                    .entry(request.to_string())
                    .or_insert_with(|| {
                        match self
                            .request_duration_vec
                            .get_metric_with_label_values(&[request])
                        {
                            Ok(x) => Some(x),
                            Err(err) => {
                                error!(crate::LOG, "Bad metric: {:?}", err);
                                None
                            }
                        }
                    })
                    .as_ref()?
                    .start_timer(),
            )
        }
    }

    pub fn observe_app_result(&self, result: &crate::app::endpoint::Result) {
        match result {
            Ok(_) => {
                self.app_result_ok.inc();
            }
            Err(err) => {
                if let Some(m) = self.app_results_errors.get(&err.error_kind()) {
                    m.inc()
                }
            }
        }
    }

    pub fn request_started(self: Arc<Self>) -> StartedRequest {
        StartedRequest::new(self)
    }
}

pub struct StartedRequest {
    metric: Arc<Metrics>,
}

impl StartedRequest {
    fn new(metric: Arc<Metrics>) -> Self {
        metric.running_requests_total.inc();
        Self { metric }
    }
}

impl Drop for StartedRequest {
    fn drop(&mut self) {
        self.metric.running_requests_total.dec();
    }
}
