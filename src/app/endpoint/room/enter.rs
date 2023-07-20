use std::sync::Arc;

use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::{
    extract::{Path, State},
    Json,
};
use serde_derive::{Deserialize, Serialize};
use serde_json::json;
use svc_agent::{mqtt::ResponseStatus, Addressable, AgentId};

use svc_utils::extractors::AgentIdExtractor;
use tracing::instrument;
use uuid::Uuid;

use crate::app::context::{AppContext, Context};
use crate::app::endpoint::prelude::*;

use crate::db::agent;
use crate::db::event::{insert_agent_action, AgentAction};

#[derive(Debug, Deserialize)]
pub struct EnterPayload {
    #[serde(default)]
    agent_label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EnterRequest {
    id: Uuid,
}

#[derive(Deserialize, Serialize)]
pub struct RoomEnterEvent {
    id: Uuid,
    agent_id: AgentId,
    banned: bool,
    agent: agent::AgentWithBan,
}

pub struct EnterHandler;

pub async fn enter(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
    Json(payload): Json<EnterPayload>,
) -> RequestResult {
    let agent_label = payload
        .agent_label
        .as_ref()
        .context("No agent label present")
        .error(AppErrorKind::InvalidPayload)?;
    let agent_id = AgentId::new(agent_label, agent_id.as_account_id().to_owned());
    let request = EnterRequest { id: room_id };
    EnterHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

#[async_trait]
impl RequestHandler for EnterHandler {
    type Payload = EnterRequest;

    #[instrument(
    skip_all,
    fields(
    room_id = %payload.id, scope, classroom_id
    )
    )]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let room =
            helpers::find_room(context, payload.id, helpers::RoomTimeRequirement::Open).await?;

