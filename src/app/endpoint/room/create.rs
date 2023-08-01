use std::sync::Arc;

use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::{
    extract::{self},
    Json,
};
use serde_derive::Deserialize;
use serde_json::Value as JsonValue;
use svc_agent::mqtt::ResponseStatus;
use svc_utils::extractors::AgentIdExtractor;
use tracing::instrument;
use uuid::Uuid;

use crate::app::context::{AppContext, Context};
use crate::app::endpoint::prelude::*;
use crate::db::room::{ClassType, InsertQuery};
use crate::db::room_time::{BoundedDateTimeTuple, RoomTime};

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    audience: String,
    #[serde(with = "crate::serde::ts_seconds_bound_tuple")]
    time: BoundedDateTimeTuple,
    tags: Option<JsonValue>,
    preserve_history: Option<bool>,
    classroom_id: Uuid,
    kind: ClassType,
}

pub async fn create(
    ctx: extract::Extension<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Json(request): Json<CreateRequest>,
) -> RequestResult {
    CreateHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub struct CreateHandler;

#[async_trait]
impl RequestHandler for CreateHandler {
    type Payload = CreateRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        // Validate opening time.
        match RoomTime::new(payload.time) {
            Some(_room_time) => (),
            _ => {
                return Err(anyhow!("Invalid room time"))
                    .error(AppErrorKind::InvalidRoomTime)
                    .map_err(|mut e| {
                        e.tag("audience", &payload.audience);
                        e.tag(
                            "time",
                            serde_json::to_string(&payload.time)
                                .as_deref()
                                .unwrap_or("Failed to serialize time"),
                        );
                        e.tag(
                            "tags",
                            serde_json::to_string(&payload.tags)
                                .as_deref()
                                .unwrap_or("Failed to serialize tags"),
                        );
                        e
                    })
            }
        }

        let object = AuthzObject::new(&["classrooms"]).into();

        // Authorize room creation on the tenant.
        let authz_time = context
            .authz()
            .authorize(
                payload.audience.clone(),
                reqp.as_account_id().to_owned(),
                object,
                "create".into(),
            )
            .await?;

        // Insert room.
        let room = {
            let mut query = InsertQuery::new(
                &payload.audience,
                payload.time.into(),
                payload.classroom_id,
                payload.kind,
            );

            if let Some(tags) = payload.tags {
                query = query.tags(tags);
            }

            if let Some(preserve_history) = payload.preserve_history {
                query = query.preserve_history(preserve_history);
            }

            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::RoomInsertQuery, query.execute(&mut conn))
                .await
                .context("Failed to insert room")
                .error(AppErrorKind::DbQueryFailed)?
        };

        helpers::add_room_logger_tags(&room);

        // Respond and broadcast to the audience topic.
        let mut response = AppResponse::new(
            ResponseStatus::CREATED,
            room.clone(),
            context.start_timestamp(),
            Some(authz_time),
        );

        response.add_notification(
            "room.create",
            &format!("audiences/{}/events", payload.audience),
            room,
            context.start_timestamp(),
        );

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use chrono::{Duration, SubsecRound, Utc};
    use serde_json::json;

    use crate::db::room::{ClassType, Object as Room};
    use crate::db::room_time::BoundedDateTimeTuple;
    use crate::test_helpers::prelude::*;

    use super::*;

    #[tokio::test]
    async fn create_room() {
        // Allow agent to create rooms.
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut authz = TestAuthz::new();
        authz.allow(agent.account_id(), vec!["classrooms"], "create");

        // Make room.create request.
        let mut context = TestContext::new(TestDb::new().await, authz);
        let now = Utc::now().trunc_subsecs(0);

        let time = (
            Bound::Included(now + Duration::hours(1)),
            Bound::Excluded(now + Duration::hours(2)),
        );

        let tags = json!({ "webinar_id": "123" });

        let payload = CreateRequest {
            time: BoundedDateTimeTuple::from(time),
            audience: USR_AUDIENCE.to_owned(),
            tags: Some(tags.clone()),
            preserve_history: Some(false),
            classroom_id: Uuid::new_v4(),
            kind: ClassType::Minigroup,
        };

        let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect("Room creation failed");

        // Assert response.
        let (room, respp, _) = find_response::<Room>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::CREATED);
        assert_eq!(room.audience(), USR_AUDIENCE);
        assert_eq!(room.time().map(|t| t.into()), Ok(time));
        assert_eq!(room.tags(), Some(&tags));

        // Assert notification.
        let (room, evp, topic) = find_event::<Room>(messages.as_slice());
        assert!(topic.ends_with(&format!("/audiences/{}/events", USR_AUDIENCE)));
        assert_eq!(evp.label(), "room.create");
        assert_eq!(room.audience(), USR_AUDIENCE);
        assert_eq!(room.time().map(|t| t.into()), Ok(time));
        assert_eq!(room.tags(), Some(&tags));
        assert_eq!(room.preserve_history(), false);
    }

    #[tokio::test]
    async fn create_room_unbounded() {
        // Allow agent to create rooms.
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut authz = TestAuthz::new();
        authz.allow(agent.account_id(), vec!["classrooms"], "create");

        // Make room.create request.
        let mut context = TestContext::new(TestDb::new().await, authz);
        let now = Utc::now().trunc_subsecs(0);

        let time = (Bound::Included(now + Duration::hours(1)), Bound::Unbounded);

        let tags = json!({ "webinar_id": "123" });

        let payload = CreateRequest {
            time: BoundedDateTimeTuple::from(time),
            audience: USR_AUDIENCE.to_owned(),
            tags: Some(tags.clone()),
            preserve_history: Some(false),
            classroom_id: Uuid::new_v4(),
            kind: ClassType::P2P,
        };

        let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect("Room creation failed");

        // Assert response.
        let (room, respp, _) = find_response::<Room>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::CREATED);
        assert_eq!(room.audience(), USR_AUDIENCE);
        assert_eq!(room.time().map(|t| t.into()), Ok(time));
        assert_eq!(room.tags(), Some(&tags));

        // Assert notification.
        let (room, evp, topic) = find_event::<Room>(messages.as_slice());
        assert!(topic.ends_with(&format!("/audiences/{}/events", USR_AUDIENCE)));
        assert_eq!(evp.label(), "room.create");
        assert_eq!(room.audience(), USR_AUDIENCE);
        assert_eq!(room.time().map(|t| t.into()), Ok(time));
        assert_eq!(room.tags(), Some(&tags));
        assert_eq!(room.preserve_history(), false);
    }

    #[tokio::test]
    async fn create_room_unbounded_with_classroom_id() {
        // Allow agent to create rooms.
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut authz = TestAuthz::new();
        authz.allow(agent.account_id(), vec!["classrooms"], "create");

        // Make room.create request.
        let mut context = TestContext::new(TestDb::new().await, authz);
        let now = Utc::now().trunc_subsecs(0);

        let time = (Bound::Included(now + Duration::hours(1)), Bound::Unbounded);

        let tags = json!({ "webinar_id": "123" });
        let cid = Uuid::new_v4();

        let payload = CreateRequest {
            time: BoundedDateTimeTuple::from(time),
            audience: USR_AUDIENCE.to_owned(),
            tags: Some(tags.clone()),
            preserve_history: Some(false),
            classroom_id: cid,
            kind: ClassType::Webinar,
        };

        let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect("Room creation failed");

        // Assert response.
        let (room, respp, _) = find_response::<Room>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::CREATED);
        assert_eq!(room.audience(), USR_AUDIENCE);
        assert_eq!(room.time().map(|t| t.into()), Ok(time));
        assert_eq!(room.tags(), Some(&tags));
        assert_eq!(room.classroom_id(), cid);

        // Assert notification.
        let (room, evp, topic) = find_event::<Room>(messages.as_slice());
        assert!(topic.ends_with(&format!("/audiences/{}/events", USR_AUDIENCE)));
        assert_eq!(evp.label(), "room.create");
        assert_eq!(room.audience(), USR_AUDIENCE);
        assert_eq!(room.time().map(|t| t.into()), Ok(time));
        assert_eq!(room.tags(), Some(&tags));
        assert_eq!(room.preserve_history(), false);
        assert_eq!(room.classroom_id(), cid);
    }

    #[tokio::test]
    async fn create_room_not_authorized() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        // Make room.create request.
        let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());
        let now = Utc::now().trunc_subsecs(0);

        let time = (
            Bound::Included(now + Duration::hours(1)),
            Bound::Excluded(now + Duration::hours(2)),
        );

        let payload = CreateRequest {
            time: time.clone(),
            audience: USR_AUDIENCE.to_owned(),
            tags: None,
            preserve_history: None,
            classroom_id: Uuid::new_v4(),
            kind: ClassType::Minigroup,
        };

        let err = handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room creation");

        assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
    }

    #[tokio::test]
    async fn create_room_invalid_time() {
        // Allow agent to create rooms.
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut authz = TestAuthz::new();
        authz.allow(agent.account_id(), vec!["classrooms"], "create");

        // Make room.create request.
        let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

        let payload = CreateRequest {
            time: (Bound::Unbounded, Bound::Unbounded),
            audience: USR_AUDIENCE.to_owned(),
            tags: None,
            preserve_history: None,
            classroom_id: Uuid::new_v4(),
            kind: ClassType::Webinar,
        };

        let err = handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room creation");

        assert_eq!(err.status(), ResponseStatus::BAD_REQUEST);
        assert_eq!(err.kind(), "invalid_room_time");
    }
}
