use std::sync::Arc;

use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::extract::{Path, State};
use serde_derive::Deserialize;
use svc_agent::mqtt::ResponseStatus;
use svc_authn::Authenticable;
use svc_utils::extractors::AgentIdExtractor;
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::db;

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub struct ListRequest {
    room_id: Uuid,
}

pub async fn list(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
) -> RequestResult {
    let request = ListRequest { room_id };
    ListHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub(crate) struct ListHandler;

#[async_trait]
impl RequestHandler for ListHandler {
    type Payload = ListRequest;

    async fn handle<C: Context + Sync + Send>(
        context: &mut C,
        Self::Payload { room_id }: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let room = helpers::find_room(context, room_id, helpers::RoomTimeRequirement::Open).await?;

        let object = {
            let object = room.authz_object();
            let object = object.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
            AuthzObject::new(&object).into()
        };

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "update".into(),
            )
            .await?;

        // Get agents list in the room.
        let agents = {
            let mut conn = context.get_ro_conn().await?;

            let query = db::room_ban::ListQuery::new(room_id);

            context
                .metrics()
                .measure_query(QueryKey::AgentListQuery, query.execute(&mut conn))
                .await
                .context("Failed to list agents")
                .error(AppErrorKind::DbQueryFailed)?
        };

        // Respond with agents list.
        Ok(AppResponse::new(
            ResponseStatus::OK,
            agents,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use serde_derive::Deserialize;
    use svc_agent::AccountId;
    use uuid::Uuid;

    use crate::db::room_ban::InsertQuery as BanInsertQuery;
    use crate::test_helpers::prelude::*;

    use super::*;

    #[derive(Deserialize)]
    struct RoomBan {
        account_id: AccountId,
        reason: Option<String>,
    }

    #[tokio::test]
    async fn list_bans() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let banned_agent = TestAgent::new("web", "user456", USR_AUDIENCE);

        let room = {
            // Create room and put the agent online.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_room(&mut conn).await;

            shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
            shared_helpers::insert_agent(&mut conn, banned_agent.agent_id(), room.id()).await;
            let mut q = BanInsertQuery::new(banned_agent.account_id().to_owned(), room.id());
            q.reason("foobar");
            q.execute(&mut conn).await.expect("Failed to insert ban");

            room
        };

        let mut authz = TestAuthz::new();
        authz.allow(
            agent.account_id(),
            vec!["classrooms", &room.classroom_id().to_string()],
            "update",
        );

        let mut context = TestContext::new(db, authz);

        let payload = ListRequest { room_id: room.id() };

        let messages = handle_request::<ListHandler>(&mut context, &agent, payload)
            .await
            .expect("Bans listing failed");

        // Assert response.
        let (agents, respp, _) = find_response::<Vec<RoomBan>>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(agents.len(), 1);

        assert_eq!(&agents[0].account_id, banned_agent.account_id());
        assert_eq!(agents[0].reason.as_deref(), Some("foobar"));
    }

    #[tokio::test]
    async fn list_bans_not_authorized() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let db = TestDb::new().await;

        let room = {
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        let mut authz = TestAuthz::new();
        let classroom_id = room.classroom_id().to_string();
        authz.allow(
            agent.account_id(),
            vec!["classrooms", &classroom_id],
            "read",
        );

        let mut context = TestContext::new(db, authz);

        let payload = ListRequest { room_id: room.id() };

        let err = handle_request::<ListHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on agents listing");

        assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
    }

    #[tokio::test]
    async fn list_bans_closed_room() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let db = TestDb::new().await;

        let room = {
            // Create closed room.
            let mut conn = db.get_conn().await;
            shared_helpers::insert_closed_room(&mut conn).await
        };

        // Allow agent to list agents in the room.
        let mut authz = TestAuthz::new();
        let classroom_id = room.id().to_string();
        authz.allow(
            agent.account_id(),
            vec!["classrooms", &classroom_id],
            "update",
        );

        // Make agent.list request.
        let mut context = TestContext::new(db, authz);

        let payload = ListRequest { room_id: room.id() };

        let err = handle_request::<ListHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on agents listing");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_closed");
    }

    #[tokio::test]
    async fn list_bans_missing_room() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

        let payload = ListRequest {
            room_id: Uuid::new_v4(),
        };

        let err = handle_request::<ListHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on agents listing");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_not_found");
    }
}
