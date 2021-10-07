use std::ops::Bound;

use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::{future, stream, StreamExt};
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use svc_agent::{
    mqtt::{
        IncomingRequestProperties, IntoPublishableMessage, OutgoingEvent, OutgoingEventProperties,
        OutgoingRequest, ResponseStatus, ShortTermTimingProperties, SubscriptionTopic,
    },
    Addressable, AgentId, Subscription,
};
use svc_error::Error as SvcError;
use tracing::{error, instrument};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::app::endpoint::subscription::CorrelationDataPayload;
use crate::app::API_VERSION;
use crate::db::adjustment::Segments;
use crate::db::agent;
use crate::db::room::{InsertQuery, UpdateQuery};
use crate::db::room_time::{BoundedDateTimeTuple, RoomTime};
use crate::{
    app::operations::adjust_room,
    db::event::{insert_agent_action, AgentAction},
};

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Serialize)]
struct SubscriptionRequest {
    subject: AgentId,
    object: Vec<String>,
}

impl SubscriptionRequest {
    fn new(subject: AgentId, object: Vec<String>) -> Self {
        Self { subject, object }
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct CreateRequest {
    audience: String,
    #[serde(with = "crate::serde::ts_seconds_bound_tuple")]
    time: BoundedDateTimeTuple,
    tags: Option<JsonValue>,
    preserve_history: Option<bool>,
    classroom_id: Option<Uuid>,
}

pub(crate) struct CreateHandler;

#[async_trait]
impl RequestHandler for CreateHandler {
    type Payload = CreateRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
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

        let object = AuthzObject::new(&["rooms"]).into();

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
            let mut query = InsertQuery::new(&payload.audience, payload.time.into());

            if let Some(tags) = payload.tags {
                query = query.tags(tags);
            }

            if let Some(preserve_history) = payload.preserve_history {
                query = query.preserve_history(preserve_history);
            }

            if let Some(cid) = payload.classroom_id {
                query = query.classroom_id(cid);
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

        Ok(Box::new(stream::iter(vec![response, notification])))
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

    #[instrument(
        skip_all,
        fields(
            room_id = %payload.id, scope, classroom_id
        )
    )]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        let room =
            helpers::find_room(context, payload.id, helpers::RoomTimeRequirement::Any).await?;

        // Authorize room reading on the tenant.
        let object = AuthzObject::room(&room).into();

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "read".into(),
            )
            .await?;

        Ok(Box::new(stream::once(future::ready(
            helpers::build_response(
                ResponseStatus::OK,
                room,
                reqp,
                context.start_timestamp(),
                Some(authz_time),
            ),
        ))))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateRequest {
    id: Uuid,
    #[serde(default)]
    #[serde(with = "crate::serde::ts_seconds_option_bound_tuple")]
    time: Option<BoundedDateTimeTuple>,
    tags: Option<JsonValue>,
    classroom_id: Option<Uuid>,
}

pub(crate) struct UpdateHandler;

#[async_trait]
impl RequestHandler for UpdateHandler {
    type Payload = UpdateRequest;

    #[instrument(
        skip_all,
        fields(
            room_id = %payload.id, scope, classroom_id
        )
    )]
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

        let room = helpers::find_room(context, payload.id, time_requirement).await?;

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

        Ok(Box::new(stream::iter(responses)))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct EnterRequest {
    id: Uuid,
    #[serde(default)]
    broadcast_subscription: bool,
}

pub(crate) struct EnterHandler;

#[async_trait]
impl RequestHandler for EnterHandler {
    type Payload = EnterRequest;

