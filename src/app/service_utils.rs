use axum::{response::IntoResponse, Json};
use chrono::{DateTime, Duration, Utc};
use futures::{future, stream, StreamExt};
use http::StatusCode;
use serde::Serialize;
use serde_json::Value;
use svc_agent::{
    mqtt::{
        IncomingRequestProperties, OutgoingEvent, OutgoingEventProperties,
        ShortTermTimingProperties,
    },
    Addressable, AgentId, Authenticable,
};
use tokio::task::JoinHandle;

use crate::app::endpoint::helpers;
use crate::app::message_handler::{Message, MessageStream, MessageStreamTrait};

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
                if let Err(err) = jh_output {
                    error!(?err, "Failed to await async task, join handle error");
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
    payload: Value,
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
            payload: serde_json::to_value(&payload).unwrap(),
            async_tasks: Default::default(),
        }
    }

    pub fn into_mqtt_messages(
        self,
        reqp: &IncomingRequestProperties,
    ) -> Result<MessageStream, error::Error> {
        let mut notifications = self.notifications;
        if self.status != StatusCode::NO_CONTENT {
            let response = helpers::build_response(
                self.status,
                self.payload,
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

    pub fn add_async_task(&mut self, task: JoinHandle<Message>) {
        self.async_tasks.push(task);
    }
}

impl IntoResponse for Response {
    fn into_response(self) -> axum::response::Response {
        let tasks_stream = Box::new(self.async_tasks.into_stream()) as MessageStream;

        let mut resp = (self.status, Json(self.payload)).into_response();

        resp.extensions_mut().insert(self.notifications);
        resp.extensions_mut().insert(tasks_stream);

        resp
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
