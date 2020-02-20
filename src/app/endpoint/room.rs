use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::task::SpawnExt;
use log::{error, warn};
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use svc_agent::{
    mqtt::{
        IncomingRequestProperties, IntoPublishableDump, OutgoingEvent, OutgoingEventProperties,
        OutgoingRequest, ResponseStatus, ShortTermTimingProperties,
    },
    Addressable, AgentId,
};
use svc_error::{extension::sentry, Error as SvcError};
use uuid::Uuid;

use crate::app::endpoint::{helpers, RequestHandler};
use crate::app::message_handler::publish_message;
use crate::app::operations::adjust_room;
use crate::app::{Context, API_VERSION};
use crate::db::adjustment::Segment;
use crate::db::agent;
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
                    None,
                )])
            }
        }
    };
}

#[derive(Debug, Serialize)]
struct SubscriptionRequest {
    subject: AgentId,
    object: Vec<String>,
}

impl SubscriptionRequest {
    fn new(subject: AgentId, object: Vec<&str>) -> Self {
        Self {
            subject,
            object: object.iter().map(|&s| s.into()).collect(),
        }
    }
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
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError> {
        let backend_room = context
            .backend()
            .create_room(reqp, &payload.audience)
            .await
            .map_err(|err| {
                svc_error!(
                    ResponseStatus::FAILED_DEPENDENCY,
                    "Backend room creation request failed: {}",
                    err
                )
            })?;

        context
            .backend()
            .open_room(reqp, &payload.audience, backend_room.id)
            .await
            .map_err(|err| {
                svc_error!(
                    ResponseStatus::FAILED_DEPENDENCY,
                    "Backend room opening request failed: {}",
                    err
                )
            })?;

        let room = {
            let mut query = InsertQuery::new(&payload.audience, payload.time).id(backend_room.id);

            if let Some(tags) = payload.tags {
                query = query.tags(tags);
            }

            let conn = context.db().get()?;
            query.execute(&conn)?
        };

        let response = helpers::build_response(
            ResponseStatus::CREATED,
            room.clone(),
            reqp,
            start_timestamp,
            None,
        );

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
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError> {
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
            None,
        )])
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct EnterRequest {
    id: Uuid,
}

pub(crate) struct EnterHandler;

#[async_trait]
impl RequestHandler for EnterHandler {
    type Payload = EnterRequest;
    const ERROR_TITLE: &'static str = "Failed to enter room";

    async fn handle(
        context: &Context,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError> {
        let conn = context.db().get()?;

        let room = find_room!(
            conn,
            payload.id,
            reqp,
            start_timestamp,
            EnterHandler::ERROR_TITLE
        );

        // Authorize subscribing to the room's events.
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id, "events"];

        let authz_time = context
            .authz()
            .authorize(room.audience(), reqp, object.clone(), "subscribe")
            .await?;

        // Register agent in `in_progress` state.
        agent::InsertQuery::new(reqp.as_agent_id(), room.id()).execute(&conn)?;

        // Send dynamic subscription creation request to the broker.
        let payload = SubscriptionRequest::new(reqp.as_agent_id().to_owned(), object);

        let mut short_term_timing = ShortTermTimingProperties::until_now(start_timestamp);
        short_term_timing.set_authorization_time(authz_time);

        let props = reqp.to_request(
            "subscription.create",
            reqp.response_topic(),
            reqp.correlation_data(),
            short_term_timing,
        );

        // FIXME: It looks like sending a request to the client but the broker intercepts it
        //        creates a subscription and replaces the request with the response.
        //        This is kind of ugly but it guaranties that the request will be processed by
        //        the broker node where the client is connected to. We need that because
        //        the request changes local state on that node.
        //        A better solution will be possible after resolution of this issue:
        //        https://github.com/vernemq/vernemq/issues/1326.
        //        Then we won't need the local state on the broker at all and will be able
        //        to send a multicast request to the broker.
        let outgoing_request = OutgoingRequest::unicast(payload, props, reqp, API_VERSION);
        Ok(vec![Box::new(outgoing_request)])
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct LeaveRequest {
    id: Uuid,
}

pub(crate) struct LeaveHandler;

#[async_trait]
impl RequestHandler for LeaveHandler {
    type Payload = LeaveRequest;
    const ERROR_TITLE: &'static str = "Failed to leave room";

    async fn handle(
        context: &Context,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError> {
        let conn = context.db().get()?;

        let room = find_room!(
            conn,
            payload.id,
            reqp,
            start_timestamp,
            LeaveHandler::ERROR_TITLE
        );

        // Check room presence.
        let results = agent::ListQuery::new()
            .room_id(room.id())
            .agent_id(reqp.as_agent_id())
            .status(agent::Status::Ready)
            .execute(&conn)?;

        if results.len() == 0 {
            return Err(svc_error!(
                ResponseStatus::NOT_FOUND,
                "agent = '{}' is not online in the room = '{}'",
                reqp.as_agent_id(),
                room.id()
            ));
        }

        // Send dynamic subscription deletion request to the broker.
        let room_id = room.id().to_string();

        let payload = SubscriptionRequest::new(
            reqp.as_agent_id().to_owned(),
            vec!["rooms", &room_id, "events"],
        );

        let props = reqp.to_request(
            "subscription.delete",
            reqp.response_topic(),
            reqp.correlation_data(),
            ShortTermTimingProperties::until_now(start_timestamp),
        );

        // FIXME: It looks like sending a request to the client but the broker intercepts it
        //        deletes the subscription and replaces the request with the response.
        //        This is kind of ugly but it guaranties that the request will be processed by
        //        the broker node where the client is connected to. We need that because
        //        the request changes local state on that node.
        //        A better solution will be possible after resolution of this issue:
        //        https://github.com/vernemq/vernemq/issues/1326.
        //        Then we won't need the local state on the broker at all and will be able
        //        to send a multicast request to the broker.
        let outgoing_request = OutgoingRequest::unicast(payload, props, reqp, API_VERSION);
        Ok(vec![Box::new(outgoing_request)])
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
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError> {
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

        // TODO: Authorize tenant account by room audience.

        let mut agent = context.agent().clone();
        let db = context.db().to_owned();
        let id = room.id();

        context
            .thread_pool()
            .spawn(async move {
                let future = adjust_room(
                    &db,
                    &room,
                    payload.started_at,
                    &payload.segments,
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
                svc_error!(
                    ResponseStatus::INTERNAL_SERVER_ERROR,
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
            None,
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
