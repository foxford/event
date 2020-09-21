use std::hash::Hash;

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
use crate::app::metrics::Metric2;
use crate::config::TelemetryConfig;

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ProfilerKeys {
    DbConnAcquisition,
    RoDbConnAcquisition,
    AdjustmentInsertQuery,
    AgentDeleteQuery,
    AgentInsertQuery,
    AgentListQuery,
    AgentUpdateQuery,
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
    EventInsertQuery,
    EventListQuery,
    EventOriginalEventQuery,
    RoomAdjustCloneEventsQuery,
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
                let metrics = context
                    .get_metrics(payload.duration)
                    .status(ResponseStatus::INTERNAL_SERVER_ERROR)?;

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
