use std::sync::Arc;

use axum::response::IntoResponse;
use chrono::{DateTime, Duration, Utc};
use futures::{future, stream, Stream, StreamExt};
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
use crate::app::message_handler::{Message, MessageStream};

use super::{error, notification_puller::NatsIds, topic::Topic};

#[derive(Clone)]
pub struct Notification {
    inner: Arc<NotificationInner>,
}

impl Notification {
    pub fn new(
        label: &'static str,
        topic: Topic,
        payload: impl Serialize + Send + Sync + 'static,
        start_timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            inner: Arc::new(NotificationInner {
                label,
                topic,
                start_timestamp,
                payload: serde_json::to_value(&payload).unwrap(),
            }),
        }
    }

    pub fn mqtt_path(&self) -> String {
        self.inner.topic.to_string()
    }

    pub fn payload(&self) -> &Value {
        &self.inner.payload
    }

    pub fn label(&self) -> &'static str {
        self.inner.label
    }
}

struct NotificationInner {
    start_timestamp: DateTime<Utc>,
    label: &'static str,
    topic: Topic,
    payload: Value,
}

impl From<Notification> for Message {
    fn from(n: Notification) -> Self {
        let timing = ShortTermTimingProperties::until_now(n.inner.start_timestamp);
        let props = OutgoingEventProperties::new(n.inner.label, timing);
        Box::new(OutgoingEvent::broadcast(
            n.inner.payload.clone(),
            props,
            &n.mqtt_path(),
        )) as Self
    }
}

#[derive(Default)]
pub struct Notifications(Vec<Notification>);

impl Notifications {
    fn into_stream(self) -> impl Stream<Item = Notification> {
        stream::iter(self.into_iter())
    }

    fn push(&mut self, msg: Notification) {
        self.0.push(msg);
    }
}

impl IntoIterator for Notifications {
    type Item = <Vec<Notification> as IntoIterator>::Item;
    type IntoIter = <Vec<Notification> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[derive(Default)]
pub struct AsyncTasks(Vec<JoinHandle<Notification>>);

impl AsyncTasks {
    fn into_stream(self) -> impl Stream<Item = Notification> {
        stream::iter(self.0)
            .flat_map(stream::once)
            .filter_map(|jh_output| {
                if let Err(e) = &jh_output {
                    error!(err = ?e, "Failed to await async task, join handle error");
                }
                future::ready(jh_output.ok())
            })
    }

    fn push(&mut self, msg: JoinHandle<Notification>) {
        self.0.push(msg);
    }
}

impl IntoIterator for AsyncTasks {
    type Item = <Vec<JoinHandle<Notification>> as IntoIterator>::Item;
    type IntoIter = <Vec<JoinHandle<Notification>> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

pub struct Response {
    notifications: Notifications,
    status: StatusCode,
    start_timestamp: DateTime<Utc>,
    authz_time: Option<Duration>,
    payload: Value,
    async_tasks: AsyncTasks,
    nats_ids: NatsIds,
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
            nats_ids: NatsIds::new(),
        }
    }

    pub fn into_mqtt_messages(
        self,
        reqp: &IncomingRequestProperties,
    ) -> Result<MessageStream, error::Error> {
        let notifications_stream = self.notifications.into_stream().map(move |n| n.into());

        let async_tasks_stream = self.async_tasks.into_stream().map(move |n| n.into());

        let stream = if self.status != StatusCode::NO_CONTENT {
            let response = helpers::build_response(
                self.status,
                self.payload,
                reqp,
                self.start_timestamp,
                self.authz_time,
            );

            let stream = stream::once(futures::future::ready(response))
                .chain(notifications_stream)
                .chain(async_tasks_stream);

            Box::new(stream) as MessageStream
        } else {
            let stream = notifications_stream.chain(async_tasks_stream);

            Box::new(stream) as MessageStream
        };

        Ok(stream)
    }

    pub fn add_notification(
        &mut self,
        label: &'static str,
        topic: Topic,
        payload: impl Serialize + Send + Sync + 'static,
        start_timestamp: DateTime<Utc>,
    ) {
        self.notifications
            .push(Notification::new(label, topic, payload, start_timestamp));
    }

    pub fn add_async_task(&mut self, task: JoinHandle<Notification>) {
        self.async_tasks.push(task);
    }

    pub fn set_nats_ids(&mut self, ids: NatsIds) {
        self.nats_ids = ids;
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
            .extension(self.nats_ids)
            .body(axum::body::Body::from(
                serde_json::to_string(&self.payload).unwrap(),
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
