use std::result::Result as StdResult;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use failure::Error;
use serde::de::DeserializeOwned;
use svc_agent::mqtt::{
    compat::IncomingEnvelope, IncomingEventProperties, IncomingRequestProperties, ResponseStatus,
};
use svc_error::Error as SvcError;

use crate::app::context::Context;
pub(self) use crate::app::message_handler::MessageStream;
use crate::app::message_handler::{EventEnvelopeHandler, RequestEnvelopeHandler};

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
    ) -> Result;
}

macro_rules! request_routes {
    ($($m: pat => $h: ty),*) => {
        pub(crate) async fn route_request<C: Context>(
            context: &C,
            envelope: IncomingEnvelope,
            reqp: &IncomingRequestProperties,
            start_timestamp: DateTime<Utc>,
        ) -> Option<MessageStream> {
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
    ) -> Result;
}

macro_rules! event_routes {
    ($($l: pat => $h: ty),*) => {
        #[allow(unused_variables)]
        pub(crate) async fn route_event<C: Context>(
            context: &C,
            envelope: IncomingEnvelope,
            evp: &IncomingEventProperties,
            start_timestamp: DateTime<Utc>,
        ) -> Option<MessageStream> {
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

pub(crate) type Result = StdResult<MessageStream, SvcError>;

pub(crate) trait SvcErrorSugar<T> {
    fn status(self, status: ResponseStatus) -> StdResult<T, SvcError>;
}

impl<T> SvcErrorSugar<T> for StdResult<T, Error> {
    fn status(self, status: ResponseStatus) -> StdResult<T, SvcError> {
        self.map_err(|err| {
            SvcError::builder()
                .status(status)
                .detail(&err.to_string())
                .build()
        })
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(self) mod helpers;
mod agent;
mod edition;
mod event;
mod room;
mod state;
mod subscription;

pub(self) mod prelude {
    pub(super) use super::{helpers, EventHandler, RequestHandler, Result, SvcErrorSugar};
}
