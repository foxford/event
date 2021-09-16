use std::result::Result as StdResult;

use anyhow::Context as AnyhowContext;
use async_std::stream;
use async_trait::async_trait;
use serde_derive::{Deserialize, Serialize};
use serde_json::json;
use svc_agent::{
    mqtt::{
        IncomingEventProperties, IncomingRequestProperties, IncomingResponseProperties,
        IntoPublishableMessage, OutgoingEvent, ResponseStatus, ShortTermTimingProperties,
    },
    Addressable, AgentId, Authenticable,
};
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

impl CorrelationDataPayload {
    pub(crate) fn new(
        reqp: IncomingRequestProperties,
        subject: AgentId,
        object: Vec<String>,
    ) -> Self {
        Self {
            reqp,
            subject,
            object,
        }
    }
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
pub(crate) struct CreateDeleteResponsePayload {}

pub(crate) struct CreateResponseHandler;

#[async_trait]
impl ResponseHandler for CreateResponseHandler {
    type Payload = CreateDeleteResponsePayload;
    type CorrelationData = CorrelationDataPayload;

    async fn handle<C: Context>(
        context: &mut C,
        _payload: Self::Payload,
        respp: &IncomingResponseProperties,
        corr_data: &Self::CorrelationData,
    ) -> Result {
        // Check if the event is sent by the broker.
        if respp.as_account_id() != &context.config().broker_id {
            return Err(anyhow!(
                "Expected subscription.create event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                respp.as_account_id()
            )).error(AppErrorKind::AccessDenied);
        }

        context.add_logger_tags(o!(
            "agent_label" => respp.as_agent_id().label().to_owned(),
            "account_label" => respp.as_account_id().label().to_owned(),
            "audience" => respp.as_account_id().audience().to_owned(),
        ));

        if respp.status() == ResponseStatus::OK {
            // Parse room id.
            let room_id = try_room_id(&corr_data.object)?;
            context.add_logger_tags(o!("room_id" => room_id.to_string()));

            // Determine whether the agent is banned.
            let agent_with_ban = {
                // Find room.
                helpers::find_room(context, room_id, helpers::RoomTimeRequirement::Open).await?;

                // Update agent state to `ready`.
                let q = agent::UpdateQuery::new(corr_data.subject.clone(), room_id)
                    .status(agent::Status::Ready);

                let mut conn = context.get_conn().await?;

                context
                    .metrics()
                    .measure_query(QueryKey::AgentUpdateQuery, q.execute(&mut conn))
                    .await
                    .context("Failed to put agent into 'ready' status")
                    .error(AppErrorKind::DbQueryFailed)?;

                let query = agent::FindWithBanQuery::new(corr_data.subject.clone(), room_id);

                context
                    .metrics()
                    .measure_query(QueryKey::AgentFindWithBanQuery, query.execute(&mut conn))
                    .await
                    .context("Failed to find agent with ban")
                    .error(AppErrorKind::DbQueryFailed)?
                    .ok_or_else(|| anyhow!("No agent {} in room {}", corr_data.subject, room_id))
                    .error(AppErrorKind::AgentNotEnteredTheRoom)?
            };

            let banned = agent_with_ban.banned().unwrap_or(false);

            // Send a response to the original `room.enter` request and a room-wide notification.
            let response = helpers::build_response(
                ResponseStatus::OK,
                json!({}),
                &corr_data.reqp,
                context.start_timestamp(),
                None,
            );

            let notification = helpers::build_notification(
                "room.enter",
                &format!("rooms/{}/events", room_id),
                RoomEnterEvent {
                    id: room_id,
                    agent_id: corr_data.subject.to_owned(),
                    agent: agent_with_ban,
                    banned,
                },
                &corr_data.reqp,
                context.start_timestamp(),
            );

            Ok(Box::new(stream::from_iter(vec![response, notification])))
        } else {
            match respp.status() {
                ResponseStatus::NOT_FOUND => Err(anyhow!("Subscriber was absent in db"))
                    .error(AppErrorKind::BrokerRequestFailed),
                ResponseStatus::UNPROCESSABLE_ENTITY => {
                    Err(anyhow!("Subscriber was present on multiple nodes"))
                        .error(AppErrorKind::BrokerRequestFailed)
                }
                code => Err(anyhow!(format!("Something went wrong, code = {:?}", code)))
                    .error(AppErrorKind::BrokerRequestFailed),
            }
        }
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct BroadcastCreateResponseHandler;

#[async_trait]
impl ResponseHandler for BroadcastCreateResponseHandler {
    type Payload = CreateDeleteResponsePayload;
    type CorrelationData = CorrelationDataPayload;

    async fn handle<C: Context>(
        context: &mut C,
        _payload: Self::Payload,
        respp: &IncomingResponseProperties,
        _corr_data: &Self::CorrelationData,
    ) -> Result {
        // Check if the event is sent by the broker.
        if respp.as_account_id() != &context.config().broker_id {
            return Err(anyhow!(
                "Expected broadcast_subscription.create event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                respp.as_account_id()
            )).error(AppErrorKind::AccessDenied);
        }

        context.add_logger_tags(o!(
            "agent_label" => respp.as_agent_id().label().to_owned(),
            "account_label" => respp.as_account_id().label().to_owned(),
            "audience" => respp.as_account_id().audience().to_owned(),
        ));

        warn!(context.logger(), "Broadcast subscription created");

        Ok(Box::new(stream::empty()))
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct DeleteResponseHandler;

#[async_trait]
impl ResponseHandler for DeleteResponseHandler {
    type Payload = CreateDeleteResponsePayload;
    type CorrelationData = CorrelationDataPayload;

    async fn handle<C: Context>(
        context: &mut C,
        _payload: Self::Payload,
        respp: &IncomingResponseProperties,
        corr_data: &Self::CorrelationData,
    ) -> Result {
        // Check if the event is sent by the broker.
        if respp.as_account_id() != &context.config().broker_id {
            return Err(anyhow!(
                "Expected subscription.delete event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                respp.as_account_id()
            )).error(AppErrorKind::AccessDenied);
        }

        // Parse room id.
        let room_id = try_room_id(&corr_data.object)?;
        context.add_logger_tags(o!("room_id" => room_id.to_string()));

        // Determine whether agent is active and banned and delete it from the db.
        let row_count = {
            let query = agent::DeleteQuery::new(corr_data.subject.clone(), room_id);
            let mut conn = context.get_conn().await?;

            let row_count = context
                .metrics()
                .measure_query(QueryKey::AgentDeleteQuery, query.execute(&mut conn))
                .await
                .context("Failed to delete agent")
                .error(AppErrorKind::DbQueryFailed)?;

            row_count
        };

        if row_count != 1 {
            return Err(anyhow!("The agent is not found"))
                .error(AppErrorKind::AgentNotEnteredTheRoom);
        }

        // Send a response to the original `room.enter` request and a room-wide notification.
        let response = helpers::build_response(
            ResponseStatus::OK,
            json!({}),
            &corr_data.reqp,
            context.start_timestamp(),
            None,
        );

        let notification = helpers::build_notification(
            "room.leave",
            &format!("rooms/{}/events", room_id),
            RoomLeaveEvent {
                id: room_id,
                agent_id: corr_data.subject.to_owned(),
            },
            &corr_data.reqp,
            context.start_timestamp(),
        );

        Ok(Box::new(stream::from_iter(vec![response, notification])))
    }
}

////////////////////////////////////////////////////////////////////////////////

pub(crate) struct BroadcastDeleteResponseHandler;

#[async_trait]
impl ResponseHandler for BroadcastDeleteResponseHandler {
    type Payload = CreateDeleteResponsePayload;
    type CorrelationData = CorrelationDataPayload;

    async fn handle<C: Context>(
        context: &mut C,
        _payload: Self::Payload,
        respp: &IncomingResponseProperties,
        corr_data: &Self::CorrelationData,
    ) -> Result {
        // Check if the event is sent by the broker.
        if respp.as_account_id() != &context.config().broker_id {
            return Err(anyhow!(
                "Expected subscription.delete event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                respp.as_account_id()
            )).error(AppErrorKind::AccessDenied);
        }

        // Parse room id.
        let room_id = try_room_id(&corr_data.object)?;
        context.add_logger_tags(o!("room_id" => room_id.to_string()));

        Ok(Box::new(stream::empty()))
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct DeleteEventPayload {
    subject: AgentId,
    object: Vec<String>,
}

pub(crate) struct DeleteEventHandler;

#[async_trait]
impl EventHandler for DeleteEventHandler {
    type Payload = DeleteEventPayload;

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
        let room_id = try_room_id(&payload.object)?;
        context.add_logger_tags(o!("room_id" => room_id.to_string()));

        let row_count = {
            let query = agent::DeleteQuery::new(payload.subject.clone(), room_id);
            let mut conn = context.get_conn().await?;

            let row_count = context
                .metrics()
                .measure_query(QueryKey::AgentDeleteQuery, query.execute(&mut conn))
                .await
                .context("Failed to delete agent")
                .error(AppErrorKind::DbQueryFailed)?;

            row_count
        };

        // Ignore missing agent.
        if row_count != 1 {
            return Ok(Box::new(stream::empty()));
        }
        let mut conn = context.get_conn().await?;
        let room = room::FindQuery::new(room_id)
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
        let to_uri = format!("rooms/{}/events", room_id);
        let outgoing_event = OutgoingEvent::broadcast(outgoing_event_payload, props, &to_uri);
        let boxed_event = Box::new(outgoing_event) as Box<dyn IntoPublishableMessage + Send>;
        Ok(Box::new(stream::once(boxed_event)))
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct BroadcastDeleteEventHandler;

#[async_trait]
impl EventHandler for BroadcastDeleteEventHandler {
    type Payload = DeleteEventPayload;

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        evp: &IncomingEventProperties,
    ) -> Result {
        // Check if the event is sent by the broker.
        if evp.as_account_id() != &context.config().broker_id {
            return Err(anyhow!(
                "Expected broadcast_subscription.delete event to be sent from the broker account '{}', got '{}'",
                context.config().broker_id,
                evp.as_account_id()
            )).error(AppErrorKind::AccessDenied);
        }

        let room_id = try_room_id(&payload.object)?;
        context.add_logger_tags(o!("room_id" => room_id.to_string()));

        warn!(context.logger(), "Broadcast subscription deleted by event");

        Ok(Box::new(stream::empty()))
    }
}

///////////////////////////////////////////////////////////////////////////////

fn try_room_id(object: &[String]) -> StdResult<Uuid, AppError> {
    let object: Vec<&str> = object.iter().map(AsRef::as_ref).collect();

    match object.as_slice() {
        ["rooms", room_id, "events"] => {
            Uuid::parse_str(room_id).map_err(|err| anyhow!("UUID parse error: {:?}", err))
        }
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
    mod create_response {
        use svc_agent::mqtt::ResponseStatus;

        use crate::app::API_VERSION;
        use crate::db::agent::{ListQuery as AgentListQuery, Status as AgentStatus};
        use crate::test_helpers::prelude::*;

        use super::super::*;

        #[derive(Deserialize)]
        struct TestRoomEnterEvent {
            id: Uuid,
            agent_id: AgentId,
            banned: bool,
        }

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

                // Send subscription.create response.
                let mut context = TestContext::new(db, TestAuthz::new());
                let reqp = build_reqp(agent.agent_id(), "room.enter");
                let room_id = room.id().to_string();

                let corr_data = CorrelationDataPayload {
                    reqp: reqp.clone(),
                    subject: agent.agent_id().to_owned(),
                    object: vec!["rooms".to_string(), room_id, "events".to_string()],
                };

                let broker_account_label = context.config().broker_id.label();
                let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

                let messages = handle_response::<CreateResponseHandler>(
                    &mut context,
                    &broker,
                    CreateDeleteResponsePayload {},
                    &corr_data,
                )
                .await
                .expect("Subscription creation failed");

                // Assert original request response.
                let (_payload, respp, topic) =
                    find_response::<CreateDeleteResponsePayload>(messages.as_slice());

                let expected_topic = format!(
                    "agents/{}/api/{}/in/event.{}",
                    agent.agent_id(),
                    API_VERSION,
                    SVC_AUDIENCE,
                );

                assert_eq!(topic, &expected_topic);
                assert_eq!(respp.status(), ResponseStatus::OK);
                assert_eq!(respp.correlation_data(), reqp.correlation_data());

                // Assert notification.

                let (payload, evp, topic) = find_event::<TestRoomEnterEvent>(messages.as_slice());
                assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
                assert_eq!(evp.label(), "room.enter");
                assert_eq!(payload.id, room.id());
                assert_eq!(&payload.agent_id, agent.agent_id());
                assert_eq!(payload.banned, false);

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
                let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());
                let room_id = Uuid::new_v4().to_string();

                let corr_data = CorrelationDataPayload {
                    reqp: build_reqp(agent.agent_id(), "room.enter"),
                    subject: agent.agent_id().to_owned(),
                    object: vec!["rooms".to_string(), room_id, "events".to_string()],
                };

                let broker_account_label = context.config().broker_id.label();
                let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

                let err = handle_response::<CreateResponseHandler>(
                    &mut context,
                    &broker,
                    CreateDeleteResponsePayload {},
                    &corr_data,
                )
                .await
                .expect_err("Unexpected success on subscription creation");

                assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
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

                let mut context = TestContext::new(db, TestAuthz::new());
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let room_id = room.id().to_string();

                let corr_data = CorrelationDataPayload {
                    reqp: build_reqp(agent.agent_id(), "room.enter"),
                    subject: agent.agent_id().to_owned(),
                    object: vec!["rooms".to_string(), room_id, "events".to_string()],
                };

                let broker_account_label = context.config().broker_id.label();
                let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

                let err = handle_response::<CreateResponseHandler>(
                    &mut context,
                    &broker,
                    CreateDeleteResponsePayload {},
                    &corr_data,
                )
                .await
                .expect_err("Unexpected success on subscription creation");

                assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
                assert_eq!(err.kind(), "room_closed");
            });
        }
    }

    mod delete_response {
        use svc_agent::mqtt::ResponseStatus;

        use crate::app::API_VERSION;
        use crate::db::agent::ListQuery as AgentListQuery;
        use crate::test_helpers::prelude::*;

        use super::super::*;

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

                // Send subscription.delete response.
                let mut context = TestContext::new(db, TestAuthz::new());
                let reqp = build_reqp(agent.agent_id(), "room.leave");
                let room_id = room.id().to_string();

                let corr_data = CorrelationDataPayload {
                    reqp: reqp.clone(),
                    subject: agent.agent_id().to_owned(),
                    object: vec!["rooms".to_string(), room_id, "events".to_string()],
                };

                let broker_account_label = context.config().broker_id.label();
                let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

                let messages = handle_response::<DeleteResponseHandler>(
                    &mut context,
                    &broker,
                    CreateDeleteResponsePayload {},
                    &corr_data,
                )
                .await
                .expect("Subscription deletion failed");

                // Assert original request response.
                let (_payload, respp, topic) =
                    find_response::<CreateDeleteResponsePayload>(messages.as_slice());

                let expected_topic = format!(
                    "agents/{}/api/{}/in/event.{}",
                    agent.agent_id(),
                    API_VERSION,
                    SVC_AUDIENCE,
                );

                assert_eq!(topic, &expected_topic);
                assert_eq!(respp.status(), ResponseStatus::OK);
                assert_eq!(respp.correlation_data(), reqp.correlation_data());

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

                let mut context = TestContext::new(db, TestAuthz::new());
                let room_id = room.id().to_string();

                let corr_data = CorrelationDataPayload {
                    reqp: build_reqp(agent.agent_id(), "room.leave"),
                    subject: agent.agent_id().to_owned(),
                    object: vec!["rooms".to_string(), room_id, "events".to_string()],
                };

                let broker_account_label = context.config().broker_id.label();
                let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

                let err = handle_response::<DeleteResponseHandler>(
                    &mut context,
                    &broker,
                    CreateDeleteResponsePayload {},
                    &corr_data,
                )
                .await
                .expect_err("Unexpected success on subscription deletion");

                assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
                assert_eq!(err.kind(), "agent_not_entered_the_room");
            });
        }

        #[test]
        fn delete_subscription_missing_room() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());
                let room_id = Uuid::new_v4().to_string();

                let corr_data = CorrelationDataPayload {
                    reqp: build_reqp(agent.agent_id(), "room.leave"),
                    subject: agent.agent_id().to_owned(),
                    object: vec!["rooms".to_string(), room_id, "events".to_string()],
                };

                let broker_account_label = context.config().broker_id.label();
                let broker = TestAgent::new("alpha", broker_account_label, SVC_AUDIENCE);

                let err = handle_response::<DeleteResponseHandler>(
                    &mut context,
                    &broker,
                    CreateDeleteResponsePayload {},
                    &corr_data,
                )
                .await
                .expect_err("Unexpected success on subscription deletion");

                assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
                assert_eq!(err.kind(), "agent_not_entered_the_room");
            });
        }
    }

    mod delete_event {
        use crate::db::agent::ListQuery as AgentListQuery;
        use crate::test_helpers::prelude::*;

        use super::super::*;

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
            });
        }
    }
}
