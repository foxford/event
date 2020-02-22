use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_derive::Deserialize;
use svc_agent::mqtt::{IncomingRequestProperties, IntoPublishableDump, ResponseStatus};
use svc_error::Error as SvcError;
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::{helpers, RequestHandler};
use crate::db;

///////////////////////////////////////////////////////////////////////////////

const MAX_LIMIT: i64 = 25;

#[derive(Debug, Deserialize)]
pub(crate) struct ListRequest {
    room_id: Uuid,
    offset: Option<i64>,
    limit: Option<i64>,
}

pub(crate) struct ListHandler;

#[async_trait]
impl RequestHandler for ListHandler {
    type Payload = ListRequest;
    const ERROR_TITLE: &'static str = "Failed to list agents";

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError> {
        let conn = context.db().get()?;

        // Check whether the room exists and open.
        let room = db::room::FindQuery::new(payload.room_id)
            .time(db::room::now())
            .execute(&conn)?
            .ok_or_else(|| {
                svc_error!(
                    ResponseStatus::NOT_FOUND,
                    "the room = '{}' is not found or closed",
                    payload.room_id
                )
            })?;

        // Authorize agents listing in the room.
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id, "agents"];

        let authz_time = context
            .authz()
            .authorize(room.audience(), reqp, object, "list")
            .await?;

        // Get agents list in the room.
        let agents = db::agent::ListQuery::new()
            .room_id(payload.room_id)
            .status(db::agent::Status::Ready)
            .offset(payload.offset.unwrap_or_else(|| 0))
            .limit(std::cmp::min(
                payload.limit.unwrap_or_else(|| MAX_LIMIT),
                MAX_LIMIT,
            ))
            .execute(&conn)?;

        // Respond with agents list.
        Ok(vec![helpers::build_response(
            ResponseStatus::OK,
            agents,
            reqp,
            start_timestamp,
            Some(authz_time),
        )])
    }
}

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use chrono::{Duration, SubsecRound, Utc};
    use serde_derive::Deserialize;
    use serde_json::json;
    use svc_agent::AgentId;
    use uuid::Uuid;

    use crate::db::{agent::Status as AgentStatus, room::Object as Room};
    use crate::test_helpers::prelude::*;

    use super::*;

    #[derive(Deserialize)]
    struct Agent {
        agent_id: AgentId,
        room_id: Uuid,
    }

    #[test]
    fn list_agents() {
        futures::executor::block_on(async {
            // Create room.
            let db = TestDb::new();
            let room = insert_room(&db);

            // Allow agent to list agents in the room.
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();

            authz.allow(
                agent.account_id(),
                vec!["rooms", &room_id, "agents"],
                "list",
            );

            // Put agent online in the room.
            {
                let conn = db
                    .connection_pool()
                    .get()
                    .expect("Failed to get DB connection");

                factory::Agent::new()
                    .room_id(room.id())
                    .agent_id(agent.agent_id().to_owned())
                    .status(AgentStatus::Ready)
                    .insert(&conn);
            }

            // Make agent.list request.
            let context = TestContext::new(db, authz);

            let payload = ListRequest {
                room_id: room.id(),
                offset: None,
                limit: None,
            };

            let messages = handle_request::<ListHandler>(&context, &agent, payload).await;

            // Assert response.
            let (agents, respp) = find_response::<Vec<Agent>>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(agents.len(), 1);
            assert_eq!(&agents[0].agent_id, agent.agent_id());
            assert_eq!(agents[0].room_id, room.id());
        });
    }

    fn insert_room(db: &TestDb) -> Room {
        let conn = db
            .connection_pool()
            .get()
            .expect("Failed to get DB connection");

        let now = Utc::now().trunc_subsecs(0);

        factory::Room::new()
            .audience(USR_AUDIENCE)
            .time((
                Bound::Included(now),
                Bound::Excluded(now + Duration::hours(1)),
            ))
            .tags(&json!({ "webinar_id": "123" }))
            .insert(&conn)
    }
}
