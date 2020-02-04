use async_trait::async_trait;
use chrono::{DateTime, Utc};
use failure::Error;
use serde::de::DeserializeOwned;
use svc_agent::mqtt::{
    compat::IncomingEnvelope, IncomingEventProperties, IncomingRequestProperties,
    IntoPublishableDump,
};

#[allow(unused_imports)]
use crate::app::{
    message_handler::{EventEnvelopeHandler, RequestEnvelopeHandler},
    Context,
};

///////////////////////////////////////////////////////////////////////////////

#[async_trait]
pub(crate) trait RequestHandler {
    type Payload: Send + DeserializeOwned;
    const ERROR_TITLE: &'static str;

    async fn handle(
        context: &Context,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, Error>;
}

macro_rules! request_routes {
    ($($m: pat => $h: ty),*) => {
        pub(crate) async fn route_request(
            context: &Context,
            envelope: IncomingEnvelope,
            reqp: &IncomingRequestProperties,
            start_timestamp: DateTime<Utc>,
        ) -> Option<Vec<Box<dyn IntoPublishableDump>>> {
            match reqp.method() {
                $(
                    $m => {
                        Some(<$h>::handle_envelope(context, envelope, reqp, start_timestamp).await)
                    }
                )*
                _ => None,
            }
        }
    }
}

// Request routes configuration: method => RequestHandler
request_routes!(
    "room.create" => room::CreateHandler,
    "room.read" => room::ReadHandler,
    "room.adjust" => room::AdjustHandler
);

///////////////////////////////////////////////////////////////////////////////

#[async_trait]
pub(crate) trait EventHandler {
    type Payload: Send + DeserializeOwned;

    async fn handle(
        context: &Context,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, Error>;
}

macro_rules! event_routes {
    ($($l: pat => $h: ty),*) => {
        #[allow(unused_variables)]
        pub(crate) async fn route_event(
            context: &Context,
            envelope: IncomingEnvelope,
            evp: &IncomingEventProperties,
            start_timestamp: DateTime<Utc>,
        ) -> Option<Vec<Box<dyn IntoPublishableDump>>> {
            match evp.label() {
                $(
                    Some($l) => {
                        Some(<$h>::handle_envelope(context, envelope, evp, start_timestamp).await)
                    }
                )*
                _ => None,
            }
        }
    }
}

// Event routes configuration: label => EventHandler
event_routes!();

///////////////////////////////////////////////////////////////////////////////

mod helpers;
mod room;
