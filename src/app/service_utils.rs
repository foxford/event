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

use crate::app::message_handler::{Message, MessageStream, MessageStreamTrait};
use crate::app::{endpoint::helpers, error::ErrorExt};

use super::error;

#[derive(Default)]
pub struct Notifications(Vec<Message>);

impl Notifications {
    fn into_stream(self) -> impl MessageStreamTrait {
        stream::iter(self.0)
    }

    fn push(&mut self, msg: Message) {
        self.0.push(msg);
    }
}

impl IntoIterator for Notifications {
    type Item = <Vec<Message> as IntoIterator>::Item;
    type IntoIter = <Vec<Message> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[derive(Default)]
pub struct AsyncTasks(Vec<JoinHandle<Message>>);

impl AsyncTasks {
    fn into_stream(self) -> impl MessageStreamTrait {
        stream::iter(self.0)
            .flat_map(stream::once)
            .filter(|jh_output| {
                if let Err(e) = jh_output {
                    error!(err = ?e, "Failed to await async task, join handle error");
                }
                future::ready(jh_output.is_ok())
            })
            .map(|jh_output| jh_output.unwrap())
    }

    fn push(&mut self, msg: JoinHandle<Message>) {
        self.0.push(msg);
    }
}

pub struct Response {
    notifications: Notifications,
    status: StatusCode,
    start_timestamp: DateTime<Utc>,
    authz_time: Option<Duration>,
    payload: Result<Value, serde_json::Error>,
    async_tasks: AsyncTasks,
}

impl Response {
    pub fn new(
        status: StatusCode,
        payload: impl Serialize + Send + Sync + 'static,
        start_timestamp: DateTime<Utc>,
        maybe_authz_time: Option<Duration>,
    ) -> Self {
        Self {
            notifications: Default::default(),
            status,
            start_timestamp,
            authz_time: maybe_authz_time,
            payload: serde_json::to_value(&payload),
            async_tasks: Default::default(),
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

        let stream = notifications
            .into_stream()
            .chain(self.async_tasks.into_stream());

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
            .0
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
        let tasks_stream = Box::new(self.async_tasks.into_stream()) as MessageStream;

        http::Response::builder()
            .status(self.status)
            .extension(self.notifications)
            .extension(tasks_stream)
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
