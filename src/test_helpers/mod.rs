use chrono::Utc;
use serde::de::DeserializeOwned;
use serde_json::json;
use svc_agent::mqtt::{IncomingRequestProperties, IntoPublishableDump};

use crate::app::endpoint::RequestHandler;

use self::agent::TestAgent;
use self::context::TestContext;
use self::outgoing_envelope::{
    OutgoingEnvelope, OutgoingEnvelopeProperties, OutgoingEventProperties,
    OutgoingResponseProperties,
};

///////////////////////////////////////////////////////////////////////////////

pub(crate) const SVC_AUDIENCE: &'static str = "dev.svc.example.org";
pub(crate) const USR_AUDIENCE: &'static str = "dev.usr.example.org";

pub(crate) async fn handle_request<H: RequestHandler>(
    context: &TestContext,
    agent: &TestAgent,
    payload: H::Payload,
) -> Vec<OutgoingEnvelope> {
    let agent_id = agent.agent_id().to_string();
    let now = Utc::now().timestamp().to_string();

    let reqp_json = json!({
        "type": "request",
        "correlation_data": "ignore",
        "method": "ignore",
        "agent_id": agent_id,
        "connection_mode": "default",
        "connection_version": "v2",
        "response_topic": format!("agents/{}/api/v1/in/event.{}", agent_id, SVC_AUDIENCE),
        "broker_agent_id": format!("alpha.mqtt-gateway.{}", SVC_AUDIENCE),
        "broker_timestamp": now,
        "broker_processing_timestamp": now,
        "broker_initial_processing_timestamp": now,
        "tracking_id": "16911d40-0b13-11ea-8171-60f81db6d53e.14097484-0c8d-11ea-bb82-60f81db6d53e.147b2994-0c8d-11ea-8933-60f81db6d53e",
        "session_tracking_label": "16cc4294-0b13-11ea-91ae-60f81db6d53e.16ee876e-0b13-11ea-8c32-60f81db6d53e 2565f962-0b13-11ea-9359-60f81db6d53e.25c2b97c-0b13-11ea-9f20-60f81db6d53e",
    });

    let reqp = serde_json::from_value::<IncomingRequestProperties>(reqp_json)
        .expect("Failed to parse reqp");

    let messages = H::handle(context, payload, &reqp, Utc::now())
        .await
        .expect("Failed to handler request");

    parse_messages(messages)
}

fn parse_messages(messages: Vec<Box<dyn IntoPublishableDump>>) -> Vec<OutgoingEnvelope> {
    let mut parsed_messages = Vec::with_capacity(messages.len());

    for message in messages {
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

pub(crate) fn find_event<P>(messages: &[OutgoingEnvelope]) -> (P, &OutgoingEventProperties, &str)
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

pub(crate) fn find_response<P>(messages: &[OutgoingEnvelope]) -> (P, &OutgoingResponseProperties)
where
    P: DeserializeOwned,
{
    for message in messages {
        if let OutgoingEnvelopeProperties::Response(respp) = message.properties() {
            return (message.payload::<P>(), respp);
        }
    }

    panic!("Response not found");
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) mod prelude {
    #[allow(unused_imports)]
    pub(crate) use super::{
        agent::TestAgent, authz::TestAuthz, context::TestContext, db::TestDb, factory, find_event,
        find_response, handle_request, SVC_AUDIENCE, USR_AUDIENCE,
    };
}

pub(crate) mod agent;
pub(crate) mod authz;
pub(crate) mod context;
pub(crate) mod db;
pub(crate) mod factory;
pub(crate) mod outgoing_envelope;
