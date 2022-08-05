use std::convert::TryInto;

use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use reqwest::{header, Url};
use serde::{Deserialize, Serialize};
use svc_agent::AgentId;
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
