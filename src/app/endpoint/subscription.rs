use std::result::Result as StdResult;

use anyhow::Context as AnyhowContext;
use async_std::stream;
use async_trait::async_trait;
use serde_derive::{Deserialize, Serialize};
use svc_agent::{
    mqtt::{
        IncomingEventProperties, IntoPublishableMessage, OutgoingEvent, ShortTermTimingProperties,
    },
    AgentId, Authenticable,
};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::db::agent;

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct SubscriptionEvent {
    subject: AgentId,
    object: Vec<String>,
}

impl SubscriptionEvent {
    fn try_room_id(&self) -> StdResult<Uuid, AppError> {
        let object: Vec<&str> = self.object.iter().map(AsRef::as_ref).collect();

        match object.as_slice() {
            ["rooms", room_id, "events"] => {
                Uuid::parse_str(room_id).map_err(|err| anyhow!("UUID parse error: {}", err))
            }
            _ => Err(anyhow!(
                "Bad 'object' format; expected [\"room\", <ROOM_ID>, \"events\"]",
            )),
        }
        .error(AppErrorKind::InvalidSubscriptionObject)
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
        context: &mut C,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
    ) -> Result {
        // Check if the event is sent by the broker.
        if evp.as_account_id() != &context.config().broker_id {
            return Err(anyhow!(
                "Expected subscription.create event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                evp.as_account_id()
            )).error(AppErrorKind::AccessDenied);
        }

        let room_id = payload.try_room_id()?;

        {
            // Find room.
            helpers::find_room(
                context,
                room_id,
                helpers::RoomTimeRequirement::Open,
                evp.label().unwrap_or("about:blank"),
            )
            .await?;

            // Update agent state to `ready`.
            let q = agent::UpdateQuery::new(payload.subject.clone(), room_id)
                .status(agent::Status::Ready);

            let mut conn = context.get_conn().await?;

            context
                .profiler()
                .measure((ProfilerKeys::AgentUpdateQuery, None), q.execute(&mut conn))
                .await
                .with_context(|| {
                    format!(
                        "Failed to put agent into 'ready' status, agent_id = '{}', room_id = '{}'",
                        payload.subject, room_id,
                    )
                })
                .error(AppErrorKind::DbQueryFailed)?;
        }

        // Send broadcast notification that the agent has entered the room.
        let outgoing_event_payload = RoomEnterLeaveEvent {
            id: room_id.to_owned(),
            agent_id: payload.subject,
        };

        let start_timestamp = context.start_timestamp();
        let short_term_timing = ShortTermTimingProperties::until_now(start_timestamp);
        let props = evp.to_event("room.enter", short_term_timing);
        let to_uri = format!("rooms/{}/events", room_id);
        let outgoing_event = OutgoingEvent::broadcast(outgoing_event_payload, props, &to_uri);
        let boxed_event = Box::new(outgoing_event) as Box<dyn IntoPublishableMessage + Send>;
        Ok(Box::new(stream::once(boxed_event)))
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct DeleteHandler;

#[async_trait]
impl EventHandler for DeleteHandler {
    type Payload = SubscriptionEvent;

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
    ) -> Result {
        // Check if the event is sent by the broker.
        if evp.as_account_id() != &context.config().broker_id {
            return Err(anyhow!(
                "Expected subscription.delete event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                evp.as_account_id()
            )).error(AppErrorKind::AccessDenied);
        }

        // Delete agent from the DB.
        let room_id = payload.try_room_id()?;

        let row_count = {
            let query = agent::DeleteQuery::new(payload.subject.clone(), room_id);
            let mut conn = context.get_conn().await?;

            context
                .profiler()
                .measure(
                    (ProfilerKeys::AgentDeleteQuery, None),
                    query.execute(&mut conn),
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to delete agent, agent_id = '{}', room_id = '{}'",
                        payload.subject, room_id,
                    )
                })
                .error(AppErrorKind::DbQueryFailed)?
        };

        if row_count != 1 {
            return Err(anyhow!(
                "the agent is not found for agent_id = '{}', room = '{}'",
                payload.subject,
                room_id
            ))
            .error(AppErrorKind::AgentNotEnteredTheRoom);
        }

        // Send broadcast notification that the agent has left the room.
        let outgoing_event_payload = RoomEnterLeaveEvent {
            id: room_id,
            agent_id: payload.subject,
        };

        let start_timestamp = context.start_timestamp();
        let short_term_timing = ShortTermTimingProperties::until_now(start_timestamp);
        let props = evp.to_event("room.leave", short_term_timing);
        let to_uri = format!("rooms/{}/events", room_id);
        let outgoing_event = OutgoingEvent::broadcast(outgoing_event_payload, props, &to_uri);
        let boxed_event = Box::new(outgoing_event) as Box<dyn IntoPublishableMessage + Send>;
        Ok(Box::new(stream::once(boxed_event)))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use svc_agent::mqtt::ResponseStatus;

    use crate::db::agent::{ListQuery as AgentListQuery, Status as AgentStatus};
    use crate::test_helpers::prelude::*;

    use super::*;

    ///////////////////////////////////////////////////////////////////////////

    #[test]
    fn create_subscription() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                // Put agent in the room in `in_progress` status.
                factory::Agent::new()
                    .room_id(room.id())
                    .agent_id(agent.agent_id().to_owned())
                    .insert(&mut conn)
                    .await;

                room
            };

            // Send subscription.create event.
            let context = TestContext::new(db, TestAuthz::new());
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
            let mut conn = context
                .get_conn()
                .await
                .expect("Failed to get DB connection");

            let db_agents = AgentListQuery::new()
                .agent_id(agent.agent_id().to_owned())
                .room_id(room.id())
                .execute(&mut conn)
                .await
                .expect("Failed to execute agent list query");

            let db_agent = db_agents.first().expect("Missing agent in the DB");
            assert_eq!(db_agent.status(), AgentStatus::Ready);
        });
    }

    #[test]
    fn create_subscription_missing_room() {
        async_std::task::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let context = TestContext::new(TestDb::new().await, TestAuthz::new());
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
            assert_eq!(err.kind(), "room_not_found");
        });
    }

    #[test]
    fn create_subscription_closed_room() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;

            let room = {
                let mut conn = db.get_conn().await;
                shared_helpers::insert_closed_room(&mut conn).await
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
            assert_eq!(err.kind(), "room_closed");
        });
    }

    ///////////////////////////////////////////////////////////////////////////

    #[test]
    fn delete_subscription() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
                room
            };

            // Send subscription.delete event.
            let context = TestContext::new(db, TestAuthz::new());
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
            let mut conn = context
                .get_conn()
                .await
                .expect("Failed to get DB connection");

            let db_agents = AgentListQuery::new()
                .agent_id(agent.agent_id().to_owned())
                .room_id(room.id())
                .execute(&mut conn)
                .await
                .expect("Failed to execute agent list query");

            assert_eq!(db_agents.len(), 0);
        });
    }

    #[test]
    fn delete_subscription_missing_agent() {
        async_std::task::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let db = TestDb::new().await;

            let room = {
                let mut conn = db.get_conn().await;
                shared_helpers::insert_room(&mut conn).await
            };

            let context = TestContext::new(db, TestAuthz::new());
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
            assert_eq!(err.kind(), "agent_not_entered_the_room");
        });
    }

    #[test]
    fn delete_subscription_missing_room() {
        async_std::task::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let context = TestContext::new(TestDb::new().await, TestAuthz::new());
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
            assert_eq!(err.kind(), "agent_not_entered_the_room");
        });
    }
}
