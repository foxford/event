use std::collections::HashMap;
use std::ops::Bound;
use std::sync::Arc;

use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use sqlx::Acquire;
use svc_agent::{
    mqtt::{OutgoingEvent, OutgoingEventProperties, ResponseStatus, ShortTermTimingProperties},
    AccountId, Addressable, AgentId,
};
use svc_error::Error as SvcError;
use svc_utils::extractors::AgentIdExtractor;
use tracing::{error, info, instrument};
use uuid::Uuid;

use crate::app::endpoint::prelude::*;
use crate::app::{
    context::{AppContext, Context},
    message_handler::Message,
};
use crate::db::adjustment::Segments;
use crate::db::agent;
use crate::db::room::{InsertQuery, UpdateQuery};
use crate::db::room_time::{BoundedDateTimeTuple, RoomTime};
use crate::{
    app::operations::{adjust_room, AdjustOutput},
    db::event::{insert_agent_action, AgentAction},
};

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    audience: String,
    #[serde(with = "crate::serde::ts_seconds_bound_tuple")]
    time: BoundedDateTimeTuple,
    tags: Option<JsonValue>,
    preserve_history: Option<bool>,
    classroom_id: Uuid,
    #[serde(default)]
    validate_whiteboard_access: Option<bool>,
}

pub async fn create(
    State(ctx): State<Arc<AppContext>>,
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

pub(crate) struct CreateHandler;

#[async_trait]
impl RequestHandler for CreateHandler {
    type Payload = CreateRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<'a, C: Context + Sync + Send>(
        context: &'a mut C,
        payload: Self::Payload,
        reqp: RequestParams<'a>,
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
            let mut query =
                InsertQuery::new(&payload.audience, payload.time.into(), payload.classroom_id);

            if let Some(tags) = payload.tags {
                query = query.tags(tags);
            }

            if let Some(preserve_history) = payload.preserve_history {
                query = query.preserve_history(preserve_history);
            }

            if let Some(flag) = payload.validate_whiteboard_access {
                query = query.validate_whiteboard_access(flag);
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

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct ReadRequest {
    id: Uuid,
}

pub async fn read(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
) -> RequestResult {
    let request = ReadRequest { id: room_id };
    ReadHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
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
    async fn handle<'a, C: Context + Sync + Send>(
        context: &'a mut C,
        payload: Self::Payload,
        reqp: RequestParams<'a>,
    ) -> RequestResult {
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

        Ok(AppResponse::new(
            ResponseStatus::OK,
            room,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub struct UpdatePayload {
    #[serde(default, with = "crate::serde::ts_seconds_option_bound_tuple")]
    time: Option<BoundedDateTimeTuple>,
    tags: Option<JsonValue>,
    classroom_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateRequest {
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

pub(crate) struct UpdateHandler;

#[async_trait]
impl RequestHandler for UpdateHandler {
    type Payload = UpdateRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<'a, C: Context + Sync + Send>(
        context: &'a mut C,
        Self::Payload { id, payload }: Self::Payload,
        reqp: RequestParams<'a>,
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

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub struct EnterPayload {
    #[serde(default)]
    agent_label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EnterRequest {
    id: Uuid,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct RoomEnterEvent {
    id: Uuid,
    agent_id: AgentId,
    banned: bool,
    agent: crate::db::agent::AgentWithBan,
}

pub(crate) struct EnterHandler;

pub async fn enter(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
    Json(payload): Json<EnterPayload>,
) -> RequestResult {
    let agent_label = payload
        .agent_label
        .as_ref()
        .context("No agent label present")
        .error(AppErrorKind::InvalidPayload)?;
    let agent_id = AgentId::new(agent_label, agent_id.as_account_id().to_owned());
    let request = EnterRequest { id: room_id };
    EnterHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

#[async_trait]
impl RequestHandler for EnterHandler {
    type Payload = EnterRequest;

    #[instrument(
        skip_all,
        fields(
            room_id = %payload.id, scope, classroom_id
        )
    )]
    async fn handle<'a, C: Context + Sync + Send>(
        context: &'a mut C,
        payload: Self::Payload,
        reqp: RequestParams<'a>,
    ) -> RequestResult {
        let room =
            helpers::find_room(context, payload.id, helpers::RoomTimeRequirement::Open).await?;

        // Authorize subscribing to the room's events.
        let object: Box<dyn svc_authz::IntentObject> =
            AuthzObject::new(&["classrooms", &room.classroom_id().to_string()]).into();

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

        let req1 = context
            .broker_client()
            .enter_room(room.id(), reqp.as_agent_id());
        let req2 = context
            .broker_client()
            .enter_broadcast_room(room.id(), reqp.as_agent_id());

        tokio::try_join!(req1, req2)
            .context("Broker request failed")
            .error(AppErrorKind::BrokerRequestFailed)?;

        // Determine whether the agent is banned.
        let agent_with_ban = {
            // Find room.
            helpers::find_room(context, room.id(), helpers::RoomTimeRequirement::Open).await?;

            // Update agent state to `ready`.
            let q = agent::UpdateQuery::new(reqp.as_agent_id().clone(), room.id())
                .status(agent::Status::Ready);

            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::AgentUpdateQuery, q.execute(&mut conn))
                .await
                .context("Failed to put agent into 'ready' status")
                .error(AppErrorKind::DbQueryFailed)?;

            let query = agent::FindWithBanQuery::new(reqp.as_agent_id().clone(), room.id());

            context
                .metrics()
                .measure_query(QueryKey::AgentFindWithBanQuery, query.execute(&mut conn))
                .await
                .context("Failed to find agent with ban")
                .error(AppErrorKind::DbQueryFailed)?
                .ok_or_else(|| anyhow!("No agent {} in room {}", reqp.as_agent_id(), room.id()))
                .error(AppErrorKind::AgentNotEnteredTheRoom)?
        };

        let banned = agent_with_ban.banned().unwrap_or(false);

        // Send a response to the original `room.enter` request and a room-wide notification.
        let mut response = AppResponse::new(
            ResponseStatus::OK,
            json!({}),
            context.start_timestamp(),
            Some(authz_time),
        );

        response.add_notification(
            "room.enter",
            &format!("rooms/{}/events", room.id()),
            RoomEnterEvent {
                id: room.id(),
                agent_id: reqp.as_agent_id().to_owned(),
                agent: agent_with_ban,
                banned,
            },
            context.start_timestamp(),
        );

        Ok(response)
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub struct LockedTypesPayload {
    locked_types: HashMap<String, bool>,
}

#[derive(Debug, Deserialize)]
pub struct LockedTypesRequest {
    id: Uuid,
    #[serde(flatten)]
    payload: LockedTypesPayload,
}

pub async fn locked_types(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
    Json(payload): Json<LockedTypesPayload>,
) -> RequestResult {
    let request = LockedTypesRequest {
        id: room_id,
        payload,
    };
    LockedTypesHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub(crate) struct LockedTypesHandler;

#[async_trait]
impl RequestHandler for LockedTypesHandler {
    type Payload = LockedTypesRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<'a, C: Context + Sync + Send>(
        context: &'a mut C,
        Self::Payload { id, payload }: Self::Payload,
        reqp: RequestParams<'a>,
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

        let room = {
            let locked_types = room
                .locked_types()
                .iter()
                .map(|(k, v)| (k.to_owned(), *v))
                .chain(payload.locked_types)
                .collect::<HashMap<_, _>>();

            let mut conn = context.get_conn().await?;

            let mut txn = conn
                .begin()
                .await
                .context("Failed to acquire transaction")
                .error(AppErrorKind::DbQueryFailed)?;

            let query = UpdateQuery::new(room.id()).locked_types(locked_types);

            let room = context
                .metrics()
                .measure_query(QueryKey::RoomUpdateQuery, query.execute(&mut txn))
                .await
                .context("Failed to update room")
                .error(AppErrorKind::DbQueryFailed)?;

            txn.commit()
                .await
                .context("Failed to commit transaction")
                .error(AppErrorKind::DbQueryFailed)?;

            room
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
            &format!("rooms/{}/events", room.id()),
            room,
            context.start_timestamp(),
        );

        Ok(response)
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub struct WhiteboardAccessPayload {
    whiteboard_access: HashMap<AccountId, bool>,
}

#[derive(Debug, Deserialize)]
pub struct WhiteboardAccessRequest {
    id: Uuid,
    #[serde(flatten)]
    payload: WhiteboardAccessPayload,
}

pub async fn whiteboard_access(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
    Json(payload): Json<WhiteboardAccessPayload>,
) -> RequestResult {
    let request = WhiteboardAccessRequest {
        id: room_id,
        payload,
    };
    WhiteboardAccessHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub(crate) struct WhiteboardAccessHandler;

#[async_trait]
impl RequestHandler for WhiteboardAccessHandler {
    type Payload = WhiteboardAccessRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<'a, C: Context + Sync + Send>(
        context: &'a mut C,
        Self::Payload { id, payload }: Self::Payload,
        reqp: RequestParams<'a>,
    ) -> RequestResult {
        // Find realtime room.
        let room = helpers::find_room(context, id, helpers::RoomTimeRequirement::Any).await?;

        if !room.validate_whiteboard_access() {
            Err(anyhow!(
                "Useless whiteboard access change for room that doesnt check it"
            ))
            .error(AppErrorKind::WhiteboardAccessUpdateNotChecked)?
        }

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

        let room = {
            let whiteboard_access = room
                .whiteboard_access()
                .iter()
                .map(|(k, v)| (k.to_owned(), *v))
                .chain(payload.whiteboard_access)
                .collect();
            let mut conn = context.get_conn().await?;
            let mut txn = conn
                .begin()
                .await
                .context("Failed to acquire transaction")
                .error(AppErrorKind::DbQueryFailed)?;

            let query = UpdateQuery::new(room.id()).whiteboard_access(whiteboard_access);

            let room = context
                .metrics()
                .measure_query(QueryKey::RoomUpdateQuery, query.execute(&mut txn))
                .await
                .context("Failed to update room")
                .error(AppErrorKind::DbQueryFailed)?;

            txn.commit()
                .await
                .context("Failed to commit transaction")
                .error(AppErrorKind::DbQueryFailed)?;

            room
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
            &format!("rooms/{}/events", room.id()),
            room,
            context.start_timestamp(),
        );

        Ok(response)
    }
}

///////////////////////////////////////////////////////////////////////////////

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

pub(crate) struct AdjustHandler;

#[async_trait]
impl RequestHandler for AdjustHandler {
    type Payload = AdjustRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<'a, C: Context + Sync + Send>(
        context: &'a mut C,
        Self::Payload { id, payload }: Self::Payload,
        reqp: RequestParams<'a>,
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

///////////////////////////////////////////////////////////////////////////////

pub(crate) use dump_events::EventsDumpHandler;

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
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
                validate_whiteboard_access: None,
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
                validate_whiteboard_access: None,
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
                validate_whiteboard_access: None,
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
                validate_whiteboard_access: None,
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
                validate_whiteboard_access: None,
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
            authz.allow(
                agent.account_id(),
                vec!["classrooms", &room.classroom_id().to_string()],
                "read",
            );

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
                factory::Room::new(uuid::Uuid::new_v4())
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
                factory::Room::new(Uuid::new_v4())
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
                factory::Room::new(Uuid::new_v4())
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
                factory::Room::new(Uuid::new_v4())
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

    mod enter {
        use crate::app::broker_client::CreateDeleteResponse;

        use crate::test_helpers::prelude::*;

        use super::super::*;

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
            let classroom_id = room.classroom_id().to_string();
            authz.allow(
                agent.account_id(),
                vec!["classrooms", &classroom_id],
                "read",
            );

            // Make room.enter request.
            let mut context = TestContext::new(db, authz);

            context
                .broker_client_mock()
                .expect_enter_room()
                .with(mockall::predicate::always(), mockall::predicate::always())
                .returning(move |_, _agent_id| Ok(CreateDeleteResponse::Ok));

            context
                .broker_client_mock()
                .expect_enter_broadcast_room()
                .with(mockall::predicate::always(), mockall::predicate::always())
                .returning(move |_, _agent_id| Ok(CreateDeleteResponse::Ok));

            let payload = EnterRequest { id: room.id() };

            let messages = handle_request::<EnterHandler>(&mut context, &agent, payload)
                .await
                .expect("Room entrance failed");

            assert_eq!(messages.len(), 2);

            let (payload, _evp, _) = find_event_by_predicate::<JsonValue, _>(&messages, |evp| {
                evp.label() == "room.enter"
            })
            .unwrap();
            assert_eq!(payload["id"], room.id().to_string());
            assert_eq!(payload["agent_id"], agent.agent_id().to_string());

            // assert response exists
            find_response::<JsonValue>(&messages);
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
            let payload = EnterRequest { id: room.id() };

            let err = handle_request::<EnterHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on room entering");

            assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
        }

        #[tokio::test]
        async fn enter_room_missing() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());
            let payload = EnterRequest { id: Uuid::new_v4() };

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
            let classroom_id = room.classroom_id().to_string();
            authz.allow(
                agent.account_id(),
                vec!["classrooms", &classroom_id],
                "read",
            );

            // Make room.enter request.
            let mut context = TestContext::new(db, TestAuthz::new());
            let payload = EnterRequest { id: room.id() };

            let err = handle_request::<EnterHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on room entering");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_closed");
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

    mod locked_types {
        use crate::db::room::Object as Room;
        use crate::test_helpers::prelude::*;

        use super::super::*;

        #[tokio::test]
        async fn lock_types_in_room() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
                room
            };

            // Allow agent to update rooms.
            let mut authz = TestAuthz::new();
            authz.allow(
                agent.account_id(),
                vec!["classrooms", &room.classroom_id().to_string()],
                "update",
            );

            // Make room.create request.
            let mut context = TestContext::new(db, authz);

            let payload = LockedTypesRequest {
                id: room.id(),
                payload: LockedTypesPayload {
                    locked_types: [("message".into(), true)].iter().cloned().collect(),
                },
            };

            let messages = handle_request::<LockedTypesHandler>(&mut context, &agent, payload)
                .await
                .expect("Room types lock failed");

            let og_room = room;
            // Assert response.
            let (room, respp, _) = find_response::<Room>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(og_room.id(), room.id());
            assert_eq!(room.locked_types().len(), 1);
            assert_eq!(room.locked_types().get("message"), Some(&true));

            // Assert notification.
            let (room, evp, topic) = find_event::<Room>(messages.as_slice());
            assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
            assert_eq!(evp.label(), "room.update");
            assert_eq!(og_room.id(), room.id());
            assert_eq!(room.locked_types().len(), 1);
            assert_eq!(room.locked_types().get("message"), Some(&true));
        }

        #[tokio::test]
        async fn lock_multiple_types_in_room() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
                room
            };

            // Allow agent to update rooms.
            let mut authz = TestAuthz::new();
            authz.allow(
                agent.account_id(),
                vec!["classrooms", &room.classroom_id().to_string()],
                "update",
            );

            // Make room.create request.
            let mut context = TestContext::new(db, authz);

            let payload = LockedTypesRequest {
                id: room.id(),
                payload: LockedTypesPayload {
                    locked_types: [("message".into(), true)].iter().cloned().collect(),
                },
            };

            handle_request::<LockedTypesHandler>(&mut context, &agent, payload)
                .await
                .expect("Room types lock failed");

            let payload = LockedTypesRequest {
                id: room.id(),
                payload: LockedTypesPayload {
                    locked_types: [("document".into(), true)].iter().cloned().collect(),
                },
            };

            handle_request::<LockedTypesHandler>(&mut context, &agent, payload)
                .await
                .expect("Room types lock failed");

            let payload = LockedTypesRequest {
                id: room.id(),
                payload: LockedTypesPayload {
                    locked_types: [("message".into(), false)].iter().cloned().collect(),
                },
            };

            let messages = handle_request::<LockedTypesHandler>(&mut context, &agent, payload)
                .await
                .expect("Room types lock failed");

            let og_room = room;
            let (room, respp, _) = find_response::<Room>(messages.as_slice());

            // Assert response.
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(og_room.id(), room.id());
            assert_eq!(room.locked_types().len(), 1);
            assert_eq!(room.locked_types().len(), 1);
            assert_eq!(room.locked_types().get("message"), None);
            assert_eq!(room.locked_types().get("document"), Some(&true));

            // Assert notification.
            let (room, evp, topic) = find_event::<Room>(messages.as_slice());
            assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
            assert_eq!(evp.label(), "room.update");
            assert_eq!(og_room.id(), room.id());
            assert_eq!(room.locked_types().len(), 1);
            assert_eq!(room.locked_types().get("message"), None);
            assert_eq!(room.locked_types().get("document"), Some(&true));
        }

        #[tokio::test]
        async fn lock_types_in_room_not_authorized() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
                room
            };

            // Make room.create request.
            let mut context = TestContext::new(db, TestAuthz::new());

            let payload = LockedTypesRequest {
                id: room.id(),
                payload: LockedTypesPayload {
                    locked_types: [("message".into(), true)].iter().cloned().collect(),
                },
            };

            let err = handle_request::<LockedTypesHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on lock types");

            assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
        }
    }

    mod whiteboard_access {
        use super::super::*;
        use crate::db::room::Object as Room;
        use crate::test_helpers::prelude::*;

        #[tokio::test]
        async fn whiteboard_access() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room =
                    shared_helpers::insert_validating_whiteboard_access_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
                room
            };

            // Allow agent to update rooms.
            let mut authz = TestAuthz::new();
            authz.allow(
                agent.account_id(),
                vec!["classrooms", &room.classroom_id().to_string()],
                "update",
            );

            // Make request.
            let mut context = TestContext::new(db, authz);

            let payload = WhiteboardAccessRequest {
                id: room.id(),
                payload: WhiteboardAccessPayload {
                    whiteboard_access: [(agent.account_id().to_owned(), true)]
                        .iter()
                        .cloned()
                        .collect(),
                },
            };

            let messages = handle_request::<WhiteboardAccessHandler>(&mut context, &agent, payload)
                .await
                .expect("Room whiteboard access update failed");

            let og_room = room;
            // Assert response.
            let (room, respp, _) = find_response::<Room>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(og_room.id(), room.id());
            assert_eq!(room.whiteboard_access().len(), 1);
            assert_eq!(
                room.whiteboard_access().get(agent.account_id()),
                Some(&true)
            );

            // Assert notification.
            let (room, evp, topic) = find_event::<Room>(messages.as_slice());
            assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
            assert_eq!(evp.label(), "room.update");
            assert_eq!(og_room.id(), room.id());
            assert_eq!(room.whiteboard_access().len(), 1);
            assert_eq!(
                room.whiteboard_access().get(agent.account_id()),
                Some(&true)
            );
        }

        #[tokio::test]
        async fn whiteboard_access_for_multiple_accounts_in_room() {
            let db = TestDb::new().await;
            let teacher = TestAgent::new("web", "teacher", USR_AUDIENCE);
            let agent1 = TestAgent::new("web", "user123", USR_AUDIENCE);
            let agent2 = TestAgent::new("web", "user456", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room =
                    shared_helpers::insert_validating_whiteboard_access_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, teacher.agent_id(), room.id()).await;
                room
            };

            // Allow agent to update rooms.
            let mut authz = TestAuthz::new();
            authz.allow(
                teacher.account_id(),
                vec!["classrooms", &room.classroom_id().to_string()],
                "update",
            );

            // Make room.create request.
            let mut context = TestContext::new(db, authz);

            let payload = WhiteboardAccessRequest {
                id: room.id(),
                payload: WhiteboardAccessPayload {
                    whiteboard_access: [(agent1.account_id().to_owned(), true)]
                        .iter()
                        .cloned()
                        .collect(),
                },
            };

            handle_request::<WhiteboardAccessHandler>(&mut context, &teacher, payload)
                .await
                .expect("Room whiteboard access update failed");

            let payload = WhiteboardAccessRequest {
                id: room.id(),
                payload: WhiteboardAccessPayload {
                    whiteboard_access: [(agent2.account_id().to_owned(), true)]
                        .iter()
                        .cloned()
                        .collect(),
                },
            };

            handle_request::<WhiteboardAccessHandler>(&mut context, &teacher, payload)
                .await
                .expect("Room whiteboard access update failed");

            let payload = WhiteboardAccessRequest {
                id: room.id(),
                payload: WhiteboardAccessPayload {
                    whiteboard_access: [(agent1.account_id().to_owned(), false)]
                        .iter()
                        .cloned()
                        .collect(),
                },
            };

            let messages =
                handle_request::<WhiteboardAccessHandler>(&mut context, &teacher, payload)
                    .await
                    .expect("Room whiteboard access update failed");

            let og_room = room;
            let (room, respp, _) = find_response::<Room>(messages.as_slice());

            // Assert response.
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(og_room.id(), room.id());
            assert_eq!(room.whiteboard_access().len(), 1);
            assert_eq!(room.whiteboard_access().len(), 1);
            assert_eq!(room.whiteboard_access().get(agent1.account_id()), None);
            assert_eq!(
                room.whiteboard_access().get(agent2.account_id()),
                Some(&true)
            );

            // Assert notification.
            let (room, evp, topic) = find_event::<Room>(messages.as_slice());
            assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
            assert_eq!(evp.label(), "room.update");
            assert_eq!(og_room.id(), room.id());
            assert_eq!(room.whiteboard_access().len(), 1);
            assert_eq!(room.whiteboard_access().get(agent1.account_id()), None);
            assert_eq!(
                room.whiteboard_access().get(agent2.account_id()),
                Some(&true)
            );
        }

        #[tokio::test]
        async fn whiteboard_access_in_room_not_authorized() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room =
                    shared_helpers::insert_validating_whiteboard_access_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
                room
            };

            // Make room.create request.
            let mut context = TestContext::new(db, TestAuthz::new());

            let payload = WhiteboardAccessRequest {
                id: room.id(),
                payload: WhiteboardAccessPayload {
                    whiteboard_access: [(agent.account_id().to_owned(), true)]
                        .iter()
                        .cloned()
                        .collect(),
                },
            };

            let err = handle_request::<WhiteboardAccessHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on whiteboard access update");

            assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
        }
    }
}

pub use dump_events::dump_events;
mod dump_events;
