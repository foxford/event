use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use svc_agent::{
    mqtt::{
        IncomingEventProperties, IntoPublishableDump, OutgoingEvent, ResponseStatus,
        ShortTermTimingProperties,
    },
    AgentId, Authenticable,
};
use svc_error::Error as SvcError;
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::EventHandler;
use crate::db::{agent, room};

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct SubscriptionEvent {
    subject: AgentId,
    object: Vec<String>,
}

impl SubscriptionEvent {
    fn try_room_id(&self) -> Result<Uuid, SvcError> {
        let object: Vec<&str> = self.object.iter().map(AsRef::as_ref).collect();

        match object.as_slice() {
            ["rooms", room_id, "events"] => Uuid::parse_str(room_id).map_err(|err| {
                svc_error!(ResponseStatus::BAD_REQUEST, "UUID parse error: {}", err)
            }),
            _ => Err(svc_error!(
                ResponseStatus::BAD_REQUEST,
                "Bad 'object' format; expected [\"room\", <ROOM_ID>, \"events\"]"
            )),
        }
    }
}

#[derive(Deserialize, Serialize)]
pub(crate) struct RoomEnterLeaveEvent {
    id: Uuid,
    agent_id: AgentId,
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct CreateHandler;

#[async_trait]
impl EventHandler for CreateHandler {
    type Payload = SubscriptionEvent;

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError> {
        // Check if the event is sent by the broker.
        if evp.as_account_id() != &context.config().broker_id {
            return Err(svc_error!(
                ResponseStatus::FORBIDDEN,
                "Expected subscription.create event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                evp.as_account_id()
            ));
        }

        // Find room.
        let room_id = payload.try_room_id()?;
        let conn = context.db().get()?;

        room::FindQuery::new(room_id)
            .time(room::now())
            .execute(&conn)?
            .ok_or_else(|| {
                svc_error!(
                    ResponseStatus::NOT_FOUND,
                    "the room = '{}' is not found or closed",
                    room_id
                )
            })?;

        // Update agent state to `ready`.
        agent::UpdateQuery::new(&payload.subject, room_id)
            .status(agent::Status::Ready)
            .execute(&conn)?;

        // Send broadcast notification that the agent has entered the room.
        let outgoing_event_payload = RoomEnterLeaveEvent {
            id: room_id.to_owned(),
            agent_id: payload.subject,
        };

        let short_term_timing = ShortTermTimingProperties::until_now(start_timestamp);
        let props = evp.to_event("room.enter", short_term_timing);
        let to_uri = format!("rooms/{}/events", room_id);
        let outgoing_event = OutgoingEvent::broadcast(outgoing_event_payload, props, &to_uri);
        Ok(vec![Box::new(outgoing_event)])
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct DeleteHandler;

#[async_trait]
impl EventHandler for DeleteHandler {
    type Payload = SubscriptionEvent;

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError> {
        // Check if the event is sent by the broker.
        if evp.as_account_id() != &context.config().broker_id {
            return Err(svc_error!(
                ResponseStatus::FORBIDDEN,
                "Expected subscription.delete event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                evp.as_account_id()
            ));
        }

        // Delete agent from the DB.
        let room_id = payload.try_room_id()?;
        let conn = context.db().get()?;
        let row_count = agent::DeleteQuery::new(&payload.subject, room_id).execute(&conn)?;

        if row_count != 1 {
            return Err(svc_error!(
                ResponseStatus::NOT_FOUND,
                "the agent is not found for agent_id = '{}', room = '{}'",
                payload.subject,
                room_id
            ));
        }

        // Send broadcast notification that the agent has left the room.
        let outgoing_event_payload = RoomEnterLeaveEvent {
            id: room_id.to_owned(),
            agent_id: payload.subject,
        };

        let short_term_timing = ShortTermTimingProperties::until_now(start_timestamp);
        let props = evp.to_event("room.leave", short_term_timing);
        let to_uri = format!("rooms/{}/events", room_id);
        let outgoing_event = OutgoingEvent::broadcast(outgoing_event_payload, props, &to_uri);
        Ok(vec![Box::new(outgoing_event)])
    }
}
