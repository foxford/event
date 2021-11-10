use axum::response::IntoResponse;
use chrono::{DateTime, Duration, Utc};
use futures::{future, stream, StreamExt};
use http::StatusCode;
use serde::Serialize;
use serde_json::Value;
use svc_agent::{
    mqtt::{
        IncomingRequestProperties, IntoPublishableMessage, OutgoingEvent, OutgoingEventProperties,
        ShortTermTimingProperties,
    },
    Addressable, AgentId, Authenticable,
};
use tokio::task::JoinHandle;

use crate::app::message_handler::MessageStream;
use crate::app::{endpoint::helpers, error::ErrorExt};

use super::error;

pub struct Response {
    notifications: Vec<Box<dyn IntoPublishableMessage + Send + Sync + 'static>>,
    status: StatusCode,
    start_timestamp: DateTime<Utc>,
    authz_time: Option<Duration>,
    payload: Result<Value, serde_json::Error>,
    async_tasks: Vec<JoinHandle<Box<dyn IntoPublishableMessage + Send + Sync + 'static>>>,
}

impl Response {
    pub fn new(
        status: StatusCode,
        payload: impl Serialize + Send + Sync + 'static,
        start_timestamp: DateTime<Utc>,
        maybe_authz_time: Option<Duration>,
    ) -> Self {
        Self {
            notifications: Vec::new(),
            status,
            start_timestamp,
            authz_time: maybe_authz_time,
            payload: serde_json::to_value(&payload),
            async_tasks: Vec::new(),
        }
    }

    pub fn into_mqtt_messages(
        self,
        reqp: &IncomingRequestProperties,
    ) -> Result<MessageStream, error::Error> {
        let mut notifications = self.notifications;
        if self.status != StatusCode::NO_CONTENT {
            let payload = self.payload.error(error::ErrorKind::InvalidPayload)?;
            let response = helpers::build_response(
                self.status,
                payload,
                reqp,
                self.start_timestamp,
                self.authz_time,
            );
            notifications.push(response);
        }

        let tasks_stream = stream::iter(self.async_tasks)
            .flat_map(stream::once)
            .filter(|jh_output| future::ready(jh_output.is_ok()))
            .map(|jh_output| jh_output.unwrap());

        let stream = stream::iter(notifications).chain(tasks_stream);

        Ok(Box::new(stream))
    }

    pub fn add_notification(
        &mut self,
        label: &'static str,
        path: &str,
        payload: impl Serialize + Send + Sync + 'static,
        start_timestamp: DateTime<Utc>,
    ) {
        let timing = ShortTermTimingProperties::until_now(start_timestamp);
        let props = OutgoingEventProperties::new(label, timing);
        self.notifications
            .push(Box::new(OutgoingEvent::broadcast(payload, props, path)))
    }

    pub fn add_async_task(
        &mut self,
        task: JoinHandle<Box<dyn IntoPublishableMessage + Send + Sync + 'static>>,
    ) {
        self.async_tasks.push(task);
    }
}

impl IntoResponse for Response {
    type Body = axum::body::Body;

    type BodyError = <Self::Body as axum::body::HttpBody>::Error;

    fn into_response(self) -> hyper::Response<Self::Body> {
        http::Response::builder()
            .status(self.status)
            .extension(self.notifications)
            .extension(self.async_tasks)
            .body(axum::body::Body::from(
                serde_json::to_string(&self.payload.expect("Todo")).expect("todo"),
            ))
            .expect("Must be valid response")
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RequestParams<'a> {
    Http { agent_id: &'a AgentId },
    MqttParams(&'a IncomingRequestProperties),
}

impl<'a> Addressable for RequestParams<'a> {
    fn as_agent_id(&self) -> &svc_agent::AgentId {
        match self {
            RequestParams::Http { agent_id } => agent_id,
            RequestParams::MqttParams(reqp) => reqp.as_agent_id(),
        }
    }
}

impl<'a> Authenticable for RequestParams<'a> {
    fn as_account_id(&self) -> &svc_agent::AccountId {
        match self {
            RequestParams::Http { agent_id } => agent_id.as_account_id(),
            RequestParams::MqttParams(reqp) => reqp.as_account_id(),
        }
    }
}