    #[instrument(
        skip_all,
        fields(
            room_id = %payload.id, scope, classroom_id
        )
    )]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        let room =
            helpers::find_room(context, payload.id, helpers::RoomTimeRequirement::Open).await?;

        // Authorize subscribing to the room's events.
        let room_id = room.id().to_string();

        let object: Box<dyn svc_authz::IntentObject> =
            AuthzObject::new(&["rooms", &room_id]).into();

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "read".into(),
            )
            .await?;

        // Register agent in `in_progress` state.
        {
            let mut conn = context.get_conn().await?;
            let query = agent::InsertQuery::new(reqp.as_agent_id().to_owned(), room.id());

            context
                .metrics()
                .measure_query(QueryKey::AgentInsertQuery, query.execute(&mut conn))
                .await
                .context("Failed to insert agent into room")
                .error(AppErrorKind::DbQueryFailed)?;
            context
                .metrics()
                .measure_query(
                    QueryKey::EventInsertQuery,
                    insert_agent_action(&room, AgentAction::Enter, reqp.as_agent_id(), &mut conn),
                )
                .await
                .context("Failed to insert agent action")
                .error(AppErrorKind::DbQueryFailed)?;
        }

        let mut requests = Vec::with_capacity(2);
        // Send dynamic subscription creation request to the broker.
        let subscribe_req =
            subscription_request(context, reqp, &room_id, "subscription.create", authz_time)?;

        requests.push(subscribe_req);

        let broadcast_subscribe_req = subscription_request(
            context,
            reqp,
            &room_id,
            "broadcast_subscription.create",
            authz_time,
        )?;

        requests.push(broadcast_subscribe_req);

        Ok(Box::new(stream::iter(requests)))
    }
}

