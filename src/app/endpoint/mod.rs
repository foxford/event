use std::result::Result as StdResult;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_derive::{Deserialize, Serialize};
use svc_agent::mqtt::{
    IncomingEvent, IncomingEventProperties, IncomingRequest, IncomingRequestProperties,
    IncomingResponse, IncomingResponseProperties,
};

use crate::app::context::Context;
use crate::app::error::Error as AppError;
pub(self) use crate::app::message_handler::MessageStream;
use crate::app::message_handler::{
    EventEnvelopeHandler, RequestEnvelopeHandler, ResponseEnvelopeHandler,
};

///////////////////////////////////////////////////////////////////////////////

pub(crate) type Result = StdResult<MessageStream, AppError>;

#[async_trait]
pub(crate) trait RequestHandler {
    type Payload: Send + DeserializeOwned;

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result;
}

macro_rules! request_routes {
    ($($m: pat => $h: ty),*) => {
        pub(crate) async fn route_request<C: Context>(
            context: &mut C,
            request: &IncomingRequest<String>,
        ) -> Option<MessageStream> {
            match request.properties().method() {
                $(
                    $m => Some(
                        <$h>::handle_envelope::<C>(context, request).await
                    ),
                )*
                _ => None,
            }
        }
    }
}

// Request routes configuration: method => RequestHandler
request_routes!(
    "agent.list" => agent::ListHandler,
    "agent.update" => agent::UpdateHandler,
    "change.create" => change::CreateHandler,
    "change.delete" => change::DeleteHandler,
    "change.list" => change::ListHandler,
    "edition.commit" => edition::CommitHandler,
    "edition.create" => edition::CreateHandler,
    "edition.list" => edition::ListHandler,
    "edition.delete" => edition::DeleteHandler,
    "event.create" => event::CreateHandler,
    "event.list" => event::ListHandler,
    "room.adjust" => room::AdjustHandler,
    "room.create" => room::CreateHandler,
    "room.dump_events" => room::EventsDumpHandler,
    "room.enter" => room::EnterHandler,
    "room.leave" => room::LeaveHandler,
    "room.read" => room::ReadHandler,
    "room.update" => room::UpdateHandler,
    "state.read" => state::ReadHandler,
    "system.vacuum" => system::VacuumHandler
);

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize, Serialize)]
pub(crate) enum CorrelationData {
    SubscriptionCreate(subscription::CorrelationDataPayload),
    SubscriptionDelete(subscription::CorrelationDataPayload),
    BroadcastSubscriptionCreate(subscription::CorrelationDataPayload),
    BroadcastSubscriptionDelete(subscription::CorrelationDataPayload),
}

#[async_trait]
pub(crate) trait ResponseHandler {
    type Payload: Send + DeserializeOwned;
    type CorrelationData: Sync;

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        respp: &IncomingResponseProperties,
        corr_data: &Self::CorrelationData,
    ) -> Result;
}

macro_rules! response_routes {
    ($($c: tt => $h: ty),*) => {
        #[allow(unused_variables)]
        pub(crate) async fn route_response<C: Context>(
            context: &mut C,
            response: &IncomingResponse<String>,
            corr_data: &CorrelationData,
        ) -> MessageStream {
            match corr_data {
                $(
                    CorrelationData::$c(cd) => <$h>::handle_envelope::<C>(context, response, cd).await,
                )*
            }
        }
    }
}

response_routes!(
    SubscriptionCreate => subscription::CreateResponseHandler,
    SubscriptionDelete => subscription::DeleteResponseHandler,
    BroadcastSubscriptionCreate => subscription::BroadcastCreateResponseHandler,
    BroadcastSubscriptionDelete => subscription::BroadcastDeleteResponseHandler
);

///////////////////////////////////////////////////////////////////////////////

#[async_trait]
pub(crate) trait EventHandler {
    type Payload: Send + DeserializeOwned;

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
    ) -> Result;
}

macro_rules! event_routes {
    ($($l: pat => $h: ty),*) => {
        #[allow(unused_variables)]
        pub(crate) async fn route_event<C: Context>(
            context: &mut C,
            event: &IncomingEvent<String>,
        ) -> Option<MessageStream> {
            match event.properties().label() {
                $(
                    Some($l) => Some(
                        <$h>::handle_envelope::<C>(context, event).await
                    ),
                )*
                _ => None,
            }
        }
    }
}

// Event routes configuration: label => EventHandler
event_routes!(
    "metric.pull" => metric::PullHandler,
    "subscription.delete" => subscription::DeleteEventHandler,
    "broadcast_subscription.delete" => subscription::BroadcastDeleteEventHandler
);

///////////////////////////////////////////////////////////////////////////////

mod agent;
pub(crate) mod authz;
mod change;
mod edition;
mod event;
pub(self) mod helpers;
pub(crate) mod metric;
mod room;
mod state;
mod subscription;
mod system;

pub(self) mod prelude {
    pub(super) use super::{helpers, EventHandler, RequestHandler, ResponseHandler, Result};
    pub(super) use crate::app::endpoint::authz::AuthzObject;
    pub(super) use crate::app::endpoint::CorrelationData;
    pub(super) use crate::app::error::{Error as AppError, ErrorExt, ErrorKind as AppErrorKind};
    pub(super) use crate::app::metrics::ProfilerKeys;

    pub(super) use svc_authn::Authenticable;
}
