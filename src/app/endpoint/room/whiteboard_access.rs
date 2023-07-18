use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::{
    extract::{Path, State},
    Json,
};
use serde_derive::Deserialize;
use sqlx::Acquire;
use svc_agent::{mqtt::ResponseStatus, AccountId};
use svc_utils::extractors::AgentIdExtractor;
use tracing::instrument;
use uuid::Uuid;

use crate::app::context::{AppContext, Context};
use crate::app::endpoint::prelude::*;
use crate::db::room::UpdateQuery;

#[derive(Debug, Deserialize)]
pub struct WhiteboardAccessPayload {
    whiteboard_access: HashMap<AccountId, bool>,
}

#[derive(Debug, Deserialize)]
pub struct WhiteboardAccessRequest {
    id: Uuid,
    #[serde(flatten)]
    payload: WhiteboardAccessPayload,
}

pub async fn whiteboard_access(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
    Json(payload): Json<WhiteboardAccessPayload>,
) -> RequestResult {
    let request = WhiteboardAccessRequest {
        id: room_id,
        payload,
    };
    WhiteboardAccessHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub struct WhiteboardAccessHandler;

#[async_trait]
impl RequestHandler for WhiteboardAccessHandler {
    type Payload = WhiteboardAccessRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<C: Context>(
        context: &mut C,
        Self::Payload { id, payload }: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        // Find realtime room.
        let room = helpers::find_room(context, id, helpers::RoomTimeRequirement::Any).await?;

        if !room.validate_whiteboard_access() {
            Err(anyhow!(
                "Useless whiteboard access change for room that doesnt check it"
            ))
            .error(AppErrorKind::WhiteboardAccessUpdateNotChecked)?
        }

        // Authorize trusted account for the room's audience.
        let object = AuthzObject::room(&room).into();

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "update".into(),
            )
            .await?;

        let room = {
            let whiteboard_access = room
                .whiteboard_access()
                .iter()
                .map(|(k, v)| (k.to_owned(), *v))
                .chain(payload.whiteboard_access)
                .collect();
            let mut conn = context.get_conn().await?;
            let mut txn = conn
                .begin()
                .await
                .context("Failed to acquire transaction")
                .error(AppErrorKind::DbQueryFailed)?;

            let query = UpdateQuery::new(room.id()).whiteboard_access(whiteboard_access);

            let room = context
                .metrics()
                .measure_query(QueryKey::RoomUpdateQuery, query.execute(&mut txn))
                .await
                .context("Failed to update room")
                .error(AppErrorKind::DbQueryFailed)?;

            txn.commit()
                .await
                .context("Failed to commit transaction")
                .error(AppErrorKind::DbQueryFailed)?;

            room
        };

        // Respond and broadcast to the audience topic.
        let mut response = AppResponse::new(
            ResponseStatus::OK,
            room.clone(),
            context.start_timestamp(),
            Some(authz_time),
        );

        response.add_notification(
            "room.update",
            &format!("rooms/{}/events", room.id()),
            room,
            context.start_timestamp(),
        );

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::room::Object as Room;
    use crate::test_helpers::prelude::*;

    #[tokio::test]
    async fn whiteboard_access() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let room = {
            // Create room and put the agent online.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_validating_whiteboard_access_room(&mut conn).await;
            shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
            room
        };

        // Allow agent to update rooms.
        let mut authz = TestAuthz::new();
        authz.allow(
            agent.account_id(),
            vec!["classrooms", &room.classroom_id().to_string()],
            "update",
        );

        // Make request.
        let mut context = TestContext::new(db, authz);

        let payload = WhiteboardAccessRequest {
            id: room.id(),
            payload: WhiteboardAccessPayload {
                whiteboard_access: [(agent.account_id().to_owned(), true)]
                    .iter()
                    .cloned()
                    .collect(),
            },
        };

        let messages = handle_request::<WhiteboardAccessHandler>(&mut context, &agent, payload)
            .await
            .expect("Room whiteboard access update failed");

        let og_room = room;
        // Assert response.
        let (room, respp, _) = find_response::<Room>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(og_room.id(), room.id());
        assert_eq!(room.whiteboard_access().len(), 1);
        assert_eq!(
            room.whiteboard_access().get(agent.account_id()),
            Some(&true)
        );

        // Assert notification.
        let (room, evp, topic) = find_event::<Room>(messages.as_slice());
        assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
        assert_eq!(evp.label(), "room.update");
        assert_eq!(og_room.id(), room.id());
        assert_eq!(room.whiteboard_access().len(), 1);
        assert_eq!(
            room.whiteboard_access().get(agent.account_id()),
            Some(&true)
        );
    }

    #[tokio::test]
    async fn whiteboard_access_for_multiple_accounts_in_room() {
        let db = TestDb::new().await;
        let teacher = TestAgent::new("web", "teacher", USR_AUDIENCE);
        let agent1 = TestAgent::new("web", "user123", USR_AUDIENCE);
        let agent2 = TestAgent::new("web", "user456", USR_AUDIENCE);

        let room = {
            // Create room and put the agent online.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_validating_whiteboard_access_room(&mut conn).await;
            shared_helpers::insert_agent(&mut conn, teacher.agent_id(), room.id()).await;
            room
        };

        // Allow agent to update rooms.
        let mut authz = TestAuthz::new();
        authz.allow(
            teacher.account_id(),
            vec!["classrooms", &room.classroom_id().to_string()],
            "update",
        );

        // Make room.create request.
        let mut context = TestContext::new(db, authz);

        let payload = WhiteboardAccessRequest {
            id: room.id(),
            payload: WhiteboardAccessPayload {
                whiteboard_access: [(agent1.account_id().to_owned(), true)]
                    .iter()
                    .cloned()
                    .collect(),
            },
        };

        handle_request::<WhiteboardAccessHandler>(&mut context, &teacher, payload)
            .await
            .expect("Room whiteboard access update failed");

        let payload = WhiteboardAccessRequest {
            id: room.id(),
            payload: WhiteboardAccessPayload {
                whiteboard_access: [(agent2.account_id().to_owned(), true)]
                    .iter()
                    .cloned()
                    .collect(),
            },
        };

        handle_request::<WhiteboardAccessHandler>(&mut context, &teacher, payload)
            .await
            .expect("Room whiteboard access update failed");

        let payload = WhiteboardAccessRequest {
            id: room.id(),
            payload: WhiteboardAccessPayload {
                whiteboard_access: [(agent1.account_id().to_owned(), false)]
                    .iter()
                    .cloned()
                    .collect(),
            },
        };

        let messages = handle_request::<WhiteboardAccessHandler>(&mut context, &teacher, payload)
            .await
            .expect("Room whiteboard access update failed");

        let og_room = room;
        let (room, respp, _) = find_response::<Room>(messages.as_slice());

        // Assert response.
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(og_room.id(), room.id());
        assert_eq!(room.whiteboard_access().len(), 1);
        assert_eq!(room.whiteboard_access().len(), 1);
        assert_eq!(room.whiteboard_access().get(agent1.account_id()), None);
        assert_eq!(
            room.whiteboard_access().get(agent2.account_id()),
            Some(&true)
        );

        // Assert notification.
        let (room, evp, topic) = find_event::<Room>(messages.as_slice());
        assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
        assert_eq!(evp.label(), "room.update");
        assert_eq!(og_room.id(), room.id());
        assert_eq!(room.whiteboard_access().len(), 1);
        assert_eq!(room.whiteboard_access().get(agent1.account_id()), None);
        assert_eq!(
            room.whiteboard_access().get(agent2.account_id()),
            Some(&true)
        );
    }

    #[tokio::test]
    async fn whiteboard_access_in_room_not_authorized() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let room = {
            // Create room and put the agent online.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_validating_whiteboard_access_room(&mut conn).await;
            shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
            room
        };

        // Make room.create request.
        let mut context = TestContext::new(db, TestAuthz::new());

        let payload = WhiteboardAccessRequest {
            id: room.id(),
            payload: WhiteboardAccessPayload {
                whiteboard_access: [(agent.account_id().to_owned(), true)]
                    .iter()
                    .cloned()
                    .collect(),
            },
        };

        let err = handle_request::<WhiteboardAccessHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on whiteboard access update");

        assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
    }
}
