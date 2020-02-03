use async_trait::async_trait;
use chrono::{DateTime, Utc};
use failure::{format_err, Error};
use futures::task::SpawnExt;
use log::{error, warn};
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use svc_agent::mqtt::{
    IncomingRequestProperties, IntoPublishableDump, OutgoingEvent, OutgoingEventProperties,
    ResponseStatus, ShortTermTimingProperties,
};
use svc_authn::Authenticable;
use svc_error::{extension::sentry, Error as SvcError};
use uuid::Uuid;

use crate::app::endpoint::{helpers, RequestHandler};
use crate::app::message_handler::publish_message;
use crate::app::operations::adjust_room;
use crate::app::Context;
use crate::db::adjustment::Segment;
use crate::db::room::{FindQuery, InsertQuery, Time};

///////////////////////////////////////////////////////////////////////////////

macro_rules! find_room {
    ($conn: expr, $id: expr, $reqp: expr, $start_timestamp: expr, $error_title: expr) => {
        match FindQuery::new($id).execute(&$conn)? {
            Some(room) => room,
            None => {
                return Ok(vec![helpers::build_error_response(
                    ResponseStatus::NOT_FOUND,
                    $error_title,
                    &format!("Room not found, id = '{}'", &$id),
                    $reqp,
                    $start_timestamp,
                )])
            }
        }
    };
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct CreateRequest {
    audience: String,
    #[serde(with = "crate::serde::ts_seconds_bound_tuple")]
    time: Time,
    tags: Option<JsonValue>,
}

pub(crate) struct CreateHandler;

#[async_trait]
impl RequestHandler for CreateHandler {
    type Payload = CreateRequest;
    const ERROR_TITLE: &'static str = "Failed to create room";

    async fn handle(
        context: &Context,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, Error> {
        let backend_room = context
            .backend()
            .create_room(reqp, &payload.audience)
            .await?;

        context
            .backend()
            .open_room(reqp, &payload.audience, backend_room.id)
            .await?;

        let room = {
            let mut query = InsertQuery::new(backend_room.id, &payload.audience, payload.time);

            if let Some(tags) = payload.tags {
                query = query.tags(tags);
            }

            let conn = context.db().get()?;
            query.execute(&conn)?
        };

        let response =
            helpers::build_response(ResponseStatus::CREATED, room.clone(), reqp, start_timestamp);

        let notification = helpers::build_notification(
            "room.create",
            &format!("audiences/{}", payload.audience),
            room,
            reqp,
            start_timestamp,
        );

        Ok(vec![response, notification])
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct ReadRequest {
    id: Uuid,
}

pub(crate) struct ReadHandler;

#[async_trait]
impl RequestHandler for ReadHandler {
    type Payload = ReadRequest;
    const ERROR_TITLE: &'static str = "Failed to read room";

    async fn handle(
        context: &Context,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, Error> {
        let conn = context.db().get()?;

        let room = find_room!(
            conn,
            payload.id,
            reqp,
            start_timestamp,
            ReadHandler::ERROR_TITLE
        );

        Ok(vec![helpers::build_response(
            ResponseStatus::OK,
            room,
            reqp,
            start_timestamp,
        )])
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct AdjustRequest {
    id: Uuid,
    started_at: DateTime<Utc>,
    #[serde(with = "crate::serde::milliseconds_bound_tuples")]
    segments: Vec<Segment>,
    offset: i64,
}

pub(crate) struct AdjustHandler;

#[async_trait]
impl RequestHandler for AdjustHandler {
    type Payload = AdjustRequest;
    const ERROR_TITLE: &'static str = "Failed to adjust room";

    async fn handle(
        context: &Context,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, Error> {
        let room = {
            let conn = context.db().get()?;

            find_room!(
                conn,
                payload.id,
                reqp,
                start_timestamp,
                AdjustHandler::ERROR_TITLE
            )
        };

        let mut agent = context.agent().clone();
        let db = context.db().to_owned();
        let backend = context.backend();
        let account = reqp.as_account_id().to_owned();
        let id = room.id();

        context
            .thread_pool()
            .spawn(async move {
                let future = adjust_room(
                    db,
                    &backend,
                    &account,
                    &room,
                    payload.started_at,
                    payload.segments,
                    payload.offset,
                );

                let result = future
                    .await
                    .map(|(original_room, modified_room, modified_segments)| {
                        RoomAdjustResult::Success {
                            original_room_id: original_room.id(),
                            modified_room_id: modified_room.id(),
                            modified_segments,
                        }
                    })
                    .unwrap_or_else(|err| {
                        error!(
                            "Room adjustment job failed for room_id = '{}': {}",
                            room.id(),
                            err
                        );

                        let error = SvcError::builder()
                            .status(ResponseStatus::INTERNAL_SERVER_ERROR)
                            .kind("room.adjust", AdjustHandler::ERROR_TITLE)
                            .detail(&err.to_string())
                            .build();

                        sentry::send(error.clone())
                            .unwrap_or_else(|err| warn!("Error sending error to Sentry: {}", err));

                        RoomAdjustResult::Error { error }
                    });

                let notification = RoomAdjustNotification {
                    status: result.status(),
                    tags: room.tags().map(|t| t.to_owned()),
                    result,
                };

                let timing = ShortTermTimingProperties::new(Utc::now());
                let props = OutgoingEventProperties::new("room.adjust", timing);
                let path = format!("audiences/{}/rooms/{}", room.audience(), room.id());
                let event = Box::new(OutgoingEvent::broadcast(notification, props, &path));

                publish_message(&mut agent, event).unwrap_or_else(|err| {
                    error!("Failed to publish room.adjust notification: {}", err)
                });
            })
            .map_err(|err| {
                format_err!(
                    "Failed to spawn room adjustment task for room_id = '{}': {}",
                    id,
                    err,
                )
            })?;

        Ok(vec![helpers::build_response(
            ResponseStatus::ACCEPTED,
            json!({}),
            reqp,
            start_timestamp,
        )])
    }
}

#[derive(Serialize)]
struct RoomAdjustNotification {
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
        #[serde(with = "crate::serde::milliseconds_bound_tuples")]
        modified_segments: Vec<Segment>,
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