fn subscription_request<C: Context>(
    context: &mut C,
    reqp: &IncomingRequestProperties,
    room_id: &str,
    method: &str,
    authz_time: chrono::Duration,
) -> std::result::Result<Box<dyn IntoPublishableMessage + Send>, AppError> {
    let subject = reqp.as_agent_id().to_owned();
    let object = vec![
        "rooms".to_string(),
        room_id.to_owned(),
        "events".to_string(),
    ];
    let payload = SubscriptionRequest::new(subject.clone(), object.clone());

    let broker_id = AgentId::new("nevermind", context.config().broker_id.to_owned());

    let response_topic = Subscription::unicast_responses_from(&broker_id)
        .subscription_topic(context.agent_id(), API_VERSION)
        .context("Failed to build response topic")
        .error(AppErrorKind::BrokerRequestFailed)?;

    let corr_data_payload = CorrelationDataPayload::new(reqp.to_owned(), subject, object);

    let corr_data = CorrelationData::SubscriptionCreate(corr_data_payload)
        .dump()
        .context("Failed to dump correlation data")
        .error(AppErrorKind::BrokerRequestFailed)?;

    let mut timing = ShortTermTimingProperties::until_now(context.start_timestamp());
    timing.set_authorization_time(authz_time);

    let props = reqp.to_request(method, &response_topic, &corr_data, timing);
    let to = &context.config().broker_id;
    let outgoing_request = OutgoingRequest::multicast(payload, props, to, API_VERSION);
    let boxed_request = Box::new(outgoing_request) as Box<dyn IntoPublishableMessage + Send>;
    Ok(boxed_request)
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

    #[instrument(
        skip_all,
        fields(
            room_id = %payload.id, scope, classroom_id
        )
    )]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        let (room, presence) = {
            let room =
                helpers::find_room(context, payload.id, helpers::RoomTimeRequirement::Any).await?;

            // Check room presence.
            let query = agent::ListQuery::new()
                .room_id(room.id())
                .agent_id(reqp.as_agent_id().to_owned())
                .status(agent::Status::Ready);

            let mut conn = context.get_ro_conn().await?;

            let presence = context
                .metrics()
                .measure_query(QueryKey::AgentListQuery, query.execute(&mut conn))
                .await
                .context("Failed to list agents")
                .error(AppErrorKind::DbQueryFailed)?;

            (room, presence)
        };

        if presence.is_empty() {
            return Err(anyhow!("Agent is not online in the room"))
                .error(AppErrorKind::AgentNotEnteredTheRoom);
        }
        // Send dynamic subscription deletion request to the broker.
        let subject = reqp.as_agent_id().to_owned();
        let room_id = room.id().to_string();
        let object = vec![String::from("rooms"), room_id, String::from("events")];
        let payload = SubscriptionRequest::new(subject.clone(), object.clone());

        let broker_id = AgentId::new("nevermind", context.config().broker_id.to_owned());

        let response_topic = Subscription::unicast_responses_from(&broker_id)
            .subscription_topic(context.agent_id(), API_VERSION)
            .context("Failed to build response topic")
            .error(AppErrorKind::BrokerRequestFailed)?;

        let corr_data_payload = CorrelationDataPayload::new(reqp.to_owned(), subject, object);

        let corr_data = CorrelationData::SubscriptionDelete(corr_data_payload)
            .dump()
            .context("Failed to dump correlation data")
            .error(AppErrorKind::BrokerRequestFailed)?;

        let timing = ShortTermTimingProperties::until_now(context.start_timestamp());

        let props = reqp.to_request("subscription.delete", &response_topic, &corr_data, timing);
        let to = &context.config().broker_id;
        let outgoing_request = OutgoingRequest::multicast(payload, props, to, API_VERSION);
        let boxed_request = Box::new(outgoing_request) as Box<dyn IntoPublishableMessage + Send>;
        Ok(Box::new(stream::once(future::ready(boxed_request))))
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

    #[instrument(
        skip_all,
        fields(
            room_id = %payload.id, scope, classroom_id
        )
    )]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        // Find realtime room.
        let room =
            helpers::find_room(context, payload.id, helpers::RoomTimeRequirement::Any).await?;

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

        let notification_future = tokio::task::spawn(async move {
            let operation_result = adjust_room(
                &db,
                &metrics,
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
                    error!("Room adjustment job failed: {:?}", err);
                    let app_error = AppError::new(AppErrorKind::RoomAdjustTaskFailed, err);
                    app_error.notify_sentry();
                    RoomAdjustResult::Error {
                        error: app_error.to_svc_error(),
                    }
                }
            };

            // Publish success/failure notification.
            let notification = RoomAdjustNotification {
                room_id: payload.id,
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
        let response = stream::once(future::ready(helpers::build_response(
            ResponseStatus::ACCEPTED,
            json!({}),
            reqp,
            context.start_timestamp(),
            Some(authz_time),
        )));

        let notification = notification_future.into_chainable_stream();
        Ok(Box::new(response.chain(notification)))
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

pub(crate) use dump_events::EventsDumpHandler;

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

        #[tokio::test]
        async fn create_room() {
            // Allow agent to create rooms.
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut authz = TestAuthz::new();
            authz.allow(agent.account_id(), vec!["rooms"], "create");

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
                classroom_id: None,
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
            authz.allow(agent.account_id(), vec!["rooms"], "create");

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
                classroom_id: None,
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
            authz.allow(agent.account_id(), vec!["rooms"], "create");

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
                classroom_id: Some(cid),
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
            assert_eq!(room.classroom_id(), Some(cid));

            // Assert notification.
            let (room, evp, topic) = find_event::<Room>(messages.as_slice());
            assert!(topic.ends_with(&format!("/audiences/{}/events", USR_AUDIENCE)));
            assert_eq!(evp.label(), "room.create");
            assert_eq!(room.audience(), USR_AUDIENCE);
            assert_eq!(room.time().map(|t| t.into()), Ok(time));
            assert_eq!(room.tags(), Some(&tags));
            assert_eq!(room.preserve_history(), false);
            assert_eq!(room.classroom_id(), Some(cid));
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
                classroom_id: None,
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
            authz.allow(agent.account_id(), vec!["rooms"], "create");

            // Make room.create request.
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

            let payload = CreateRequest {
                time: (Bound::Unbounded, Bound::Unbounded),
                audience: USR_AUDIENCE.to_owned(),
                tags: None,
                preserve_history: None,
                classroom_id: None,
            };

            let err = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on room creation");

            assert_eq!(err.status(), ResponseStatus::BAD_REQUEST);
            assert_eq!(err.kind(), "invalid_room_time");
        }
    }

    mod read {
        use crate::db::room::Object as Room;
        use crate::test_helpers::prelude::*;

        use super::super::*;

        #[tokio::test]
        async fn read_room() {
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
            let mut context = TestContext::new(db, authz);
            let payload = ReadRequest { id: room.id() };

            let messages = handle_request::<ReadHandler>(&mut context, &agent, payload)
                .await
                .expect("Room reading failed");

            // Assert response.
            let (resp_room, respp, _) = find_response::<Room>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(resp_room.audience(), room.audience());
            assert_eq!(resp_room.time(), room.time());
            assert_eq!(resp_room.tags(), room.tags());
            assert_eq!(resp_room.preserve_history(), room.preserve_history());
        }

        #[tokio::test]
        async fn read_room_not_authorized() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let db = TestDb::new().await;

            let room = {
                // Create room.
                let mut conn = db.get_conn().await;
                shared_helpers::insert_room(&mut conn).await
            };

            // Make room.read request.
            let mut context = TestContext::new(db, TestAuthz::new());
            let payload = ReadRequest { id: room.id() };

            let err = handle_request::<ReadHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on room reading");

            assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
        }

        #[tokio::test]
        async fn read_room_missing() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());
            let payload = ReadRequest { id: Uuid::new_v4() };

            let err = handle_request::<ReadHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on room reading");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_not_found");
        }
    }

    mod update {
        use std::ops::Bound;

        use chrono::{Duration, SubsecRound, Utc};

        use crate::db::room::Object as Room;
        use crate::db::room_time::RoomTimeBound;
        use crate::test_helpers::prelude::*;

        use super::super::*;

        #[tokio::test]
        async fn update_room() {
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
            let mut context = TestContext::new(db, authz);

            let time = (
                Bound::Included(now + Duration::hours(2)),
                Bound::Excluded(now + Duration::hours(3)),
            );

            let tags = json!({"webinar_id": "456789"});

            let payload = UpdateRequest {
                id: room.id(),
                time: Some(time),
                tags: Some(tags.clone()),
                classroom_id: None,
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
            let mut context = TestContext::new(db, authz);

            let time = (
                Bound::Included(now + Duration::hours(1)),
                Bound::Excluded(now + Duration::hours(3)),
            );

            let payload = UpdateRequest {
                id: room.id(),
                time: Some(time),
                tags: None,
                classroom_id: None,
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
            let mut context = TestContext::new(db, authz);

            let time = (
                Bound::Included(now - Duration::hours(2)),
                Bound::Excluded(now - Duration::hours(1)),
            );

            let payload = UpdateRequest {
                id: room.id(),
                time: Some(time),
                tags: None,
                classroom_id: None,
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
                    // Less than 1 second apart is basically 'now'
                    // avoids intermittent failures
                    assert!(x.num_seconds().abs() < 1);
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
            let mut context = TestContext::new(db, authz);

            let time = (
                Bound::Included(now + Duration::hours(1)),
                Bound::Excluded(now - Duration::hours(2)),
            );

            let payload = UpdateRequest {
                id: room.id(),
                time: Some(time),
                tags: None,
                classroom_id: None,
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
                time: None,
                tags: None,
                classroom_id: None,
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
                time: None,
                tags: None,
                classroom_id: None,
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
                time: Some(time.into()),
                tags: None,
                classroom_id: None,
            };

            let err = handle_request::<UpdateHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on room update");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_closed");
        }
    }

    mod enter {
        use crate::app::API_VERSION;
        use crate::test_helpers::prelude::*;

        use super::super::*;
        use super::DynSubRequest;

        #[test]
        fn test_parsing() {
            serde_json::from_str::<EnterRequest>(
                r#"
                {"id": "82f62913-c2ba-4b21-b24f-5ed499107c0a"}
            "#,
            )
            .expect("Failed to parse EnterRequest");

            serde_json::from_str::<EnterRequest>(
                r#"
                {"id": "82f62913-c2ba-4b21-b24f-5ed499107c0a", "broadcast_subscription": true}
            "#,
            )
            .expect("Failed to parse EnterRequest");

            serde_json::from_str::<EnterRequest>(
                r#"
                {"id": "82f62913-c2ba-4b21-b24f-5ed499107c0a", "broadcast_subscription": false}
            "#,
            )
            .expect("Failed to parse EnterRequest");
        }

        #[tokio::test]
        async fn enter_room() {
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

            authz.allow(agent.account_id(), vec!["rooms", &room_id], "read");

            // Make room.enter request.
            let mut context = TestContext::new(db, authz);
            let payload = EnterRequest {
                id: room.id(),
                broadcast_subscription: false,
            };

            let messages = handle_request::<EnterHandler>(&mut context, &agent, payload)
                .await
                .expect("Room entrance failed");

            // Assert dynamic subscription request.
            let (payload, reqp, topic) = find_request::<DynSubRequest>(messages.as_slice());

            let expected_topic = format!(
                "agents/{}.{}/api/{}/out/{}",
                context.config().agent_label,
                context.config().id,
                API_VERSION,
                context.config().broker_id,
            );

            assert_eq!(topic, expected_topic);
            assert_eq!(reqp.method(), "subscription.create");
            assert_eq!(payload.subject, agent.agent_id().to_owned());
            assert_eq!(payload.object, vec!["rooms", &room_id, "events"]);
        }

        #[tokio::test]
        async fn enter_room_not_authorized() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let db = TestDb::new().await;

            let room = {
                // Create room.
                let mut conn = db.get_conn().await;
                shared_helpers::insert_room(&mut conn).await
            };

            // Make room.enter request.
            let mut context = TestContext::new(db, TestAuthz::new());
            let payload = EnterRequest {
                id: room.id(),
                broadcast_subscription: false,
            };

            let err = handle_request::<EnterHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on room entering");

            assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
        }

        #[tokio::test]
        async fn enter_room_missing() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());
            let payload = EnterRequest {
                id: Uuid::new_v4(),
                broadcast_subscription: false,
            };

            let err = handle_request::<EnterHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on room entering");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_not_found");
        }

        #[tokio::test]
        async fn enter_room_closed() {
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

            authz.allow(agent.account_id(), vec!["rooms", &room_id], "read");

            // Make room.enter request.
            let mut context = TestContext::new(db, TestAuthz::new());
            let payload = EnterRequest {
                id: room.id(),
                broadcast_subscription: false,
            };

            let err = handle_request::<EnterHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on room entering");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_closed");
        }
    }

    mod leave {
        use crate::app::API_VERSION;
        use crate::test_helpers::prelude::*;

        use super::super::*;
        use super::DynSubRequest;

        #[tokio::test]
        async fn leave_room() {
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
            let mut context = TestContext::new(db, TestAuthz::new());
            let payload = LeaveRequest { id: room.id() };

            let messages = handle_request::<LeaveHandler>(&mut context, &agent, payload)
                .await
                .expect("Room leaving failed");

            // Assert dynamic subscription request.
            let (payload, reqp, topic) = find_request::<DynSubRequest>(messages.as_slice());

            let expected_topic = format!(
                "agents/{}.{}/api/{}/out/{}",
                context.config().agent_label,
                context.config().id,
                API_VERSION,
                context.config().broker_id,
            );

            assert_eq!(topic, expected_topic);
            assert_eq!(reqp.method(), "subscription.delete");
            assert_eq!(&payload.subject, agent.agent_id());

            let room_id = room.id().to_string();
            assert_eq!(payload.object, vec!["rooms", &room_id, "events"]);
        }

        #[tokio::test]
        async fn leave_room_while_not_entered() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let db = TestDb::new().await;

            let room = {
                // Create room.
                let mut conn = db.get_conn().await;
                shared_helpers::insert_room(&mut conn).await
            };

            // Make room.leave request.
            let mut context = TestContext::new(db, TestAuthz::new());
            let payload = LeaveRequest { id: room.id() };

            let err = handle_request::<LeaveHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on room leaving");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "agent_not_entered_the_room");
        }

        #[tokio::test]
        async fn leave_room_missing() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());
            let payload = LeaveRequest { id: Uuid::new_v4() };

            let err = handle_request::<LeaveHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on room leaving");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_not_found");
        }
    }

    mod adjust {
        use chrono::Utc;

        use crate::test_helpers::prelude::*;

        use super::super::*;

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
                started_at: Utc::now(),
                segments: vec![].into(),
                offset: 0,
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
                started_at: Utc::now(),
                segments: vec![].into(),
                offset: 0,
            };

            let err = handle_request::<AdjustHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on room adjustment");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_not_found");
        }
    }
}

mod dump_events;
