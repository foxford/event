use chrono::Utc;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde_json::json;
use svc_agent::{
    mqtt::{IncomingEventProperties, IncomingRequestProperties},
    AgentId,
};
use uuid::Uuid;

use crate::app::endpoint::{EventHandler, RequestHandler};
use crate::app::error::Error as AppError;
use crate::app::message_handler::MessageStream;
use crate::app::service_utils::RequestParams;
use crate::app::API_VERSION;

use self::agent::TestAgent;
use self::context::TestContext;
use self::outgoing_envelope::{
    OutgoingEnvelope, OutgoingEnvelopeProperties, OutgoingEventProperties,
    OutgoingResponseProperties,
};

///////////////////////////////////////////////////////////////////////////////

pub const SVC_AUDIENCE: &'static str = "dev.svc.example.org";
pub const USR_AUDIENCE: &'static str = "dev.usr.example.org";

pub async fn handle_request<H: RequestHandler>(
    context: &mut TestContext,
    agent: &TestAgent,
    payload: H::Payload,
) -> Result<Vec<OutgoingEnvelope>, AppError> {
    let reqp = build_reqp(agent.agent_id(), "ignore");
    let messages = H::handle(context, payload, RequestParams::MqttParams(&reqp)).await?;
    Ok(parse_messages(messages.into_mqtt_messages(&reqp)?).await)
}

pub async fn handle_event<H: EventHandler>(
    context: &mut TestContext,
    agent: &TestAgent,
    payload: H::Payload,
) -> Result<Vec<OutgoingEnvelope>, AppError> {
    let evp = build_evp(agent.agent_id(), "ignore");
    let messages = H::handle(context, payload, &evp).await?;
    Ok(parse_messages(messages).await)
}

async fn parse_messages(mut messages: MessageStream) -> Vec<OutgoingEnvelope> {
    let mut parsed_messages = vec![];

    while let Some(message) = messages.next().await {
        let dump = message
            .into_dump(TestAgent::new("alpha", "event", SVC_AUDIENCE).address())
            .expect("Failed to dump outgoing message");

        let mut parsed_message = serde_json::from_str::<OutgoingEnvelope>(dump.payload())
            .expect("Failed to parse dumped message");

        parsed_message.set_topic(dump.topic());
        parsed_messages.push(parsed_message);
    }

    parsed_messages
}

pub fn find_event<P>(messages: &[OutgoingEnvelope]) -> (P, &OutgoingEventProperties, &str)
where
    P: DeserializeOwned,
{
    for message in messages {
        if let OutgoingEnvelopeProperties::Event(evp) = message.properties() {
            return (message.payload::<P>(), evp, message.topic());
        }
    }

    panic!("Event not found");
}

pub fn find_event_by_predicate<P, F>(
    messages: &[OutgoingEnvelope],
    f: F,
) -> Option<(P, &OutgoingEventProperties, &str)>
where
    P: DeserializeOwned,
    F: Fn(&OutgoingEventProperties) -> bool,
{
    for message in messages {
        if let OutgoingEnvelopeProperties::Event(evp) = message.properties() {
            if f(evp) {
                return Some((message.payload::<P>(), evp, message.topic()));
            }
        }
    }

    return None;
}

pub fn find_response<P>(messages: &[OutgoingEnvelope]) -> (P, &OutgoingResponseProperties, &str)
where
    P: DeserializeOwned,
{
    for message in messages {
        if let OutgoingEnvelopeProperties::Response(respp) = message.properties() {
            return (message.payload::<P>(), respp, message.topic());
        }
    }

    panic!("Response not found");
}

pub fn build_reqp(agent_id: &AgentId, method: &str) -> IncomingRequestProperties {
    let now = Utc::now().timestamp_millis().to_string();

    let reqp_json = json!({
        "type": "request",
        "correlation_data": "123456789",
        "agent_id": agent_id,
        "connection_mode": "default",
        "connection_version": "v2",
        "method": method,
        "response_topic": format!(
            "agents/{}/api/{}/in/event.{}",
            agent_id, API_VERSION, SVC_AUDIENCE
        ),
        "broker_agent_id": format!("alpha.mqtt-gateway.{}", SVC_AUDIENCE),
        "broker_timestamp": now,
        "broker_processing_timestamp": now,
        "broker_initial_processing_timestamp": now,
        "tracking_id": format!("{}.{}.{}", Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()),
        "session_tracking_label": format!(
            "{}.{} {}.{}",
            Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()
        ),
    });

    serde_json::from_value::<IncomingRequestProperties>(reqp_json).expect("Failed to parse reqp")
}

pub fn build_evp(agent_id: &AgentId, label: &str) -> IncomingEventProperties {
    let now = Utc::now().timestamp_millis().to_string();

    let evp_json = json!({
        "type": "event",
        "label": label,
        "agent_id": agent_id,
        "connection_mode": "default",
        "connection_version": "v2",
        "broker_agent_id": format!("alpha.mqtt-gateway.{}", SVC_AUDIENCE),
        "broker_timestamp": now,
        "broker_processing_timestamp": now,
        "broker_initial_processing_timestamp": now,
        "tracking_id": format!("{}.{}.{}", Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()),
        "session_tracking_label": format!(
            "{}.{} {}.{}",
            Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()
        ),
    });

    serde_json::from_value::<IncomingEventProperties>(evp_json).expect("Failed to parse evp")
}

///////////////////////////////////////////////////////////////////////////////

pub mod prelude {
    #[allow(unused_imports)]
    pub use crate::app::context::GlobalContext;

    #[allow(unused_imports)]
    pub use super::{
        agent::TestAgent,
        authz::{DbBanTestAuthz, TestAuthz},
        build_evp, build_reqp,
        context::TestContext,
        db::{test_db_ban_callback, TestDb},
        factory, find_event, find_event_by_predicate, find_response, handle_event, handle_request,
        shared_helpers, SVC_AUDIENCE, USR_AUDIENCE,
    };
}

pub mod agent;
pub mod authz;
pub mod context;
pub mod db;
pub mod factory;
pub mod outgoing_envelope;
pub mod shared_helpers;
