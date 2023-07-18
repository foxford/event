use std::ops::Bound;
use std::sync::Arc;

use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::Utc;
use serde_derive::Deserialize;
use serde_json::Value as JsonValue;
use svc_agent::mqtt::ResponseStatus;
use svc_utils::extractors::AgentIdExtractor;
use tracing::instrument;
use uuid::Uuid;

use crate::app::context::{AppContext, Context};
use crate::app::endpoint::prelude::*;
use crate::db::room::UpdateQuery;
use crate::db::room_time::BoundedDateTimeTuple;

#[derive(Debug, Deserialize)]
pub struct UpdatePayload {
    #[serde(default, with = "crate::serde::ts_seconds_option_bound_tuple")]
    time: Option<BoundedDateTimeTuple>,
    tags: Option<JsonValue>,
    classroom_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    id: Uuid,
    #[serde(flatten)]
    payload: UpdatePayload,
}

pub async fn update(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
    Json(payload): Json<UpdatePayload>,
) -> RequestResult {
    let request = UpdateRequest {
        id: room_id,
        payload,
    };
    UpdateHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub struct UpdateHandler;

#[async_trait]
impl RequestHandler for UpdateHandler {
    type Payload = UpdateRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<C: Context>(
        context: &mut C,
        Self::Payload { id, payload }: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let time_requirement = if payload.time.is_some() {
            // Forbid changing time of a closed room.
            helpers::RoomTimeRequirement::NotClosed
        } else {
            helpers::RoomTimeRequirement::Any
        };

        let room = helpers::find_room(context, id, time_requirement).await?;

        // Authorize room reading on the tenant.
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

        // Validate opening time.
        let time = if let Some(new_time) = payload.time {
            let room_time = room
                .time()
                .map_err(|e| anyhow!(e))
                .error(AppErrorKind::InvalidRoomTime)?;
            match room_time.update(new_time) {
                Some(nt) => Some(nt.into()),
                None => {
                    return Err(anyhow!("Invalid room time")).error(AppErrorKind::InvalidRoomTime)
                }
            }
        } else {
            None
        };

        let room_was_open = !room.is_closed();

        // Update room.
        let room = {
            let query = UpdateQuery::new(room.id())
                .time(time)
                .tags(payload.tags)
                .classroom_id(payload.classroom_id);

            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::RoomUpdateQuery, query.execute(&mut conn))
                .await
                .context("Failed to update room")
                .error(AppErrorKind::DbQueryFailed)?
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
            &format!("audiences/{}/events", room.audience()),
            room.clone(),
            context.start_timestamp(),
        );

        let append_closed_notification = || {
            response.add_notification(
                "room.close",
                &format!("rooms/{}/events", room.id()),
                room,
                context.start_timestamp(),
            );
        };

        // Publish room closed notification
        if room_was_open {
            if let Some(time) = payload.time {
                match time.1 {
                    Bound::Included(t) if Utc::now() > t => {
                        append_closed_notification();
                    }
                    Bound::Excluded(t) if Utc::now() >= t => {
                        append_closed_notification();
                    }
                    _ => {}
                }
            }
        }

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use chrono::{Duration, SubsecRound, Utc};
    use serde_json::json;

    use crate::db::room::{ClassType, Object as Room};
    use crate::db::room_time::RoomTimeBound;
    use crate::test_helpers::prelude::*;

    use super::*;

    #[tokio::test]
    async fn update_room() {
        let db = TestDb::new().await;
        let now = Utc::now().trunc_subsecs(0);

        let room = {
            let mut conn = db.get_conn().await;

            // Create room.
            factory::Room::new(uuid::Uuid::new_v4(), ClassType::Webinar)
                .audience(USR_AUDIENCE)
                .time((
                    Bound::Included(now + Duration::hours(1)),
                    Bound::Excluded(now + Duration::hours(2)),
                ))
                .tags(&json!({ "webinar_id": "123" }))
                .insert(&mut conn)
                .await
        };

        // Allow agent to update the room.
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut authz = TestAuthz::new();
        let classroom_id = room.classroom_id().to_string();
        authz.allow(
            agent.account_id(),
            vec!["classrooms", &classroom_id],
            "update",
        );

        // Make room.update request.
        let mut context = TestContext::new(db, authz);

        let time = (
            Bound::Included(now + Duration::hours(2)),
            Bound::Excluded(now + Duration::hours(3)),
        );

        let tags = json!({"webinar_id": "456789"});

        let payload = UpdateRequest {
            id: room.id(),
            payload: UpdatePayload {
                time: Some(time),
                tags: Some(tags.clone()),
                classroom_id: None,
            },
        };

        let messages = handle_request::<UpdateHandler>(&mut context, &agent, payload)
            .await
            .expect("Room update failed");

        // Assert response.
        let (resp_room, respp, _) = find_response::<Room>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(resp_room.id(), room.id());
        assert_eq!(resp_room.audience(), room.audience());
        assert_eq!(resp_room.time().map(|t| t.into()), Ok(time));
        assert_eq!(resp_room.tags(), Some(&tags));
    }

    #[tokio::test]
    async fn update_closed_at_in_open_room() {
        let db = TestDb::new().await;
        let now = Utc::now().trunc_subsecs(0);

        let room = {
            let mut conn = db.get_conn().await;

            // Create room.
            factory::Room::new(Uuid::new_v4(), ClassType::Webinar)
                .audience(USR_AUDIENCE)
                .time((
                    Bound::Included(now - Duration::hours(1)),
                    Bound::Excluded(now + Duration::hours(1)),
                ))
                .insert(&mut conn)
                .await
        };

        // Allow agent to update the room.
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut authz = TestAuthz::new();
        let classroom_id = room.classroom_id().to_string();
        authz.allow(
            agent.account_id(),
            vec!["classrooms", &classroom_id],
            "update",
        );

        // Make room.update request.
        let mut context = TestContext::new(db, authz);

        let time = (
            Bound::Included(now + Duration::hours(1)),
            Bound::Excluded(now + Duration::hours(3)),
        );

        let payload = UpdateRequest {
            id: room.id(),
            payload: UpdatePayload {
                time: Some(time),
                tags: None,
                classroom_id: None,
            },
        };

        let messages = handle_request::<UpdateHandler>(&mut context, &agent, payload)
            .await
            .expect("Room update failed");

        let (resp_room, respp, _) = find_response::<Room>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(resp_room.id(), room.id());
        assert_eq!(resp_room.audience(), room.audience());
        assert_eq!(
            resp_room.time().map(|t| t.into()),
            Ok((
                Bound::Included(now - Duration::hours(1)),
                Bound::Excluded(now + Duration::hours(3)),
            ))
        );
    }

    #[tokio::test]
    async fn update_closed_at_in_the_past_in_already_open_room() {
        let db = TestDb::new().await;
        let now = Utc::now().trunc_subsecs(0);

        let room = {
            let mut conn = db.get_conn().await;

            // Create room.
            factory::Room::new(Uuid::new_v4(), ClassType::Webinar)
                .audience(USR_AUDIENCE)
                .time((
                    Bound::Included(now - Duration::hours(2)),
                    Bound::Excluded(now + Duration::hours(2)),
                ))
                .insert(&mut conn)
                .await
        };

        // Allow agent to update the room.
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut authz = TestAuthz::new();
        let classroom_id = room.classroom_id().to_string();
        authz.allow(
            agent.account_id(),
            vec!["classrooms", &classroom_id],
            "update",
        );

        // Make room.update request.
        let mut context = TestContext::new(db, authz);

        let time = (
            Bound::Included(now - Duration::hours(2)),
            Bound::Excluded(now - Duration::hours(1)),
        );

        let payload = UpdateRequest {
            id: room.id(),
            payload: UpdatePayload {
                time: Some(time),
                tags: None,
                classroom_id: None,
            },
        };

        let messages = handle_request::<UpdateHandler>(&mut context, &agent, payload)
            .await
            .expect("Room update failed");

        let (resp_room, respp, _) = find_response::<Room>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(resp_room.id(), room.id());
        assert_eq!(resp_room.audience(), room.audience());
        assert_eq!(
            resp_room.time().map(|t| t.start().to_owned()),
            Ok(now - Duration::hours(2))
        );

        match resp_room.time().map(|t| t.end().to_owned()) {
            Ok(RoomTimeBound::Excluded(t)) => {
                let x = t - now;
                // Less than 2 seconds apart is basically 'now'
                // avoids intermittent failures (that were happening in CI even for 1 second boundary)
                assert!(
                    x.num_seconds().abs() < 2,
                    "Duration exceeded 1 second = {:?}",
                    x
                );
            }
            v => panic!("Expected Excluded bound, got {:?}", v),
        }

        // since we just closed the room we must receive a room.close event
        let (ev_room, _, _) = find_event_by_predicate::<Room, _>(messages.as_slice(), |evp| {
            evp.label() == "room.close"
        })
        .expect("Failed to find room.close event");
        assert_eq!(ev_room.id(), room.id());
    }

    #[tokio::test]
    async fn update_room_invalid_time() {
        let db = TestDb::new().await;
        let now = Utc::now().trunc_subsecs(0);

        let room = {
            let mut conn = db.get_conn().await;

            // Create room.
            factory::Room::new(Uuid::new_v4(), ClassType::Webinar)
                .audience(USR_AUDIENCE)
                .time((
                    Bound::Included(now + Duration::hours(1)),
                    Bound::Excluded(now + Duration::hours(2)),
                ))
                .insert(&mut conn)
                .await
        };

        // Allow agent to update the room.
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut authz = TestAuthz::new();
        let classroom_id = room.classroom_id().to_string();
        authz.allow(
            agent.account_id(),
            vec!["classrooms", &classroom_id],
            "update",
        );

        // Make room.update request.
        let mut context = TestContext::new(db, authz);

        let time = (
            Bound::Included(now + Duration::hours(1)),
            Bound::Excluded(now - Duration::hours(2)),
        );

        let payload = UpdateRequest {
            id: room.id(),
            payload: UpdatePayload {
                time: Some(time),
                tags: None,
                classroom_id: None,
            },
        };

        let err = handle_request::<UpdateHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room update");

        assert_eq!(err.status(), ResponseStatus::BAD_REQUEST);
        assert_eq!(err.kind(), "invalid_room_time");
    }

    #[tokio::test]
    async fn update_room_not_authorized() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let db = TestDb::new().await;

        let room = {
            // Create room.
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        // Make room.update request.
        let mut context = TestContext::new(db, TestAuthz::new());
        let payload = UpdateRequest {
            id: room.id(),
            payload: UpdatePayload {
                time: None,
                tags: None,
                classroom_id: None,
            },
        };

        let err = handle_request::<UpdateHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room update");

        assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
    }

    #[tokio::test]
    async fn update_room_missing() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

        let payload = UpdateRequest {
            id: Uuid::new_v4(),
            payload: UpdatePayload {
                time: None,
                tags: None,
                classroom_id: None,
            },
        };

        let err = handle_request::<UpdateHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room update");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_not_found");
    }

    #[tokio::test]
    async fn update_room_closed() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let db = TestDb::new().await;

        let room = {
            // Create closed room.
            let mut conn = db.get_conn().await;
            shared_helpers::insert_closed_room(&mut conn).await
        };

        let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());
        let now = Utc::now().trunc_subsecs(0);

        let time = (
            Bound::Included(now - Duration::hours(2)),
            Bound::Excluded(now - Duration::hours(1)),
        );

        let payload = UpdateRequest {
            id: room.id(),
            payload: UpdatePayload {
                time: Some(time.into()),
                tags: None,
                classroom_id: None,
            },
        };

        let err = handle_request::<UpdateHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room update");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_closed");
    }
}
