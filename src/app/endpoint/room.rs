use async_trait::async_trait;
use chrono::{DateTime, Utc};
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

use crate::app::context::Context;
use crate::app::endpoint::{helpers, RequestHandler};
use crate::app::operations::adjust_room;
use crate::db::adjustment::Segment;
use crate::db::agent;
use crate::db::room::{FindQuery, InsertQuery, Time};

///////////////////////////////////////////////////////////////////////////////

const MQTT_GW_API_VERSION: &'static str = "v1";

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

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError> {
        // Authorize room creation on the tenant.
        let authz_time = context
            .authz()
            .authorize(&payload.audience, reqp, vec!["rooms"], "create")
            .await?;

        // Insert room.
        let room = {
            let mut query = InsertQuery::new(&payload.audience, payload.time);

            if let Some(tags) = payload.tags {
                query = query.tags(tags);
            }

            let conn = context.db().get()?;
            query.execute(&conn)?
        };

        // Respond and broadcast to the audience topic.
        let response = helpers::build_response(
            ResponseStatus::CREATED,
            room.clone(),
            reqp,
            start_timestamp,
            Some(authz_time),
        );

        let notification = helpers::build_notification(
            "room.create",
            &format!("audiences/{}/events", payload.audience),
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

    async fn handle<C: Context>(
        context: &C,
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

        // Authorize room reading on the tenant.
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id];

        let authz_time = context
            .authz()
            .authorize(room.audience(), reqp, object, "read")
            .await?;

        Ok(vec![helpers::build_response(
            ResponseStatus::OK,
            room,
            reqp,
            start_timestamp,
            Some(authz_time),
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

    async fn handle<C: Context>(
        context: &C,
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
        let outgoing_request = OutgoingRequest::unicast(payload, props, reqp, MQTT_GW_API_VERSION);
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

    async fn handle<C: Context>(
        context: &C,
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
        let outgoing_request = OutgoingRequest::unicast(payload, props, reqp, MQTT_GW_API_VERSION);
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

    async fn handle<C: Context>(
        context: &C,
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

        // Authorize trusted account for the room's audience.
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id];

        let authz_time = context
            .authz()
            .authorize(room.audience(), reqp, object, "adjust")
            .await?;

        // Run asynchronous task for adjustment.
        let db = context.db().to_owned();

        context.run_task(async move {
            // Call room adjustment operation.
            let operation_result = adjust_room(
                &db,
                &room,
                payload.started_at,
                &payload.segments,
                payload.offset,
            );

            // Handle result.
            let result = match operation_result {
                Ok((original_room, modified_room, modified_segments)) => {
                    RoomAdjustResult::Success {
                        original_room_id: original_room.id(),
                        modified_room_id: modified_room.id(),
                        modified_segments,
                    }
                }
                Err(err) => {
                    error!(
                        "Room adjustment job failed for room_id = '{}': {}",
                        room.id(),
                        err
                    );

                    let error = SvcError::builder()
                        .status(ResponseStatus::UNPROCESSABLE_ENTITY)
                        .kind("room.adjust", AdjustHandler::ERROR_TITLE)
                        .detail(&err.to_string())
                        .build();

                    sentry::send(error.clone())
                        .unwrap_or_else(|err| warn!("Error sending error to Sentry: {}", err));

                    RoomAdjustResult::Error { error }
                }
            };

            // Publish success/failure notification.
            let notification = RoomAdjustNotification {
                status: result.status(),
                tags: room.tags().map(|t| t.to_owned()),
                result,
            };

            let timing = ShortTermTimingProperties::new(Utc::now());
            let props = OutgoingEventProperties::new("room.adjust", timing);
            let path = format!("audiences/{}/events", room.audience());
            let event = OutgoingEvent::broadcast(notification, props, &path);
            vec![Box::new(event) as Box<dyn IntoPublishableDump>]
        })?;

        // Respond with 202.
        // The actual task result will be broadcasted to the room topic when finished.
        Ok(vec![helpers::build_response(
            ResponseStatus::ACCEPTED,
            json!({}),
            reqp,
            start_timestamp,
            Some(authz_time),
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

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use chrono::{Duration, SubsecRound, Utc};
    use serde_derive::Deserialize;
    use serde_json::json;

    use crate::db::room::Object as Room;
    use crate::test_helpers::prelude::*;

    use super::*;

    #[test]
    fn create_room() {
        futures::executor::block_on(async {
            // Allow user to create rooms.
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut authz = TestAuthz::new();
            authz.allow(agent.account_id(), vec!["rooms"], "create");

            // Make room.create request.
            let context = TestContext::new(TestDb::new(), authz);
            let now = Utc::now().trunc_subsecs(0);

            let time = (
                Bound::Included(now),
                Bound::Excluded(now + Duration::hours(1)),
            );

            let tags = json!({ "webinar_id": "123" });

            let payload = CreateRequest {
                time: time.clone(),
                audience: USR_AUDIENCE.to_owned(),
                tags: Some(tags.clone()),
            };

            let messages = handle_request::<CreateHandler>(&context, &agent, payload).await;

            // Assert response.
            let (room, respp) = find_response::<Room>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::CREATED);
            assert_eq!(room.audience(), USR_AUDIENCE);
            assert_eq!(room.time(), &time);
            assert_eq!(room.tags(), Some(&tags));

            // Assert notification.
            let (room, evp, topic) = find_event::<Room>(messages.as_slice());
            assert!(topic.ends_with(&format!("/audiences/{}/events", USR_AUDIENCE)));
            assert_eq!(evp.label(), "room.create");
            assert_eq!(room.audience(), USR_AUDIENCE);
            assert_eq!(room.time(), &time);
            assert_eq!(room.tags(), Some(&tags));
        });
    }

    #[test]
    fn read_room() {
        futures::executor::block_on(async {
            // Create room.
            let db = TestDb::new();
            let room = insert_room(&db);

            // Allow user to read the room.
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            authz.allow(agent.account_id(), vec!["rooms", &room_id], "read");

            // Make room.read request.
            let context = TestContext::new(db, authz);
            let payload = ReadRequest { id: room.id() };
            let messages = handle_request::<ReadHandler>(&context, &agent, payload).await;

            // Assert response.
            let (resp_room, respp) = find_response::<Room>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(resp_room.audience(), room.audience());
            assert_eq!(resp_room.time(), room.time());
            assert_eq!(resp_room.tags(), room.tags());
        });
    }

    #[derive(Deserialize)]
    struct DynSubRequest {
        subject: AgentId,
        object: Vec<String>,
    }

    #[test]
    fn enter_room() {
        futures::executor::block_on(async {
            // Create room.
            let db = TestDb::new();
            let room = insert_room(&db);

            // Allow user to subscribe to the rooms' events.
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            authz.allow(
                agent.account_id(),
                vec!["rooms", &room_id, "events"],
                "subscribe",
            );

            // Make room.enter request.
            let context = TestContext::new(db, authz);
            let payload = EnterRequest { id: room.id() };
            let messages = handle_request::<EnterHandler>(&context, &agent, payload).await;

            // Assert dynamic subscription request.
            let (payload, reqp, topic) = find_request::<DynSubRequest>(messages.as_slice());

            let expected_topic = format!(
                "agents/{}/api/{}/in/{}",
                agent.agent_id(),
                MQTT_GW_API_VERSION,
                context.config().id,
            );

            assert_eq!(topic, expected_topic);
            assert_eq!(reqp.method(), "subscription.create");
            assert_eq!(payload.subject, agent.agent_id().to_owned());
            assert_eq!(payload.object, vec!["rooms", &room_id, "events"]);
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
