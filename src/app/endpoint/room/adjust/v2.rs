use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_derive::Deserialize;
use serde_json::json;
use svc_agent::mqtt::{
    OutgoingEvent, OutgoingEventProperties, ResponseStatus, ShortTermTimingProperties,
};
use svc_agent::AgentId;
use tracing::{error, info, instrument};
use uuid::Uuid;

use crate::app::endpoint::room::adjust::{
    RoomAdjustNotification, RoomAdjustResult, RoomAdjustResultV2,
};
use crate::{
    app::{
        context::Context,
        endpoint::prelude::*,
        message_handler::Message,
        operations::adjust_room::v2::{call as adjust_room, AdjustOutput},
    },
    db::adjustment::Segments,
};

#[derive(Debug, Deserialize)]
pub struct Recording {
    pub id: Uuid,
    pub rtc_id: Uuid,
    pub host: bool,
    #[serde(with = "crate::db::adjustment::serde::segments")]
    pub segments: Segments,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub started_at: DateTime<Utc>,
    pub created_by: AgentId,
}

#[derive(Debug, Deserialize)]
pub struct MuteEvent {
    pub rtc_id: Uuid,
    pub send_video: Option<bool>,
    pub send_audio: Option<bool>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AdjustPayload {
    recordings: Vec<Recording>,
    mute_events: Vec<MuteEvent>,
    offset: i64,
}

#[derive(Debug, Deserialize)]
pub struct AdjustRequest {
    id: Uuid,
    #[serde(flatten)]
    payload: AdjustPayload,
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
                &payload.recordings,
                &payload.mute_events,
                payload.offset,
                cfg.adjust,
            )
            .await;

            // Handle result.
            let result = match operation_result {
                Ok(AdjustOutput {
                    original_room,
                    modified_room,
                    recordings,
                    modified_room_time,
                }) => {
                    info!(class_id = %room.classroom_id(), "Adjustment job succeeded");

                    RoomAdjustResultV2::Success {
                        original_room_id: original_room.id(),
                        modified_room_id: modified_room.id(),
                        recordings,
                        modified_room_time,
                    }
                }
                Err(err) => {
                    error!(class_id = %room.classroom_id(), "Room adjustment job failed: {:?}", err);
                    let app_error = AppError::new(AppErrorKind::RoomAdjustTaskFailed, err);
                    app_error.notify_sentry();
                    RoomAdjustResultV2::Error {
                        error: app_error.to_svc_error(),
                    }
                }
            };
            let result = RoomAdjustResult::V2(result);

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
