use std::ops::Bound;

use anyhow::Context as AnyhowContext;
use async_std::prelude::*;
use async_std::stream;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::FutureExt;
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use svc_agent::{
    mqtt::{
        IncomingRequestProperties, IntoPublishableMessage, OutgoingEvent, OutgoingEventProperties,
        OutgoingRequest, ResponseStatus, ShortTermTimingProperties,
    },
    Addressable, AgentId,
};
use svc_error::{extension::sentry, Error as SvcError};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::app::operations::adjust_room;
use crate::db::adjustment::Segments;
use crate::db::agent;
use crate::db::room::{InsertQuery, UpdateQuery};

type Time = (Bound<DateTime<Utc>>, Bound<DateTime<Utc>>);

///////////////////////////////////////////////////////////////////////////////

const MQTT_GW_API_VERSION: &str = "v1";

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

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        // Validate opening time.
        match payload.time {
            (Bound::Included(opened_at), Bound::Excluded(closed_at)) if closed_at > opened_at => (),
            _ => return Err(anyhow!("Invalid room time")).error(AppErrorKind::InvalidRoomTime),
        }

        // Authorize room creation on the tenant.
        let authz_time = context
            .authz()
            .authorize(&payload.audience, reqp, vec!["rooms"], "create")
            .await?;

        // Insert room.
        let room = {
            let mut query = InsertQuery::new(&payload.audience, payload.time.into());

            if let Some(tags) = payload.tags {
                query = query.tags(tags);
            }

            let mut conn = context.get_conn().await?;

            context
                .profiler()
                .measure(
                    (
                        ProfilerKeys::RoomInsertQuery,
                        Some(reqp.method().to_owned()),
                    ),
                    query.execute(&mut conn),
                )
                .await
                .context("Failed to insert room")
                .error(AppErrorKind::DbQueryFailed)?
        };

        // Respond and broadcast to the audience topic.
        let response = helpers::build_response(
            ResponseStatus::CREATED,
            room.clone(),
            reqp,
            context.start_timestamp(),
            Some(authz_time),
        );

        let notification = helpers::build_notification(
            "room.create",
            &format!("audiences/{}/events", payload.audience),
            room,
            reqp,
            context.start_timestamp(),
        );

        Ok(Box::new(stream::from_iter(vec![response, notification])))
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

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        let room = helpers::find_room(
            context,
            payload.id,
            helpers::RoomTimeRequirement::Any,
            reqp.method(),
        )
        .await?;

        // Authorize room reading on the tenant.
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id];

        let authz_time = context
            .authz()
            .authorize(room.audience(), reqp, object, "read")
            .await?;

        Ok(Box::new(stream::once(helpers::build_response(
            ResponseStatus::OK,
            room,
            reqp,
            context.start_timestamp(),
            Some(authz_time),
        ))))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateRequest {
    id: Uuid,
    #[serde(default)]
    #[serde(with = "crate::serde::ts_seconds_option_bound_tuple")]
    time: Option<Time>,
    tags: Option<JsonValue>,
}

pub(crate) struct UpdateHandler;

#[async_trait]
impl RequestHandler for UpdateHandler {
    type Payload = UpdateRequest;

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        let time_requirement = if payload.time.is_some() {
            // Forbid changing time of a closed room.
            helpers::RoomTimeRequirement::NotClosed
        } else {
            helpers::RoomTimeRequirement::Any
        };

        let room = helpers::find_room(context, payload.id, time_requirement, reqp.method()).await?;

