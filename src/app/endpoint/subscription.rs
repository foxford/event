use std::result::Result as StdResult;

use async_std::stream;
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
use crate::app::endpoint::prelude::*;
use crate::db::{agent, room};

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct SubscriptionEvent {
    subject: AgentId,
    object: Vec<String>,
}

impl SubscriptionEvent {
    fn try_room_id(&self) -> StdResult<Uuid, SvcError> {
        let object: Vec<&str> = self.object.iter().map(AsRef::as_ref).collect();

        match object.as_slice() {
            ["rooms", room_id, "events"] => {
                Uuid::parse_str(room_id).map_err(|err| format!("UUID parse error: {}", err))
            }
            _ => Err(String::from(
                "Bad 'object' format; expected [\"room\", <ROOM_ID>, \"events\"]",
            )),
        }
        .status(ResponseStatus::BAD_REQUEST)
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
    ) -> Result {
        // Check if the event is sent by the broker.
        if evp.as_account_id() != &context.config().broker_id {
            return Err(format!(
                "Expected subscription.create event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                evp.as_account_id()
            )).status(ResponseStatus::FORBIDDEN);
        }

        // Find room.
        let room_id = payload.try_room_id()?;
        let conn = context.db().get()?;

        room::FindQuery::new(room_id)
            .time(room::now())
            .execute(&conn)?
            .ok_or_else(|| format!("the room = '{}' is not found or closed", room_id))
            .status(ResponseStatus::NOT_FOUND)?;

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
        let boxed_event = Box::new(outgoing_event) as Box<dyn IntoPublishableDump + Send>;
        Ok(Box::new(stream::once(boxed_event)))
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
    ) -> Result {
        // Check if the event is sent by the broker.
        if evp.as_account_id() != &context.config().broker_id {
            return Err(format!(
                "Expected subscription.delete event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                evp.as_account_id()
            )).status(ResponseStatus::FORBIDDEN);
        }

        // Delete agent from the DB.
        let room_id = payload.try_room_id()?;
        let conn = context.db().get()?;
        let row_count = agent::DeleteQuery::new(&payload.subject, room_id).execute(&conn)?;

        if row_count != 1 {
            return Err(format!(
                "the agent is not found for agent_id = '{}', room = '{}'",
                payload.subject, room_id
            ))
            .status(ResponseStatus::NOT_FOUND);
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
        let boxed_event = Box::new(outgoing_event) as Box<dyn IntoPublishableDump + Send>;
        Ok(Box::new(stream::once(boxed_event)))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use crate::db::agent::{ListQuery as AgentListQuery, Status as AgentStatus};
    use crate::test_helpers::prelude::*;

    use super::*;

    ///////////////////////////////////////////////////////////////////////////

    #[test]
    fn create_subscription() {
        futures::executor::block_on(async {
            let db = TestDb::new();
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                let conn = db
                    .connection_pool()
                    .get()
                    .expect("Failed to get DB connection");

                // Create room.
                let room = shared_helpers::insert_room(&conn);

                // Put agent in the room in `in_progress` status.
                factory::Agent::new()
                    .room_id(room.id())
                    .agent_id(agent.agent_id().to_owned())
                    .insert(&conn);

                room
            };

            // Send subscription.create event.
            let context = TestContext::new(db.clone(), TestAuthz::new());
            let room_id = room.id().to_string();

            let payload = SubscriptionEvent {
                subject: agent.agent_id().to_owned(),
                object: vec!["rooms".to_string(), room_id, "events".to_string()],
            };

            let broker_account_label = context.config().broker_id.label();
            let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

            let messages = handle_event::<CreateHandler>(&context, &broker, payload)
                .await
                .expect("Subscription creation failed");

            // Assert notification.
            let (payload, evp, topic) = find_event::<RoomEnterLeaveEvent>(messages.as_slice());
            assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
            assert_eq!(evp.label(), "room.enter");
            assert_eq!(payload.id, room.id());
            assert_eq!(&payload.agent_id, agent.agent_id());

            // Assert agent turned to `ready` status.
            let conn = db
                .connection_pool()
                .get()
                .expect("Failed to get DB connection");

            let db_agents = AgentListQuery::new()
                .agent_id(agent.agent_id())
                .room_id(room.id())
                .execute(&conn)
                .expect("Failed to execute agent list query");

            let db_agent = db_agents.first().expect("Missing agent in the DB");
            assert_eq!(db_agent.status(), AgentStatus::Ready);
        });
    }

    #[test]
    fn create_subscription_missing_room() {
        futures::executor::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let db = TestDb::new();
            let context = TestContext::new(db, TestAuthz::new());
            let room_id = Uuid::new_v4().to_string();

            let payload = SubscriptionEvent {
                subject: agent.agent_id().to_owned(),
                object: vec!["rooms".to_string(), room_id, "events".to_string()],
            };

            let broker_account_label = context.config().broker_id.label();
            let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

            let err = handle_event::<CreateHandler>(&context, &broker, payload)
                .await
                .expect_err("Unexpected success on subscription creation");

            assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
        });
    }

    #[test]
    fn create_subscription_closed_room() {
        futures::executor::block_on(async {
            let db = TestDb::new();

            let room = {
                let conn = db
                    .connection_pool()
                    .get()
                    .expect("Failed to get DB connection");

                shared_helpers::insert_closed_room(&conn)
            };

            let context = TestContext::new(db, TestAuthz::new());
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let room_id = room.id().to_string();

            let payload = SubscriptionEvent {
                subject: agent.agent_id().to_owned(),
                object: vec!["rooms".to_string(), room_id, "events".to_string()],
            };

            let broker_account_label = context.config().broker_id.label();
            let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

            let err = handle_event::<CreateHandler>(&context, &broker, payload)
                .await
                .expect_err("Unexpected success on subscription creation");

            assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
        });
    }

    ///////////////////////////////////////////////////////////////////////////

    #[test]
    fn delete_subscription() {
        futures::executor::block_on(async {
            let db = TestDb::new();
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                let conn = db
                    .connection_pool()
                    .get()
                    .expect("Failed to get DB connection");

                // Create room and put the agent online.
                let room = shared_helpers::insert_room(&conn);
                shared_helpers::insert_agent(&conn, agent.agent_id(), room.id());
                room
            };

            // Send subscription.delete event.
            let context = TestContext::new(db.clone(), TestAuthz::new());
            let room_id = room.id().to_string();

            let payload = SubscriptionEvent {
                subject: agent.agent_id().to_owned(),
                object: vec!["rooms".to_string(), room_id, "events".to_string()],
            };

            let broker_account_label = context.config().broker_id.label();
            let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

            let messages = handle_event::<DeleteHandler>(&context, &broker, payload)
                .await
                .expect("Subscription deletion failed");

            // Assert notification.
            let (payload, evp, topic) = find_event::<RoomEnterLeaveEvent>(messages.as_slice());
            assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
            assert_eq!(evp.label(), "room.leave");
            assert_eq!(payload.id, room.id());
            assert_eq!(&payload.agent_id, agent.agent_id());

            // Assert agent deleted from the DB.
            let conn = db
                .connection_pool()
                .get()
                .expect("Failed to get DB connection");

            let db_agents = AgentListQuery::new()
                .agent_id(agent.agent_id())
                .room_id(room.id())
                .execute(&conn)
                .expect("Failed to execute agent list query");

            assert_eq!(db_agents.len(), 0);
        });
    }

    #[test]
    fn delete_subscription_missing_agent() {
        futures::executor::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let db = TestDb::new();

            let room = {
                let conn = db
                    .connection_pool()
                    .get()
                    .expect("Failed to get DB connection");

                shared_helpers::insert_room(&conn)
            };

            let context = TestContext::new(db.clone(), TestAuthz::new());
            let room_id = room.id().to_string();

            let payload = SubscriptionEvent {
                subject: agent.agent_id().to_owned(),
                object: vec!["rooms".to_string(), room_id, "events".to_string()],
            };

            let broker_account_label = context.config().broker_id.label();
            let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

            let err = handle_event::<DeleteHandler>(&context, &broker, payload)
                .await
                .expect_err("Unexpected success on subscription deletion");

            assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
        });
    }

    #[test]
    fn delete_subscription_missing_room() {
        futures::executor::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let db = TestDb::new();
            let context = TestContext::new(db.clone(), TestAuthz::new());
            let room_id = Uuid::new_v4().to_string();

            let payload = SubscriptionEvent {
                subject: agent.agent_id().to_owned(),
                object: vec!["rooms".to_string(), room_id, "events".to_string()],
            };

            let broker_account_label = context.config().broker_id.label();
            let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

            let err = handle_event::<DeleteHandler>(&context, &broker, payload)
                .await
                .expect_err("Unexpected success on subscription deletion");

            assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
        });
    }
}
