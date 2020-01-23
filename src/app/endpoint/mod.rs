use chrono::{DateTime, Utc};
use failure::Error;
use serde::de::DeserializeOwned;
use svc_agent::mqtt::{compat::IncomingEnvelope, IncomingRequestProperties, IntoPublishableDump};

use crate::app::{message_handler::RequestEnvelopeHandler, Context};

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

pub(crate) mod system;