        // Authorize room reading on the tenant.
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id];

        let authz_time = context
            .authz()
            .authorize(room.audience(), reqp, object, "update")
            .await?;

        // Validate opening time.
        let mut time = payload.time;
        if let Some(new_time) = time {
            match new_time {
                (Bound::Included(new_opened_at), Bound::Excluded(new_closed_at))
                    if new_closed_at > new_opened_at =>
                {
                    if let (Bound::Included(opened_at), _) = room.time() {
                        if opened_at <= Utc::now() {
                            let new_closed_at = if new_closed_at <= Utc::now() {
                                Utc::now()
                            } else {
                                new_closed_at
                            };

                            time =
                                Some((Bound::Included(opened_at), Bound::Excluded(new_closed_at)));
                        }
                    }
                }
                _ => return Err(anyhow!("Invalid room time")).error(AppErrorKind::InvalidRoomTime),
            }
        }

        let room_was_open = !room.is_closed();

        // Update room.
        let room = {
            let query = UpdateQuery::new(room.id())
                .time(time.map(|t| t.into()))
                .tags(payload.tags);

            let mut conn = context.get_conn().await?;

            context
                .profiler()
                .measure(
                    (
                        ProfilerKeys::RoomUpdateQuery,
                        Some(reqp.method().to_owned()),
                    ),
                    query.execute(&mut conn),
                )
                .await
                .with_context(|| format!("Failed to update room, id = '{}'", room.id()))
                .error(AppErrorKind::DbQueryFailed)?
        };

        // Respond and broadcast to the audience topic.
        let response = helpers::build_response(
            ResponseStatus::OK,
            room.clone(),
            reqp,
            context.start_timestamp(),
            Some(authz_time),
        );

        let notification = helpers::build_notification(
            "room.update",
            &format!("audiences/{}/events", room.audience()),
            room.clone(),
            reqp,
            context.start_timestamp(),
        );

        let mut responses = vec![response, notification];

        let append_closed_notification = || {
            let closed_notification = helpers::build_notification(
                "room.close",
                &format!("rooms/{}/events", room.id()),
                room,
                reqp,
                context.start_timestamp(),
            );
            responses.push(closed_notification);
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

        Ok(Box::new(stream::from_iter(responses)))
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

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        let room = helpers::find_room(
            context,
            payload.id,
            helpers::RoomTimeRequirement::Open,
            reqp.method(),
        )
        .await?;

        // Authorize subscribing to the room's events.
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id, "events"];

        let authz_time = context
            .authz()
            .authorize(room.audience(), reqp, object.clone(), "subscribe")
            .await?;

        // Register agent in `in_progress` state.
        {
            let mut conn = context.get_conn().await?;
            let query = agent::InsertQuery::new(reqp.as_agent_id().to_owned(), room.id());

            context
                .profiler()
                .measure(
                    (
                        ProfilerKeys::AgentInsertQuery,
                        Some(reqp.method().to_owned()),
                    ),
                    query.execute(&mut conn),
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to insert agent into room, agent_id = '{}', room_id = '{}'",
                        reqp.as_agent_id(),
                        room.id(),
                    )
                })
                .error(AppErrorKind::DbQueryFailed)?;
        }

        // Send dynamic subscription creation request to the broker.
        let payload = SubscriptionRequest::new(reqp.as_agent_id().to_owned(), object);
        let start_timestamp = context.start_timestamp();
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
        let boxed_request = Box::new(outgoing_request) as Box<dyn IntoPublishableMessage + Send>;
        Ok(Box::new(stream::once(boxed_request)))
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

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        let (room, presence) = {
            let room = helpers::find_room(
                context,
                payload.id,
                helpers::RoomTimeRequirement::Any,
                reqp.method(),
            )
            .await?;

            // Check room presence.
            let query = agent::ListQuery::new()
                .room_id(room.id())
                .agent_id(reqp.as_agent_id().to_owned())
                .status(agent::Status::Ready);

            let mut conn = context.get_ro_conn().await?;

            let presence = context
                .profiler()
                .measure(
                    (ProfilerKeys::AgentListQuery, Some(reqp.method().to_owned())),
                    query.execute(&mut conn),
                )
                .await
                .with_context(|| format!("Failed to list agents, room_id = '{}'", payload.id))
                .error(AppErrorKind::DbQueryFailed)?;

            (room, presence)
        };

        if presence.is_empty() {
            return Err(anyhow!(
                "agent = '{}' is not online in the room = '{}'",
                reqp.as_agent_id(),
                room.id()
            ))
            .error(AppErrorKind::AgentNotEnteredTheRoom);
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
            ShortTermTimingProperties::until_now(context.start_timestamp()),
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
        let boxed_request = Box::new(outgoing_request) as Box<dyn IntoPublishableMessage + Send>;
        Ok(Box::new(stream::once(boxed_request)))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct AdjustRequest {
    id: Uuid,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    started_at: DateTime<Utc>,
    #[serde(with = "crate::db::adjustment::serde::segments")]
    segments: Segments,
    offset: i64,
}

pub(crate) struct AdjustHandler;

#[async_trait]
impl RequestHandler for AdjustHandler {
    type Payload = AdjustRequest;

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        // Find realtime room.
        let room = helpers::find_room(
            context,
            payload.id,
            helpers::RoomTimeRequirement::Any,
            reqp.method(),
        )
        .await?;

        // Authorize trusted account for the room's audience.
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id];

        let authz_time = context
            .authz()
            .authorize(room.audience(), reqp, object, "update")
            .await?;

        // Run asynchronous task for adjustment.
        let db = context.db().to_owned();
        let profiler = context.profiler();
        let logger = context.logger().new(o!());

        let notification_future = async_std::task::spawn(async move {
            let operation_result = adjust_room(
                &db,
                &profiler,
                &room,
                payload.started_at,
                &payload.segments,
                payload.offset,
            )
            .await;

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
                        logger,
                        "Room adjustment job failed for room_id = '{}': {}",
                        room.id(),
                        err
                    );

                    let app_error = AppError::new(AppErrorKind::RoomAdjustTaskFailed, err);
                    let svc_error: SvcError = app_error.into();

                    sentry::send(svc_error.clone()).unwrap_or_else(|err| {
                        warn!(logger, "Error sending error to Sentry: {}", err)
                    });

                    RoomAdjustResult::Error { error: svc_error }
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

            Box::new(event) as Box<dyn IntoPublishableMessage + Send>
        });

        // Respond with 202.
        // The actual task result will be broadcasted to the room topic when finished.
        let response = stream::once(helpers::build_response(
            ResponseStatus::ACCEPTED,
            json!({}),
            reqp,
            context.start_timestamp(),
            Some(authz_time),
        ));

        let notification = notification_future.into_stream();
        Ok(Box::new(response.chain(notification)))
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
        #[serde(with = "crate::db::adjustment::serde::segments")]
        modified_segments: Segments,
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
    use serde_derive::Deserialize;

    use super::AgentId;

    #[derive(Deserialize)]
    struct DynSubRequest {
        subject: AgentId,
        object: Vec<String>,
    }

    mod create {
        use std::ops::Bound;

        use chrono::{Duration, SubsecRound, Utc};
        use serde_json::json;

        use crate::db::room::Object as Room;
        use crate::test_helpers::prelude::*;

        use super::super::*;

        #[test]
        fn create_room() {
            async_std::task::block_on(async {
                // Allow agent to create rooms.
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let mut authz = TestAuthz::new();
                authz.allow(agent.account_id(), vec!["rooms"], "create");

                // Make room.create request.
                let context = TestContext::new(TestDb::new().await, authz);
                let now = Utc::now().trunc_subsecs(0);

                let time = (
                    Bound::Included(now + Duration::hours(1)),
                    Bound::Excluded(now + Duration::hours(2)),
                );

                let tags = json!({ "webinar_id": "123" });

                let payload = CreateRequest {
                    time: Time::from(time),
                    audience: USR_AUDIENCE.to_owned(),
                    tags: Some(tags.clone()),
                };

                let messages = handle_request::<CreateHandler>(&context, &agent, payload)
                    .await
                    .expect("Room creation failed");

                // Assert response.
                let (room, respp) = find_response::<Room>(messages.as_slice());
                assert_eq!(respp.status(), ResponseStatus::CREATED);
                assert_eq!(room.audience(), USR_AUDIENCE);
                assert_eq!(room.time(), time);
                assert_eq!(room.tags(), Some(&tags));

                // Assert notification.
                let (room, evp, topic) = find_event::<Room>(messages.as_slice());
                assert!(topic.ends_with(&format!("/audiences/{}/events", USR_AUDIENCE)));
                assert_eq!(evp.label(), "room.create");
                assert_eq!(room.audience(), USR_AUDIENCE);
                assert_eq!(room.time(), time);
                assert_eq!(room.tags(), Some(&tags));
            });
        }

        #[test]
        fn create_room_not_authorized() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                // Make room.create request.
                let context = TestContext::new(TestDb::new().await, TestAuthz::new());
                let now = Utc::now().trunc_subsecs(0);

                let time = (
                    Bound::Included(now + Duration::hours(1)),
                    Bound::Excluded(now + Duration::hours(2)),
                );

                let payload = CreateRequest {
                    time: time.clone(),
                    audience: USR_AUDIENCE.to_owned(),
                    tags: None,
                };

                let err = handle_request::<CreateHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room creation");

                assert_eq!(err.status_code(), ResponseStatus::FORBIDDEN);
            });
        }

        #[test]
        fn create_room_invalid_time() {
            async_std::task::block_on(async {
                // Allow agent to create rooms.
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let mut authz = TestAuthz::new();
                authz.allow(agent.account_id(), vec!["rooms"], "create");

                // Make room.create request.
                let context = TestContext::new(TestDb::new().await, TestAuthz::new());

                let payload = CreateRequest {
                    time: (Bound::Unbounded, Bound::Unbounded),
                    audience: USR_AUDIENCE.to_owned(),
                    tags: None,
                };

                let err = handle_request::<CreateHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room creation");

                assert_eq!(err.status_code(), ResponseStatus::BAD_REQUEST);
                assert_eq!(err.kind(), "invalid_room_time");
            });
        }
    }

    mod read {
        use crate::db::room::Object as Room;
        use crate::test_helpers::prelude::*;

        use super::super::*;

        #[test]
        fn read_room() {
            async_std::task::block_on(async {
                let db = TestDb::new().await;

                let room = {
                    // Create room.
                    let mut conn = db.get_conn().await;
                    shared_helpers::insert_room(&mut conn).await
                };

                // Allow agent to read the room.
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let mut authz = TestAuthz::new();
                let room_id = room.id().to_string();
                authz.allow(agent.account_id(), vec!["rooms", &room_id], "read");

                // Make room.read request.
                let context = TestContext::new(db, authz);
                let payload = ReadRequest { id: room.id() };

                let messages = handle_request::<ReadHandler>(&context, &agent, payload)
                    .await
                    .expect("Room reading failed");

                // Assert response.
                let (resp_room, respp) = find_response::<Room>(messages.as_slice());
                assert_eq!(respp.status(), ResponseStatus::OK);
                assert_eq!(resp_room.audience(), room.audience());
                assert_eq!(resp_room.time(), room.time());
                assert_eq!(resp_room.tags(), room.tags());
            });
        }

        #[test]
        fn read_room_not_authorized() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let db = TestDb::new().await;

                let room = {
                    // Create room.
                    let mut conn = db.get_conn().await;
                    shared_helpers::insert_room(&mut conn).await
                };

                // Make room.read request.
                let context = TestContext::new(db, TestAuthz::new());
                let payload = ReadRequest { id: room.id() };

                let err = handle_request::<ReadHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room reading");

                assert_eq!(err.status_code(), ResponseStatus::FORBIDDEN);
            });
        }

        #[test]
        fn read_room_missing() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let context = TestContext::new(TestDb::new().await, TestAuthz::new());
                let payload = ReadRequest { id: Uuid::new_v4() };

                let err = handle_request::<ReadHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room reading");

                assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
                assert_eq!(err.kind(), "room_not_found");
            });
        }
    }

    mod update {
        use std::ops::Bound;

        use chrono::{Duration, SubsecRound, Utc};

        use crate::db::room::Object as Room;
        use crate::test_helpers::prelude::*;

        use super::super::*;

        #[test]
        fn update_room() {
            async_std::task::block_on(async {
                let db = TestDb::new().await;
                let now = Utc::now().trunc_subsecs(0);

                let room = {
                    let mut conn = db.get_conn().await;

                    // Create room.
                    factory::Room::new()
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
                let room_id = room.id().to_string();
                authz.allow(agent.account_id(), vec!["rooms", &room_id], "update");

                // Make room.update request.
                let context = TestContext::new(db, authz);

                let time = (
                    Bound::Included(now + Duration::hours(2)),
                    Bound::Excluded(now + Duration::hours(3)),
                );

                let tags = json!({"webinar_id": "456789"});

                let payload = UpdateRequest {
                    id: room.id(),
                    time: Some(time),
                    tags: Some(tags.clone()),
                };

                let messages = handle_request::<UpdateHandler>(&context, &agent, payload)
                    .await
                    .expect("Room update failed");

                // Assert response.
                let (resp_room, respp) = find_response::<Room>(messages.as_slice());
                assert_eq!(respp.status(), ResponseStatus::OK);
                assert_eq!(resp_room.id(), room.id());
                assert_eq!(resp_room.audience(), room.audience());
                assert_eq!(resp_room.time(), time);
                assert_eq!(resp_room.tags(), Some(&tags));
            });
        }

        #[test]
        fn update_closed_at_in_open_room() {
            async_std::task::block_on(async {
                let db = TestDb::new().await;
                let now = Utc::now().trunc_subsecs(0);

                let room = {
                    let mut conn = db.get_conn().await;

                    // Create room.
                    factory::Room::new()
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
                let room_id = room.id().to_string();
                authz.allow(agent.account_id(), vec!["rooms", &room_id], "update");

                // Make room.update request.
                let context = TestContext::new(db, authz);

                let time = (
                    Bound::Included(now + Duration::hours(1)),
                    Bound::Excluded(now + Duration::hours(3)),
                );

                let payload = UpdateRequest {
                    id: room.id(),
                    time: Some(time),
                    tags: None,
                };

                let messages = handle_request::<UpdateHandler>(&context, &agent, payload)
                    .await
                    .expect("Room update failed");

                let (resp_room, respp) = find_response::<Room>(messages.as_slice());
                assert_eq!(respp.status(), ResponseStatus::OK);
                assert_eq!(resp_room.id(), room.id());
                assert_eq!(resp_room.audience(), room.audience());
                assert_eq!(
                    resp_room.time(),
                    (
                        Bound::Included(now - Duration::hours(1)),
                        Bound::Excluded(now + Duration::hours(3)),
                    )
                );
            });
        }

        #[test]
        fn update_closed_at_in_the_past_in_already_open_room() {
            async_std::task::block_on(async {
                let db = TestDb::new().await;
                let now = Utc::now().trunc_subsecs(0);

                let room = {
                    let mut conn = db.get_conn().await;

                    // Create room.
                    factory::Room::new()
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
                let room_id = room.id().to_string();
                authz.allow(agent.account_id(), vec!["rooms", &room_id], "update");

                // Make room.update request.
                let context = TestContext::new(db, authz);

                let time = (
                    Bound::Included(now - Duration::hours(2)),
                    Bound::Excluded(now - Duration::hours(1)),
                );

                let payload = UpdateRequest {
                    id: room.id(),
                    time: Some(time),
                    tags: None,
                };

                let messages = handle_request::<UpdateHandler>(&context, &agent, payload)
                    .await
                    .expect("Room update failed");

                let (resp_room, respp) = find_response::<Room>(messages.as_slice());
                assert_eq!(respp.status(), ResponseStatus::OK);
                assert_eq!(resp_room.id(), room.id());
                assert_eq!(resp_room.audience(), room.audience());
                assert_eq!(
                    resp_room.time(),
                    (
                        Bound::Included(now - Duration::hours(2)),
                        Bound::Excluded(now),
                    )
                );

                // since we just closed the room we must receive a room.close event
                let (ev_room, _, _) =
                    find_event_by_predicate::<Room, _>(messages.as_slice(), |evp, _| {
                        evp.label() == "room.close"
                    })
                    .expect("Failed to find room.close event");
                assert_eq!(ev_room.id(), room.id());
            });
        }

        #[test]
        fn update_room_invalid_time() {
            async_std::task::block_on(async {
                let db = TestDb::new().await;
                let now = Utc::now().trunc_subsecs(0);

                let room = {
                    let mut conn = db.get_conn().await;

                    // Create room.
                    factory::Room::new()
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
                let room_id = room.id().to_string();
                authz.allow(agent.account_id(), vec!["rooms", &room_id], "update");

                // Make room.update request.
                let context = TestContext::new(db, authz);

                let time = (
                    Bound::Included(now + Duration::hours(1)),
                    Bound::Excluded(now - Duration::hours(2)),
                );

                let payload = UpdateRequest {
                    id: room.id(),
                    time: Some(time),
                    tags: None,
                };

                let err = handle_request::<UpdateHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room update");

                assert_eq!(err.status_code(), ResponseStatus::BAD_REQUEST);
                assert_eq!(err.kind(), "invalid_room_time");
            });
        }

        #[test]
        fn update_room_not_authorized() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let db = TestDb::new().await;

                let room = {
                    // Create room.
                    let mut conn = db.get_conn().await;
                    shared_helpers::insert_room(&mut conn).await
                };

                // Make room.update request.
                let context = TestContext::new(db, TestAuthz::new());
                let payload = UpdateRequest {
                    id: room.id(),
                    time: None,
                    tags: None,
                };

                let err = handle_request::<UpdateHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room update");

                assert_eq!(err.status_code(), ResponseStatus::FORBIDDEN);
            });
        }

        #[test]
        fn update_room_missing() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let context = TestContext::new(TestDb::new().await, TestAuthz::new());

                let payload = UpdateRequest {
                    id: Uuid::new_v4(),
                    time: None,
                    tags: None,
                };

                let err = handle_request::<UpdateHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room update");

                assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
                assert_eq!(err.kind(), "room_not_found");
            });
        }

        #[test]
        fn update_room_closed() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let db = TestDb::new().await;

                let room = {
                    // Create closed room.
                    let mut conn = db.get_conn().await;
                    shared_helpers::insert_closed_room(&mut conn).await
                };

                let context = TestContext::new(TestDb::new().await, TestAuthz::new());
                let now = Utc::now().trunc_subsecs(0);

                let time = (
                    Bound::Included(now - Duration::hours(2)),
                    Bound::Excluded(now - Duration::hours(1)),
                );

                let payload = UpdateRequest {
                    id: room.id(),
                    time: Some(time.into()),
                    tags: None,
                };

                let err = handle_request::<UpdateHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room update");

                assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
                assert_eq!(err.kind(), "room_closed");
            });
        }
    }

    mod enter {
        use crate::test_helpers::prelude::*;

        use super::super::*;
        use super::DynSubRequest;

        #[test]
        fn enter_room() {
            async_std::task::block_on(async {
                let db = TestDb::new().await;

                let room = {
                    // Create room.
                    let mut conn = db.get_conn().await;
                    shared_helpers::insert_room(&mut conn).await
                };

                // Allow agent to subscribe to the rooms' events.
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

                let messages = handle_request::<EnterHandler>(&context, &agent, payload)
                    .await
                    .expect("Room entrance failed");

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

        #[test]
        fn enter_room_not_authorized() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let db = TestDb::new().await;

                let room = {
                    // Create room.
                    let mut conn = db.get_conn().await;
                    shared_helpers::insert_room(&mut conn).await
                };

                // Make room.enter request.
                let context = TestContext::new(db, TestAuthz::new());
                let payload = EnterRequest { id: room.id() };

                let err = handle_request::<EnterHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room entering");

                assert_eq!(err.status_code(), ResponseStatus::FORBIDDEN);
            });
        }

        #[test]
        fn enter_room_missing() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let context = TestContext::new(TestDb::new().await, TestAuthz::new());
                let payload = EnterRequest { id: Uuid::new_v4() };

                let err = handle_request::<EnterHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room entering");

                assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
                assert_eq!(err.kind(), "room_not_found");
            });
        }

        #[test]
        fn enter_room_closed() {
            async_std::task::block_on(async {
                let db = TestDb::new().await;

                let room = {
                    // Create closed room.
                    let mut conn = db.get_conn().await;
                    shared_helpers::insert_closed_room(&mut conn).await
                };

                // Allow agent to subscribe to the rooms' events.
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let mut authz = TestAuthz::new();
                let room_id = room.id().to_string();

                authz.allow(
                    agent.account_id(),
                    vec!["rooms", &room_id, "events"],
                    "subscribe",
                );

                // Make room.enter request.
                let context = TestContext::new(db, TestAuthz::new());
                let payload = EnterRequest { id: room.id() };

                let err = handle_request::<EnterHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room entering");

                assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
                assert_eq!(err.kind(), "room_closed");
            });
        }
    }

    mod leave {
        use crate::test_helpers::prelude::*;

        use super::super::*;
        use super::DynSubRequest;

        #[test]
        fn leave_room() {
            async_std::task::block_on(async {
                let db = TestDb::new().await;
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let room = {
                    // Create room.
                    let mut conn = db.get_conn().await;
                    let room = shared_helpers::insert_room(&mut conn).await;

                    // Put agent online in the room.
                    shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
                    room
                };

                // Make room.leave request.
                let context = TestContext::new(db, TestAuthz::new());
                let payload = LeaveRequest { id: room.id() };

                let messages = handle_request::<LeaveHandler>(&context, &agent, payload)
                    .await
                    .expect("Room leaving failed");

                // Assert dynamic subscription request.
                let (payload, reqp, topic) = find_request::<DynSubRequest>(messages.as_slice());

                let expected_topic = format!(
                    "agents/{}/api/{}/in/{}",
                    agent.agent_id(),
                    MQTT_GW_API_VERSION,
                    context.config().id,
                );

                assert_eq!(topic, expected_topic);
                assert_eq!(reqp.method(), "subscription.delete");
                assert_eq!(&payload.subject, agent.agent_id());

                let room_id = room.id().to_string();
                assert_eq!(payload.object, vec!["rooms", &room_id, "events"]);
            });
        }

        #[test]
        fn leave_room_while_not_entered() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let db = TestDb::new().await;

                let room = {
                    // Create room.
                    let mut conn = db.get_conn().await;
                    shared_helpers::insert_room(&mut conn).await
                };

                // Make room.leave request.
                let context = TestContext::new(db, TestAuthz::new());
                let payload = LeaveRequest { id: room.id() };

                let err = handle_request::<LeaveHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room leaving");

                assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
                assert_eq!(err.kind(), "agent_not_entered_the_room");
            });
        }

        #[test]
        fn leave_room_missing() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let context = TestContext::new(TestDb::new().await, TestAuthz::new());
                let payload = LeaveRequest { id: Uuid::new_v4() };

                let err = handle_request::<LeaveHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room leaving");

                assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
                assert_eq!(err.kind(), "room_not_found");
            });
        }
    }

    mod adjust {
        use chrono::Utc;

        use crate::test_helpers::prelude::*;

        use super::super::*;

        #[test]
        fn adjust_room_not_authorized() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let db = TestDb::new().await;

                let room = {
                    // Create room.
                    let mut conn = db.get_conn().await;
                    shared_helpers::insert_room(&mut conn).await
                };

                // Make room.adjust request.
                let context = TestContext::new(db, TestAuthz::new());

                let payload = AdjustRequest {
                    id: room.id(),
                    started_at: Utc::now(),
                    segments: vec![].into(),
                    offset: 0,
                };

                let err = handle_request::<AdjustHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room adjustment");

                assert_eq!(err.status_code(), ResponseStatus::FORBIDDEN);
            });
        }

        #[test]
        fn adjust_room_missing() {
            async_std::task::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
                let context = TestContext::new(TestDb::new().await, TestAuthz::new());

                let payload = AdjustRequest {
                    id: Uuid::new_v4(),
                    started_at: Utc::now(),
                    segments: vec![].into(),
                    offset: 0,
                };

                let err = handle_request::<AdjustHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success on room adjustment");

                assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
                assert_eq!(err.kind(), "room_not_found");
            });
        }
    }
}
