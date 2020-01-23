use chrono::{DateTime, Utc};
use failure::Error;
use serde::de::DeserializeOwned;
use svc_agent::mqtt::{
    compat::IncomingEnvelope, IncomingEventProperties, IncomingRequestProperties,
    IntoPublishableDump,
};

use crate::app::{
    message_handler::{EventEnvelopeHandler, RequestEnvelopeHandler},
    Context,
};

///////////////////////////////////////////////////////////////////////////////

pub(crate) trait RequestHandler {
    type Payload: DeserializeOwned;
    const ERROR_TITLE: &'static str;

    fn handle(
        context: &Context,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, Error>;
}

macro_rules! request_routes {
    ($($m: pat => $h: ty),*) => {
        pub(crate) fn route_request(
            context: &Context,
            envelope: IncomingEnvelope,
            reqp: &IncomingRequestProperties,
            start_timestamp: DateTime<Utc>,
        ) -> Option<Vec<Box<dyn IntoPublishableDump>>> {
            match reqp.method() {
                $(
                    $m => Some(<$h>::handle_envelope(context, envelope, reqp, start_timestamp)),
                )*
                _ => None,
            }
        }
    }
}

// Request routes configuration: method => RequestHandler
request_routes!(
    "system.ping" => system::PingHandler
);

///////////////////////////////////////////////////////////////////////////////

pub(crate) trait EventHandler {
    type Payload: DeserializeOwned;

    fn handle(
        context: &Context,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, Error>;
}

macro_rules! event_routes {
    ($($l: pat => $h: ty),*) => {
        pub(crate) fn route_event(
            context: &Context,
            envelope: IncomingEnvelope,
            evp: &IncomingEventProperties,
            start_timestamp: DateTime<Utc>,
        ) -> Option<Vec<Box<dyn IntoPublishableDump>>> {
            match evp.label() {
                $(
                    Some($l) => Some(<$h>::handle_envelope(context, envelope, evp, start_timestamp)),
                )*
                _ => None,
            }
        }
    }
}

// Event routes configuration: label => EventHandler
event_routes!(
    "dummy.say" => dummy::SayHandler
);

///////////////////////////////////////////////////////////////////////////////

mod dummy;
mod system;
