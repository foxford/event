use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use svc_agent::mqtt::{
    OutgoingEvent, OutgoingEventProperties, ResponseStatus, ShortTermTimingProperties,
};
use svc_error::Error as SvcError;
use svc_utils::extractors::AgentIdExtractor;
use tracing::{error, info, instrument};
use uuid::Uuid;

use crate::app::endpoint::prelude::*;
use crate::app::operations::{adjust_room, AdjustOutput};
use crate::app::{
    context::{AppContext, Context},
    message_handler::Message,
};
use crate::db::adjustment::Segments;

#[derive(Debug, Deserialize)]
pub struct AdjustPayload {
    #[serde(with = "chrono::serde::ts_milliseconds")]
    started_at: DateTime<Utc>,
    #[serde(with = "crate::db::adjustment::serde::segments")]
    segments: Segments,
    offset: i64,
}

#[derive(Debug, Deserialize)]
pub struct AdjustRequest {
    id: Uuid,
    #[serde(flatten)]
    payload: AdjustPayload,
}

pub async fn adjust(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
    Json(payload): Json<AdjustPayload>,
) -> RequestResult {
    let request = AdjustRequest {
        id: room_id,
        payload,
    };
    AdjustHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub struct AdjustHandler;

#[async_trait]
impl RequestHandler for AdjustHandler {
    type Payload = AdjustRequest;

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

        // Run asynchronous task for adjustment.
        let db = context.db().to_owned();
        let metrics = context.metrics();
        let cfg = context.config().to_owned();

        let notification_future = tokio::task::spawn(async move {
            let operation_result = adjust_room(
                &db,
                &metrics,
                &room,
                payload.started_at,
                &payload.segments,
                payload.offset,
                cfg.adjust,
            )
            .await;

            // Handle result.
            let result = match operation_result {
                Ok(AdjustOutput {
                    original_room,
                    modified_room,
                    modified_segments,
                    cut_original_segments,
                }) => {
                    info!(class_id = %room.classroom_id(), "Adjustment job succeeded");
                    RoomAdjustResult::Success {
                        original_room_id: original_room.id(),
                        modified_room_id: modified_room.id(),
                        modified_segments,
                        cut_original_segments,
                    }
                }
                Err(err) => {
                    error!(class_id = %room.classroom_id(), "Room adjustment job failed: {:?}", err);
                    let app_error = AppError::new(AppErrorKind::RoomAdjustTaskFailed, err);
                    app_error.notify_sentry();
                    RoomAdjustResult::Error {
                        error: app_error.to_svc_error(),
                    }
                }
            };

            // Publish success/failure notification.
            let notification = RoomAdjustNotification {
                room_id: id,
                status: result.status(),
                tags: room.tags().map(|t| t.to_owned()),
                result,
            };

            let timing = ShortTermTimingProperties::new(Utc::now());
            let props = OutgoingEventProperties::new("room.adjust", timing);
            let path = format!("audiences/{}/events", room.audience());
            let event = OutgoingEvent::broadcast(notification, props, &path);

            Box::new(event) as Message
        });

        // Respond with 202.
        // The actual task result will be broadcasted to the room topic when finished.
        let mut response = AppResponse::new(
            ResponseStatus::ACCEPTED,
            json!({}),
            context.start_timestamp(),
            Some(authz_time),
        );

        response.add_async_task(notification_future);

        Ok(response)
    }
}

#[derive(Serialize)]
struct RoomAdjustNotification {
    room_id: Uuid,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<JsonValue>,
    #[serde(flatten)]
    result: RoomAdjustResult,
}

#[derive(Serialize)]
#[serde(untagged)]
enum RoomAdjustResult {
    Success {
        original_room_id: Uuid,
        modified_room_id: Uuid,
        #[serde(with = "crate::db::adjustment::serde::segments")]
        modified_segments: Segments,
        #[serde(with = "crate::db::adjustment::serde::segments")]
        cut_original_segments: Segments,
    },
    Error {
        error: SvcError,
    },
}

impl RoomAdjustResult {
    fn status(&self) -> &'static str {
        match self {
            Self::Success { .. } => "success",
            Self::Error { .. } => "error",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_helpers::prelude::*;

    use super::*;

    #[tokio::test]
    async fn adjust_room_not_authorized() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let db = TestDb::new().await;

        let room = {
            // Create room.
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        // Make room.adjust request.
        let mut context = TestContext::new(db, TestAuthz::new());

        let payload = AdjustRequest {
            id: room.id(),
            payload: AdjustPayload {
                started_at: Utc::now(),
                segments: vec![].into(),
                offset: 0,
            },
        };

        let err = handle_request::<AdjustHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room adjustment");

        assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
    }

    #[tokio::test]
    async fn adjust_room_missing() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

        let payload = AdjustRequest {
            id: Uuid::new_v4(),
            payload: AdjustPayload {
                started_at: Utc::now(),
                segments: vec![].into(),
                offset: 0,
            },
        };

        let err = handle_request::<AdjustHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room adjustment");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_not_found");
    }
}
