use std::convert::TryInto;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
#[cfg(test)]
use mockall::automock;
use reqwest::{header, Url};
use serde::{Deserialize, Serialize};
use svc_agent::{
    error::Error as AgentError,
    mqtt::{
        OutgoingMessage, OutgoingRequest, OutgoingRequestProperties, ShortTermTimingProperties,
        SubscriptionTopic,
    },
    request::Dispatcher,
    AccountId, AgentId, Subscription,
};
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct SubscriptionRequest {
    subject: AgentId,
    object: Vec<String>,
    version: &'static str,
}

impl SubscriptionRequest {
    fn new(subject: AgentId, object: Vec<String>) -> Self {
        Self {
            subject,
            object,
            version: "v1",
        }
    }

    fn room_events(subject: &AgentId, room_id: Uuid) -> Self {
        let object = vec![
            "rooms".to_string(),
            room_id.to_string(),
            "events".to_string(),
        ];
        Self::new(subject.clone(), object)
    }
}

#[derive(Debug)]
pub enum CreateDeleteResponse {
    Ok,
    ClientDisconnected,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
enum BrokerResponsePayload {
    Err { reason: String },
    Ok {},
}

#[cfg(test)]
mod payload_test {
    #[test]
    fn parse_err() {
        let sample = super::BrokerResponsePayload::Err {
            reason: "Subject was not in vmq_subscriber_db".to_string(),
        };
        let payload = serde_json::from_str::<super::BrokerResponsePayload>(
            "{\"reason\":\"Subject was not in vmq_subscriber_db\"}",
        )
        .unwrap();

        assert_eq!(payload, sample);
    }

    #[test]
    fn parse_ok() {
        serde_json::from_str::<super::BrokerResponsePayload>("{}").unwrap();
    }
}

#[cfg_attr(test, automock)]
#[async_trait]
pub trait BrokerClient: Sync + Send {
    async fn enter_room(&self, id: Uuid, subject: &AgentId)
        -> anyhow::Result<CreateDeleteResponse>;
    async fn enter_broadcast_room(
        &self,
        id: Uuid,
        subject: &AgentId,
    ) -> anyhow::Result<CreateDeleteResponse>;
}

pub struct MqttBrokerClient {
    me: AgentId,
    broker_account_id: AccountId,
    dispatcher: Arc<Dispatcher>,
    timeout: Option<Duration>,
    api_version: String,
}

#[async_trait]
impl BrokerClient for MqttBrokerClient {
    async fn enter_room(
        &self,
        id: Uuid,
        subject: &AgentId,
    ) -> anyhow::Result<CreateDeleteResponse> {
        self.subscription_request(id, subject, "subscription.create")
            .await
    }

    async fn enter_broadcast_room(
        &self,
        id: Uuid,
        subject: &AgentId,
    ) -> anyhow::Result<CreateDeleteResponse> {
        self.subscription_request(id, subject, "broadcast_subscription.create")
            .await
    }
}

impl MqttBrokerClient {
    pub fn new(
        me: AgentId,
        broker_account_id: AccountId,
        dispatcher: Arc<Dispatcher>,
        timeout: Option<Duration>,
        api_version: &str,
    ) -> Self {
        Self {
            me,
            broker_account_id,
            dispatcher,
            timeout,
            api_version: api_version.to_string(),
        }
    }

    async fn subscription_request(
        &self,
        id: Uuid,
        subject: &AgentId,
        method: &str,
    ) -> anyhow::Result<CreateDeleteResponse> {
        let reqp = self.build_reqp(method)?;

        let payload = SubscriptionRequest::room_events(subject, id);

        let msg = if let OutgoingMessage::Request(msg) =
            OutgoingRequest::multicast(payload, reqp, &self.broker_account_id, &self.api_version)
        {
            msg
        } else {
            unreachable!()
        };

        let request = self.dispatcher.request::<_, BrokerResponsePayload>(msg);
        let response = if let Some(dur) = self.timeout {
            tokio::time::timeout(dur, request)
                .await
                .map_err(|_e| anyhow!("Mqtt request timed out"))?
        } else {
            request.await
        };

        let response = response.map_err(|e| anyhow!("Mqtt request failed, err = {:?}", e))?;
        match response.properties().status() {
            svc_agent::mqtt::ResponseStatus::OK => Ok(CreateDeleteResponse::Ok),
            status => match response.extract_payload() {
                BrokerResponsePayload::Err { reason }
                    if reason == "Subject was not in vmq_subscriber_db" =>
                {
                    // Client has already disconnected from broker
                    // We will return him an error but he probably already cancelled the http request too
                    Ok(CreateDeleteResponse::ClientDisconnected)
                }
                payload => Err(anyhow!(
                    "Mqtt request failed with status code = {:?}, payload = {:?}",
                    status,
                    payload
                )),
            },
        }
    }

    fn response_topic(&self) -> Result<String, anyhow::Error> {
        let me = self.me.clone();

        Subscription::unicast_responses_from(&self.broker_account_id)
            .subscription_topic(&me, &self.api_version)
            .map_err(|e| AgentError::new(&e.to_string()).into())
    }

    fn build_reqp(&self, method: &str) -> Result<OutgoingRequestProperties, anyhow::Error> {
        let reqp = OutgoingRequestProperties::new(
            method,
            &self.response_topic()?,
            &generate_correlation_data(),
            ShortTermTimingProperties::new(Utc::now()),
        );

        Ok(reqp)
    }
}

const CORRELATION_DATA_LENGTH: usize = 16;
use rand::{distributions::Alphanumeric, thread_rng, Rng};

fn generate_correlation_data() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(CORRELATION_DATA_LENGTH)
        .map(char::from)
        .collect()
}

#[derive(Debug, Clone)]
pub struct HttpBrokerClient {
    http: reqwest::Client,
    host: Url,
}

impl HttpBrokerClient {
    pub fn new(
        host: &str,
        token: &str,
        timeout: Option<std::time::Duration>,
    ) -> anyhow::Result<Self> {
        let client = {
            let mut builder =
                reqwest::Client::builder().default_headers(Self::default_headers(token));
            if let Some(timeout) = timeout {
                builder = builder.timeout(timeout);
            }
            builder.build()?
        };

        Ok(Self {
            http: client,
            host: host.parse()?,
        })
    }

    fn default_headers(token: &str) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            format!("Bearer {}", token).try_into().unwrap(),
        );
        headers.insert(
            http::header::CONTENT_TYPE,
            "application/json".try_into().unwrap(),
        );
        headers.insert(
            http::header::USER_AGENT,
            format!("event-{}", crate::APP_VERSION).try_into().unwrap(),
        );

        headers
    }
}

#[async_trait]
impl BrokerClient for HttpBrokerClient {
    async fn enter_room(
        &self,
        id: Uuid,
        subject: &AgentId,
    ) -> anyhow::Result<CreateDeleteResponse> {
        let payload =
            serde_json::to_string(&SubscriptionRequest::room_events(subject, id)).unwrap();

        let url = self.host.join("/api/v1/subscriptions").unwrap();
        let response = self.http.post(url).body(payload).send().await?;

        match response.status() {
            http::StatusCode::OK => Ok(CreateDeleteResponse::Ok),
            status => Err(anyhow!(
                "HTTP request failed with status code = {:?}, payload = {:?}",
                status,
                response.text().await
            )),
        }
    }

    async fn enter_broadcast_room(
        &self,
        _id: Uuid,
        _subject: &AgentId,
    ) -> anyhow::Result<CreateDeleteResponse> {
        Ok(CreateDeleteResponse::Ok)
    }
}
