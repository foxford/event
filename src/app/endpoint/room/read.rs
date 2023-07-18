use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{Path, State};
use serde_derive::Deserialize;
use svc_agent::mqtt::ResponseStatus;

use svc_utils::extractors::AgentIdExtractor;
use tracing::instrument;
use uuid::Uuid;

use crate::app::context::{AppContext, Context};
use crate::app::endpoint::prelude::*;

#[derive(Debug, Deserialize)]
pub struct ReadRequest {
    id: Uuid,
}

pub async fn read(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
) -> RequestResult {
    let request = ReadRequest { id: room_id };
    ReadHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub struct ReadHandler;

#[async_trait]
impl RequestHandler for ReadHandler {
    type Payload = ReadRequest;

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
            helpers::find_room(context, payload.id, helpers::RoomTimeRequirement::Any).await?;

        // Authorize room reading on the tenant.
        let object = AuthzObject::room(&room).into();

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "read".into(),
            )
            .await?;

        Ok(AppResponse::new(
            ResponseStatus::OK,
            room,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::db::room::Object as Room;
    use crate::test_helpers::prelude::*;

    use super::*;

    #[tokio::test]
    async fn read_room() {
        let db = TestDb::new().await;

        let room = {
            // Create room.
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        // Allow agent to read the room.
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut authz = TestAuthz::new();
        authz.allow(
            agent.account_id(),
            vec!["classrooms", &room.classroom_id().to_string()],
            "read",
        );

        // Make room.read request.
        let mut context = TestContext::new(db, authz);
        let payload = ReadRequest { id: room.id() };

        let messages = handle_request::<ReadHandler>(&mut context, &agent, payload)
            .await
            .expect("Room reading failed");

        // Assert response.
        let (resp_room, respp, _) = find_response::<Room>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(resp_room.audience(), room.audience());
        assert_eq!(resp_room.time(), room.time());
        assert_eq!(resp_room.tags(), room.tags());
        assert_eq!(resp_room.preserve_history(), room.preserve_history());
    }

    #[tokio::test]
    async fn read_room_not_authorized() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let db = TestDb::new().await;

        let room = {
            // Create room.
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        // Make room.read request.
        let mut context = TestContext::new(db, TestAuthz::new());
        let payload = ReadRequest { id: room.id() };

        let err = handle_request::<ReadHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room reading");

        assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
    }

    #[tokio::test]
    async fn read_room_missing() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());
        let payload = ReadRequest { id: Uuid::new_v4() };

        let err = handle_request::<ReadHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room reading");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_not_found");
    }
}
