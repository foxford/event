use std::sync::{mpsc, Arc};
use std::time::Duration;

use chrono::Utc;
use futures::executor::ThreadPool;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde::{de::DeserializeOwned, ser::Serialize};
use serde_derive::Deserialize;
use serde_json::Value as JsonValue;
use svc_agent::{
    mqtt::{
        compat, Agent, AgentBuilder, AgentConfig, ConnectionMode, IncomingResponse, Notification,
        OutgoingRequest, OutgoingRequestProperties, QoS, ShortTermTimingProperties,
        SubscriptionTopic,
    },
    request::Dispatcher,
    AccountId, AgentId, Subscription,
};

///////////////////////////////////////////////////////////////////////////////

const API_VERSION: &'static str = "v1";
const CORRELATION_DATA_LENGTH: usize = 16;
const IGNORE: &'static str = "ignore";

#[derive(Deserialize)]
struct IgnoreResponse {}

pub struct TestAgent {
    agent: Agent,
    dispatcher: Arc<Dispatcher>,
    pool: Arc<ThreadPool>,
    service_account_id: AccountId,
    inbox_topic: String,
}

impl TestAgent {
    pub fn start(
        config: &AgentConfig,
        id: AgentId,
        pool: Arc<ThreadPool>,
        service_account_id: &AccountId,
    ) -> Self {
        // Start agent.
        let (mut agent, rx) = AgentBuilder::new(id.clone(), API_VERSION)
            .connection_mode(ConnectionMode::Service)
            .start(&config)
            .expect("Failed to start agent");

        // Subscribe to the service responses.
        let subscription = Subscription::unicast_responses_from(service_account_id);

        agent
            .subscribe(&subscription, QoS::AtLeastOnce, None)
            .expect("Error subscribing to unicast responses");

        let inbox_topic = subscription
            .subscription_topic(&id, API_VERSION)
            .expect("Failed to build response topic");

        // Run request dispatcher.
        let dispatcher = Arc::new(Dispatcher::new(&agent));
        let dispatcher_clone = dispatcher.clone();

        pool.spawn_ok(async move {
            while let Ok(Notification::Publish(message)) = rx.recv() {
                let bytes = message.payload.as_slice();

                let envelope = serde_json::from_slice::<compat::IncomingEnvelope>(bytes)
                    .expect("Failed to parse incoming message");

                if let compat::IncomingEnvelopeProperties::Response(props) = envelope.properties() {
                    if props.correlation_data() != IGNORE {
                        match compat::into_response::<JsonValue>(envelope) {
                            Err(err) => panic!("Failed to parse response: {}", err),
                            Ok(response) => dispatcher_clone
                                .response(response)
                                .await
                                .expect("Failed to dispatch response"),
                        }
                    }
                }
            }
        });

        Self {
            agent,
            dispatcher,
            pool,
            service_account_id: service_account_id.to_owned(),
            inbox_topic,
        }
    }

    pub fn request<Req, Resp>(&self, method: &str, payload: Req) -> IncomingResponse<Resp>
    where
        Req: 'static + Send + Serialize,
        Resp: 'static + Send + DeserializeOwned,
    {
        let correlation_data = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(CORRELATION_DATA_LENGTH)
            .collect::<String>();

        let request = self.build_request(method, &correlation_data, payload);
        let (resp_tx, resp_rx) = mpsc::channel::<IncomingResponse<Resp>>();
        let dispatcher = self.dispatcher.clone();

        self.pool.spawn_ok(async move {
            let response = dispatcher
                .request::<Req, Resp>(request)
                .await
                .expect("Failed to dispatch request");

            resp_tx
                .send(response)
                .expect("Failed to notify about received response");
        });

        resp_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("Failed to await response")
    }

    pub fn request_nowait<Req>(&self, method: &str, payload: Req)
    where
        Req: 'static + Send + Serialize,
    {
        self.agent
            .clone()
            .publish(Box::new(self.build_request(method, IGNORE, payload)))
            .expect("Failed to publish request");
    }

    fn build_request<P: Serialize>(
        &self,
        method: &str,
        correlation_data: &str,
        payload: P,
    ) -> OutgoingRequest<P> {
        let reqp = OutgoingRequestProperties::new(
            method,
            &self.inbox_topic,
            correlation_data,
            ShortTermTimingProperties::new(Utc::now()),
        );

        OutgoingRequest::multicast(payload, reqp, &self.service_account_id)
    }
}
