use std::result::Result as StdResult;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_derive::{Deserialize, Serialize};
use svc_agent::mqtt::{
    IncomingEvent, IncomingEventProperties, IncomingRequest, IncomingResponseProperties,
};

use crate::app::context::Context;
use crate::app::error::Error as AppError;
pub(self) use crate::app::message_handler::MessageStream;
use crate::app::message_handler::{EventEnvelopeHandler, RequestEnvelopeHandler};

use super::service_utils::{RequestParams, Response as AppResponse};

///////////////////////////////////////////////////////////////////////////////

pub type RequestResult = StdResult<AppResponse, AppError>;
pub(crate) type MqttResult = StdResult<MessageStream, AppError>;

#[async_trait]
pub(crate) trait RequestHandler {
    type Payload: Send + DeserializeOwned;

    async fn handle<C: Context + Sync>(
        context: &mut C,
        payload: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult;
}

macro_rules! request_routes {
    ($($m: pat => $h: ty),*) => {
        pub(crate) async fn route_request<C: Context + Sync>(
            context: &mut C,
            request: &IncomingRequest<String>,
        ) -> Option<MessageStream> {
            match request.properties().method() {
                $(
                    p@$m => {
                        let metrics = context.metrics();
                        let _timer = metrics.start_request(p);
                        Some(<$h>::handle_envelope::<C>(context, request).await)
                }
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
    "ban.list" => ban::ListHandler,
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
    "room.locked_types" => room::LockedTypesHandler,
    "room.read" => room::ReadHandler,
    "room.update" => room::UpdateHandler,
    "state.read" => state::ReadHandler,
    "system.vacuum" => system::VacuumHandler
);

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize, Serialize)]
pub(crate) enum CorrelationData {
    SubscriptionCreate(subscription::CorrelationDataPayload),
    BroadcastSubscriptionCreate(subscription::CorrelationDataPayload),
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
    ) -> MqttResult;
}

///////////////////////////////////////////////////////////////////////////////

#[async_trait]
pub(crate) trait EventHandler {
    type Payload: Send + DeserializeOwned;

    async fn handle<C: Context + Sync>(
        context: &mut C,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
    ) -> MqttResult;
}

macro_rules! event_routes {
    ($($l: pat => $h: ty),*) => {
        #[allow(unused_variables)]
        pub(crate) async fn route_event<C: Context + Sync>(
            context: &mut C,
            event: &IncomingEvent<String>,
        ) -> Option<MessageStream> {
            match event.properties().label() {
                $(
                    Some(p@$l) => {
                        let metrics = context.metrics();
                        let _timer = metrics.start_request(p);
                        Some(<$h>::handle_envelope::<C>(context, event).await)}
                )*
                _ => None,
            }
        }
    }
}

// Event routes configuration: label => EventHandler
event_routes!(
    "subscription.delete" => subscription::DeleteEventHandler,
    "broadcast_subscription.delete" => subscription::BroadcastDeleteEventHandler
);

///////////////////////////////////////////////////////////////////////////////

pub mod agent;
pub(crate) mod authz;
pub mod ban;
pub mod change;
pub mod edition;
pub mod event;
pub mod helpers;
pub mod room;
pub mod state;
mod subscription;
mod system;

pub(self) mod prelude {
    pub(super) use super::{
        helpers, AppResponse, EventHandler, MqttResult, RequestHandler, RequestParams,
        RequestResult,
    };
    pub(super) use crate::app::endpoint::authz::AuthzObject;
    pub(super) use crate::app::error::{Error as AppError, ErrorExt, ErrorKind as AppErrorKind};
    pub(super) use crate::metrics::QueryKey;

    pub use crate::app::context::{AppContext, Context};
    pub use std::sync::Arc;

    pub(super) use svc_authn::Authenticable;
}

pub async fn read_options() -> hyper::Response<hyper::Body> {
    hyper::Response::builder()
        .body(hyper::Body::empty())
        .unwrap()
}
