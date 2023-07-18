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
use svc_agent::mqtt::ResponseStatus;
use svc_utils::extractors::AgentIdExtractor;
use tracing::instrument;
use uuid::Uuid;

use crate::app::context::{AppContext, Context};
use crate::app::endpoint::prelude::*;
use crate::db::room::UpdateQuery;

#[derive(Debug, Deserialize)]
pub struct LockedTypesPayload {
    locked_types: HashMap<String, bool>,
}

#[derive(Debug, Deserialize)]
pub struct LockedTypesRequest {
    id: Uuid,
    #[serde(flatten)]
    payload: LockedTypesPayload,
}

pub async fn locked_types(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
    Json(payload): Json<LockedTypesPayload>,
) -> RequestResult {
    let request = LockedTypesRequest {
        id: room_id,
        payload,
    };
    LockedTypesHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub struct LockedTypesHandler;

#[async_trait]
impl RequestHandler for LockedTypesHandler {
    type Payload = LockedTypesRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<C: Context>(
        context: &mut C,
        Self::Payload { id, payload }: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        // Find realtime room.
        let room = helpers::find_room(context, id, helpers::RoomTimeRequirement::Any).await?;

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
            let locked_types = room
                .locked_types()
                .iter()
                .map(|(k, v)| (k.to_owned(), *v))
                .chain(payload.locked_types)
                .collect::<HashMap<_, _>>();

            let mut conn = context.get_conn().await?;

            let mut txn = conn
                .begin()
                .await
                .context("Failed to acquire transaction")
                .error(AppErrorKind::DbQueryFailed)?;

            let query = UpdateQuery::new(room.id()).locked_types(locked_types);

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
    use crate::db::room::Object as Room;
    use crate::test_helpers::prelude::*;

    use super::*;

    #[tokio::test]
    async fn lock_types_in_room() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let room = {
            // Create room and put the agent online.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_room(&mut conn).await;
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

        // Make room.create request.
        let mut context = TestContext::new(db, authz);

        let payload = LockedTypesRequest {
            id: room.id(),
            payload: LockedTypesPayload {
                locked_types: [("message".into(), true)].iter().cloned().collect(),
            },
        };

        let messages = handle_request::<LockedTypesHandler>(&mut context, &agent, payload)
            .await
            .expect("Room types lock failed");

        let og_room = room;
        // Assert response.
        let (room, respp, _) = find_response::<Room>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(og_room.id(), room.id());
        assert_eq!(room.locked_types().len(), 1);
        assert_eq!(room.locked_types().get("message"), Some(&true));

        // Assert notification.
        let (room, evp, topic) = find_event::<Room>(messages.as_slice());
        assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
        assert_eq!(evp.label(), "room.update");
        assert_eq!(og_room.id(), room.id());
        assert_eq!(room.locked_types().len(), 1);
        assert_eq!(room.locked_types().get("message"), Some(&true));
    }

    #[tokio::test]
    async fn lock_multiple_types_in_room() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let room = {
            // Create room and put the agent online.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_room(&mut conn).await;
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

        // Make room.create request.
        let mut context = TestContext::new(db, authz);

        let payload = LockedTypesRequest {
            id: room.id(),
            payload: LockedTypesPayload {
                locked_types: [("message".into(), true)].iter().cloned().collect(),
            },
        };

        handle_request::<LockedTypesHandler>(&mut context, &agent, payload)
            .await
            .expect("Room types lock failed");

        let payload = LockedTypesRequest {
            id: room.id(),
            payload: LockedTypesPayload {
                locked_types: [("document".into(), true)].iter().cloned().collect(),
            },
        };

        handle_request::<LockedTypesHandler>(&mut context, &agent, payload)
            .await
            .expect("Room types lock failed");

        let payload = LockedTypesRequest {
            id: room.id(),
            payload: LockedTypesPayload {
                locked_types: [("message".into(), false)].iter().cloned().collect(),
            },
        };

        let messages = handle_request::<LockedTypesHandler>(&mut context, &agent, payload)
            .await
            .expect("Room types lock failed");

        let og_room = room;
        let (room, respp, _) = find_response::<Room>(messages.as_slice());

        // Assert response.
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(og_room.id(), room.id());
        assert_eq!(room.locked_types().len(), 1);
        assert_eq!(room.locked_types().len(), 1);
        assert_eq!(room.locked_types().get("message"), None);
        assert_eq!(room.locked_types().get("document"), Some(&true));

        // Assert notification.
        let (room, evp, topic) = find_event::<Room>(messages.as_slice());
        assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
        assert_eq!(evp.label(), "room.update");
        assert_eq!(og_room.id(), room.id());
        assert_eq!(room.locked_types().len(), 1);
        assert_eq!(room.locked_types().get("message"), None);
        assert_eq!(room.locked_types().get("document"), Some(&true));
    }

    #[tokio::test]
    async fn lock_types_in_room_not_authorized() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let room = {
            // Create room and put the agent online.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_room(&mut conn).await;
            shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
            room
        };

        // Make room.create request.
        let mut context = TestContext::new(db, TestAuthz::new());

        let payload = LockedTypesRequest {
            id: room.id(),
            payload: LockedTypesPayload {
                locked_types: [("message".into(), true)].iter().cloned().collect(),
            },
        };

        let err = handle_request::<LockedTypesHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on lock types");

        assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
    }
}
