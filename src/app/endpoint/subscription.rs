use std::result::Result as StdResult;

use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use futures::{future, stream};
use serde_derive::{Deserialize, Serialize};
use svc_agent::{
    mqtt::{
        IncomingEventProperties, IncomingRequestProperties, OutgoingEvent,
        ShortTermTimingProperties,
    },
    AgentId, Authenticable,
};
use tracing::{field::display, instrument, warn, Span};
use uuid::Uuid;

use crate::db::agent;
use crate::{
    app::context::Context,
    db::event::{insert_agent_action, AgentAction},
};
use crate::{app::endpoint::prelude::*, db::room};

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CorrelationDataPayload {
    reqp: IncomingRequestProperties,
    subject: AgentId,
    object: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct RoomEnterEvent {
    id: Uuid,
    agent_id: AgentId,
    banned: bool,
    agent: crate::db::agent::AgentWithBan,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct RoomLeaveEvent {
    id: Uuid,
    agent_id: AgentId,
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct DeleteEventPayload {
    subject: AgentId,
    object: Vec<String>,
}

pub(crate) struct DeleteEventHandler;

#[async_trait]
impl EventHandler for DeleteEventHandler {
    type Payload = DeleteEventPayload;

    #[instrument(skip_all, fields(room_id))]
    async fn handle<C: Context + Sync + Send>(
        context: &mut C,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
    ) -> MqttResult {
        // Check if the event is sent by the broker.
        if evp.as_account_id() != &context.config().broker_id {
            return Err(anyhow!(
                "Expected subscription.delete event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                evp.as_account_id()
            )).error(AppErrorKind::AccessDenied);
        }

        // Delete agent from the DB.
        let room_id = try_room_id(&payload.object)?;
        Span::current().record("room_id", &display(room_id));

        let row_count = {
            let query = agent::DeleteQuery::new(payload.subject.clone(), room_id);
            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::AgentDeleteQuery, query.execute(&mut conn))
                .await
                .context("Failed to delete agent")
                .error(AppErrorKind::DbQueryFailed)?
        };

        // Ignore missing agent.
        if row_count != 1 {
            return Ok(Box::new(stream::empty()));
        }
        let mut conn = context.get_conn().await?;
        let room = room::FindQuery::by_id(room_id)
            .execute(&mut conn)
            .await
            .context("Failed to find room")
            .error(AppErrorKind::DbQueryFailed)?;
        if let Some(room) = room {
            context
                .metrics()
                .measure_query(
                    QueryKey::EventInsertQuery,
                    insert_agent_action(&room, AgentAction::Left, &payload.subject, &mut conn),
                )
                .await
                .context("Failed to insert agent action")
                .error(AppErrorKind::DbQueryFailed)?;
        }

        // Send broadcast notification that the agent has left the room.
        let outgoing_event_payload = RoomLeaveEvent {
            id: room_id,
            agent_id: payload.subject,
        };

        let start_timestamp = context.start_timestamp();
        let short_term_timing = ShortTermTimingProperties::until_now(start_timestamp);
        let props = evp.to_event("room.leave", short_term_timing);
        let to_uri = format!("rooms/{room_id}/events");
        let outgoing_event = OutgoingEvent::broadcast(outgoing_event_payload, props, &to_uri);
        let boxed_event = Box::new(outgoing_event) as Box<_>;
        Ok(Box::new(stream::once(future::ready(boxed_event))))
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct BroadcastDeleteEventHandler;

#[async_trait]
impl EventHandler for BroadcastDeleteEventHandler {
    type Payload = DeleteEventPayload;

    #[instrument(skip_all, fields(room_id))]
    async fn handle<C: Context + Send>(
        context: &mut C,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
    ) -> MqttResult {
        // Check if the event is sent by the broker.
        if evp.as_account_id() != &context.config().broker_id {
            return Err(anyhow!(
                "Expected broadcast_subscription.delete event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                evp.as_account_id()
            )).error(AppErrorKind::AccessDenied);
        }

        let room_id = try_room_id(&payload.object)?;
        Span::current().record("room_id", &display(room_id));

        warn!("Broadcast subscription deleted by event");

        Ok(Box::new(stream::empty()))
    }
}

///////////////////////////////////////////////////////////////////////////////

fn try_room_id(object: &[String]) -> StdResult<Uuid, AppError> {
    let object: Vec<&str> = object.iter().map(AsRef::as_ref).collect();

    match object.as_slice() {
        ["rooms", room_id, "events"] => Uuid::parse_str(room_id).context("UUID parse error"),
        _ => Err(anyhow!(
            "Bad 'object' format; expected [\"room\", <ROOM_ID>, \"events\"], got: {:?}",
            object
        )),
    }
    .error(AppErrorKind::InvalidSubscriptionObject)
}

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    mod delete_event {
        use crate::db::agent::ListQuery as AgentListQuery;
        use crate::test_helpers::prelude::*;

        use super::super::*;

        #[tokio::test]
        async fn delete_subscription() {
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
            let mut context = TestContext::new(db, TestAuthz::new());
            let room_id = room.id().to_string();

            let payload = DeleteEventPayload {
                subject: agent.agent_id().to_owned(),
                object: vec!["rooms".to_string(), room_id, "events".to_string()],
            };

            let broker_account_label = context.config().broker_id.label();
            let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

            let messages = handle_event::<DeleteEventHandler>(&mut context, &broker, payload)
                .await
                .expect("Subscription deletion failed");

            // Assert notification.
            let (payload, evp, topic) = find_event::<RoomLeaveEvent>(messages.as_slice());
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
        }

        #[tokio::test]
        async fn delete_subscription_missing_agent() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let db = TestDb::new().await;

            let room = {
                let mut conn = db.get_conn().await;
                shared_helpers::insert_room(&mut conn).await
            };

            let mut context = TestContext::new(db, TestAuthz::new());
            let room_id = room.id().to_string();

            let payload = DeleteEventPayload {
                subject: agent.agent_id().to_owned(),
                object: vec!["rooms".to_string(), room_id, "events".to_string()],
            };

            let broker_account_label = context.config().broker_id.label();
            let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

            let messages = handle_event::<DeleteEventHandler>(&mut context, &broker, payload)
                .await
                .expect("Subscription deletion failed");

            assert!(messages.is_empty());
        }
    }
}
