use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use futures::executor::ThreadPool;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde::{de::DeserializeOwned, ser::Serialize};
use serde_derive::Deserialize;
use serde_json::Value as JsonValue;
use svc_agent::{
    mqtt::{
        compat, Agent, AgentBuilder, AgentConfig, ConnectionMode, IncomingEvent, IncomingResponse,
        Notification, OutgoingRequest, OutgoingRequestProperties, QoS, ShortTermTimingProperties,
        SubscriptionTopic,
    },
    request::Dispatcher as RequestDispatcher,
    AccountId, AgentId, Subscription,
};

///////////////////////////////////////////////////////////////////////////////

const API_VERSION: &'static str = "v1";
const CORRELATION_DATA_LENGTH: usize = 16;
const IGNORE: &'static str = "ignore";

#[derive(Deserialize)]
struct IgnoreResponse {}

pub struct TestAgent {
    id: AgentId,
    agent: Agent,
    request_dispatcher: Arc<RequestDispatcher>,
    event_dispatcher: Arc<EventDispatcher>,
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
            .connection_mode(ConnectionMode::Default)
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

        // Run dispatchers.
        let request_dispatcher = Arc::new(RequestDispatcher::new(&agent));
        let request_dispatcher_clone = request_dispatcher.clone();

        let event_dispatcher = Arc::new(EventDispatcher::new());
        let event_dispatcher_clone = event_dispatcher.clone();

        pool.spawn_ok(async move {
            while let Ok(Notification::Publish(message)) = rx.recv() {
                let bytes = message.payload.as_slice();

                let envelope = serde_json::from_slice::<compat::IncomingEnvelope>(bytes)
                    .expect("Failed to parse incoming message");

                match envelope.properties() {
                    compat::IncomingEnvelopeProperties::Request(_) => (),
                    compat::IncomingEnvelopeProperties::Response(respp) => {
                        if respp.correlation_data() != IGNORE {
                            match compat::into_response::<JsonValue>(envelope) {
                                Err(err) => panic!("Failed to parse response: {}", err),
                                Ok(response) => request_dispatcher_clone
                                    .response(response)
                                    .await
                                    .expect("Failed to dispatch response"),
                            }
                        }
                    }
                    compat::IncomingEnvelopeProperties::Event(_) => {
                        event_dispatcher_clone.dispatch(envelope);
                    }
                }
            }
        });

        Self {
            id,
            agent,
            request_dispatcher,
            event_dispatcher,
            pool,
            service_account_id: service_account_id.to_owned(),
            inbox_topic,
        }
    }

    pub fn id(&self) -> &AgentId {
        &self.id
    }

    /// Publish a request and wait for the response.
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
        let dispatcher = self.request_dispatcher.clone();

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

    /// Publish a request but don't wait for the response.
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
        let mut reqp = OutgoingRequestProperties::new(
            method,
            &self.inbox_topic,
            correlation_data,
            ShortTermTimingProperties::new(Utc::now()),
        );

        reqp.set_local_timestamp(Utc::now());
        OutgoingRequest::multicast(payload, reqp, &self.service_account_id)
    }

    /// Blocks until receiving an event with `label` that matches `predicate`.
    pub fn wait_for_event<P, F>(&self, label: &str, predicate: F) -> IncomingEvent<P>
    where
        P: DeserializeOwned,
        F: Fn(&IncomingEvent<P>) -> bool,
    {
        self.event_dispatcher.wait::<P, F>(label, predicate)
    }
}

///////////////////////////////////////////////////////////////////////////////

struct EventDispatcher {
    tx: Arc<Mutex<mpsc::Sender<compat::IncomingEnvelope>>>,
    rx: Arc<Mutex<mpsc::Receiver<compat::IncomingEnvelope>>>,
    is_waiting: AtomicBool,
}

impl EventDispatcher {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel::<compat::IncomingEnvelope>();

        Self {
            tx: Arc::new(Mutex::new(tx)),
            rx: Arc::new(Mutex::new(rx)),
            is_waiting: AtomicBool::new(false),
        }
    }

    fn wait<P, F>(&self, label: &str, predicate: F) -> IncomingEvent<P>
    where
        P: DeserializeOwned,
        F: Fn(&IncomingEvent<P>) -> bool,
    {
        let rx = self
            .rx
            .lock()
            .expect("Failed to obtain channel receiver lock");

        // Start receiving messages.
        self.is_waiting.store(true, Ordering::SeqCst);

        // Receive messages until facing an event with the expected label.
        while let Ok(envelope) = rx.recv() {
            if let compat::IncomingEnvelopeProperties::Event(evp) = envelope.properties() {
                // Skip messages with unexpected label.
                if evp.label() != Some(label) {
                    continue;
                }

                // Parse the event payload.
                let event = compat::into_event::<P>(envelope).expect("Failed to parse event");

                // Run caller-defined predicate to check whether it's the expected event or
                // we should wait for another one.
                if !predicate(&event) {
                    continue;
                }

                // Stop receiving messages.
                self.is_waiting.store(false, Ordering::SeqCst);

                // Read the rest of the queue to avoid memory leak.
                while let Ok(_) = rx.try_recv() {}

                // Return the parsed event.
                return event;
            }
        }

        panic!("Failed to wait for `{}` event. Channel closed.", label);
    }

    fn dispatch(&self, envelope: compat::IncomingEnvelope) {
        // Push message to the channel only if the other thread is waiting to avoid memory leak.
        if self.is_waiting.load(Ordering::SeqCst) {
            let tx = self
                .tx
                .lock()
                .expect("Failed to obtain channel sender lock");
            tx.send(envelope).expect("Failed to dispatch event");
        }
    }
}