        // Authorize subscribing to the room's events.
        let object: Box<dyn svc_authz::IntentObject> =
            AuthzObject::new(&["classrooms", &room.classroom_id().to_string()]).into();

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "read".into(),
            )
            .await?;

        // Register agent in `in_progress` state.
        {
            let mut conn = context.get_conn().await?;
            let query = agent::InsertQuery::new(reqp.as_agent_id().to_owned(), room.id());

            context
                .metrics()
                .measure_query(QueryKey::AgentInsertQuery, query.execute(&mut conn))
                .await
                .context("Failed to insert agent into room")
                .error(AppErrorKind::DbQueryFailed)?;
            context
                .metrics()
                .measure_query(
                    QueryKey::EventInsertQuery,
                    insert_agent_action(&room, AgentAction::Enter, reqp.as_agent_id(), &mut conn),
                )
                .await
                .context("Failed to insert agent action")
                .error(AppErrorKind::DbQueryFailed)?;
        }

        let req1 = context
            .broker_client()
            .enter_room(room.id(), reqp.as_agent_id());
        let req2 = context
            .broker_client()
            .enter_broadcast_room(room.id(), reqp.as_agent_id());

        tokio::try_join!(req1, req2)
            .context("Broker request failed")
            .error(AppErrorKind::BrokerRequestFailed)?;

        // Determine whether the agent is banned.
        let agent_with_ban = {
            // Find room.
            helpers::find_room(context, room.id(), helpers::RoomTimeRequirement::Open).await?;

            // Update agent state to `ready`.
            let q = agent::UpdateQuery::new(reqp.as_agent_id().clone(), room.id())
                .status(agent::Status::Ready);

            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::AgentUpdateQuery, q.execute(&mut conn))
                .await
                .context("Failed to put agent into 'ready' status")
                .error(AppErrorKind::DbQueryFailed)?;

            let query = agent::FindWithBanQuery::new(reqp.as_agent_id().clone(), room.id());

            context
                .metrics()
                .measure_query(QueryKey::AgentFindWithBanQuery, query.execute(&mut conn))
                .await
                .context("Failed to find agent with ban")
                .error(AppErrorKind::DbQueryFailed)?
                .ok_or_else(|| anyhow!("No agent {} in room {}", reqp.as_agent_id(), room.id()))
                .error(AppErrorKind::AgentNotEnteredTheRoom)?
        };

        let banned = agent_with_ban.banned().unwrap_or(false);

        // Send a response to the original `room.enter` request and a room-wide notification.
        let mut response = AppResponse::new(
            ResponseStatus::OK,
            json!({}),
            context.start_timestamp(),
            Some(authz_time),
        );

        response.add_notification(
            "room.enter",
            &format!("rooms/{}/events", room.id()),
            RoomEnterEvent {
                id: room.id(),
                agent_id: reqp.as_agent_id().to_owned(),
                agent: agent_with_ban,
                banned,
            },
            context.start_timestamp(),
        );

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use crate::app::broker_client::CreateDeleteResponse;
    use serde_json::Value as JsonValue;

    use crate::test_helpers::prelude::*;

    use super::*;

    #[test]
    fn test_parsing() {
        serde_json::from_str::<EnterRequest>(
            r#"
                {"id": "82f62913-c2ba-4b21-b24f-5ed499107c0a"}
            "#,
        )
        .expect("Failed to parse EnterRequest");

        serde_json::from_str::<EnterRequest>(
            r#"
                {"id": "82f62913-c2ba-4b21-b24f-5ed499107c0a", "broadcast_subscription": true}
            "#,
        )
        .expect("Failed to parse EnterRequest");

        serde_json::from_str::<EnterRequest>(
            r#"
                {"id": "82f62913-c2ba-4b21-b24f-5ed499107c0a", "broadcast_subscription": false}
            "#,
        )
        .expect("Failed to parse EnterRequest");
    }

    #[tokio::test]
    async fn enter_room() {
        let db = TestDb::new().await;

        let room = {
            // Create room.
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        // Allow agent to subscribe to the rooms' events.
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut authz = TestAuthz::new();
        let classroom_id = room.classroom_id().to_string();
        authz.allow(
            agent.account_id(),
            vec!["classrooms", &classroom_id],
            "read",
        );

        // Make room.enter request.
        let mut context = TestContext::new(db, authz);

        context
            .broker_client_mock()
            .expect_enter_room()
            .with(mockall::predicate::always(), mockall::predicate::always())
            .returning(move |_, _agent_id| Ok(CreateDeleteResponse::Ok));

        context
            .broker_client_mock()
            .expect_enter_broadcast_room()
            .with(mockall::predicate::always(), mockall::predicate::always())
            .returning(move |_, _agent_id| Ok(CreateDeleteResponse::Ok));

        let payload = EnterRequest { id: room.id() };

        let messages = handle_request::<EnterHandler>(&mut context, &agent, payload)
            .await
            .expect("Room entrance failed");

        assert_eq!(messages.len(), 2);

        let (payload, _evp, _) =
            find_event_by_predicate::<JsonValue, _>(&messages, |evp| evp.label() == "room.enter")
                .unwrap();
        assert_eq!(payload["id"], room.id().to_string());
        assert_eq!(payload["agent_id"], agent.agent_id().to_string());

        // assert response exists
        find_response::<JsonValue>(&messages);
    }

    #[tokio::test]
    async fn enter_room_not_authorized() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let db = TestDb::new().await;

        let room = {
            // Create room.
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        // Make room.enter request.
        let mut context = TestContext::new(db, TestAuthz::new());
        let payload = EnterRequest { id: room.id() };

        let err = handle_request::<EnterHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room entering");

        assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
    }

    #[tokio::test]
    async fn enter_room_missing() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());
        let payload = EnterRequest { id: Uuid::new_v4() };

        let err = handle_request::<EnterHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room entering");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_not_found");
    }

    #[tokio::test]
    async fn enter_room_closed() {
        let db = TestDb::new().await;

        let room = {
            // Create closed room.
            let mut conn = db.get_conn().await;
            shared_helpers::insert_closed_room(&mut conn).await
        };

        // Allow agent to subscribe to the rooms' events.
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut authz = TestAuthz::new();
        let classroom_id = room.classroom_id().to_string();
        authz.allow(
            agent.account_id(),
            vec!["classrooms", &classroom_id],
            "read",
        );

        // Make room.enter request.
        let mut context = TestContext::new(db, TestAuthz::new());
        let payload = EnterRequest { id: room.id() };

        let err = handle_request::<EnterHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room entering");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_closed");
    }
}
