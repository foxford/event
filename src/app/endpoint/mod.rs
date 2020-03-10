use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use svc_agent::mqtt::{
    compat::IncomingEnvelope, IncomingEventProperties, IncomingRequestProperties,
    IntoPublishableDump,
};
use svc_error::Error as SvcError;

#[allow(unused_imports)]
use crate::app::{
    context::Context,
    message_handler::{EventEnvelopeHandler, RequestEnvelopeHandler},
};

///////////////////////////////////////////////////////////////////////////////

#[async_trait]
pub(crate) trait RequestHandler {
    type Payload: Send + DeserializeOwned;
    const ERROR_TITLE: &'static str;

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError>;
}

macro_rules! request_routes {
    ($($m: pat => $h: ty),*) => {
        pub(crate) async fn route_request<C: Context>(
            context: &C,
            envelope: IncomingEnvelope,
            reqp: &IncomingRequestProperties,
            start_timestamp: DateTime<Utc>,
        ) -> Option<Vec<Box<dyn IntoPublishableDump>>> {
            match reqp.method() {
                $(
                    $m => Some(
                        <$h>::handle_envelope::<C>(context, envelope, reqp, start_timestamp).await
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
    "edition.create" => edition::CreateHandler,
    "edition.list" => edition::ListHandler,
    "event.create" => event::CreateHandler,
    "event.list" => event::ListHandler,
    "room.adjust" => room::AdjustHandler,
    "room.create" => room::CreateHandler,
    "room.enter" => room::EnterHandler,
    "room.leave" => room::LeaveHandler,
    "room.read" => room::ReadHandler,
    "state.read" => state::ReadHandler
);

///////////////////////////////////////////////////////////////////////////////

#[async_trait]
pub(crate) trait EventHandler {
    type Payload: Send + DeserializeOwned;

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError>;
}

macro_rules! event_routes {
    ($($l: pat => $h: ty),*) => {
        #[allow(unused_variables)]
        pub(crate) async fn route_event<C: Context>(
            context: &C,
            envelope: IncomingEnvelope,
            evp: &IncomingEventProperties,
            start_timestamp: DateTime<Utc>,
        ) -> Option<Vec<Box<dyn IntoPublishableDump>>> {
            match evp.label() {
                $(
                    Some($l) => Some(
                        <$h>::handle_envelope::<C>(context, envelope, evp, start_timestamp).await
                    ),
                )*
                _ => None,
            }
        }
    }
}

// Event routes configuration: label => EventHandler
event_routes!(
    "subscription.delete" => subscription::DeleteHandler,
    "subscription.create" => subscription::CreateHandler
);

///////////////////////////////////////////////////////////////////////////////

mod helpers;
mod agent;
mod edition;
mod event;
mod room;
mod state;
mod subscription;
