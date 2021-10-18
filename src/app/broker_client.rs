use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
#[cfg(test)]
use mockall::automock;
use serde_derive::{Deserialize, Serialize};
use svc_agent::{
    error::Error as AgentError,
    mqtt::{
        IncomingResponse, OutgoingMessage, OutgoingRequest, OutgoingRequestProperties,
        ShortTermTimingProperties, SubscriptionTopic,
    },
    request::Dispatcher,
    AccountId, AgentId, Subscription,
};
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct SubscriptionRequest {
    subject: AgentId,
    object: Vec<String>,
}

impl SubscriptionRequest {
    fn new(subject: AgentId, object: Vec<String>) -> Self {
        Self { subject, object }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateDeleteResponsePayload {}

#[cfg_attr(test, automock)]
#[async_trait]
pub trait BrokerClient: Sync + Send {
    async fn enter_room(
        &self,
        id: Uuid,
        subject: &AgentId,
    ) -> Result<IncomingResponse<CreateDeleteResponsePayload>, anyhow::Error>;
    async fn enter_broadcast_room(
        &self,
        id: Uuid,
        subject: &AgentId,
    ) -> Result<IncomingResponse<CreateDeleteResponsePayload>, anyhow::Error>;
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
    ) -> Result<IncomingResponse<CreateDeleteResponsePayload>, anyhow::Error> {
        self.subscription_request(id, subject, "subscription.create")
            .await
    }

    async fn enter_broadcast_room(
        &self,
        id: Uuid,
        subject: &AgentId,
    ) -> Result<IncomingResponse<CreateDeleteResponsePayload>, anyhow::Error> {
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
    ) -> Result<IncomingResponse<CreateDeleteResponsePayload>, anyhow::Error> {
        let reqp = self.build_reqp(method)?;
        let object = vec!["rooms".to_string(), id.to_string(), "events".to_string()];

        let payload = SubscriptionRequest::new(subject.clone(), object);

        let msg = if let OutgoingMessage::Request(msg) =
            OutgoingRequest::multicast(payload, reqp, &self.broker_account_id, &self.api_version)
        {
            msg
        } else {
            unreachable!()
        };

        let request = self
            .dispatcher
            .request::<_, CreateDeleteResponsePayload>(msg);
        let payload_result = if let Some(dur) = self.timeout {
            tokio::time::timeout(dur, request)
                .await
                .map_err(|_e| anyhow!("Mqtt request timed out"))?
        } else {
            request.await
        };

        payload_result.map_err(|e| anyhow!("Mqtt request failed, err = {:?}", e))
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
