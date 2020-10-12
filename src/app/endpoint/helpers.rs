use std::ops::Bound;

use anyhow::Context as AnyhowContext;
use chrono::{DateTime, Duration, Utc};
use serde::ser::Serialize;
use svc_agent::mqtt::{
    IncomingRequestProperties, IntoPublishableMessage, OutgoingEvent, OutgoingEventProperties,
    OutgoingResponse, ResponseStatus, ShortTermTimingProperties,
};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::error::{Error as AppError, ErrorExt, ErrorKind as AppErrorKind};
use crate::app::metrics::ProfilerKeys;
use crate::app::API_VERSION;
use crate::db;

////////////////////////////////////////////////////////////////////////////////

pub(crate) fn build_response(
    status: ResponseStatus,
    payload: impl Serialize + Send + 'static,
    reqp: &IncomingRequestProperties,
    start_timestamp: DateTime<Utc>,
    maybe_authz_time: Option<Duration>,
) -> Box<dyn IntoPublishableMessage + Send> {
    let mut timing = ShortTermTimingProperties::until_now(start_timestamp);

    if let Some(authz_time) = maybe_authz_time {
        timing.set_authorization_time(authz_time);
    }

    let props = reqp.to_response(status, timing);
    Box::new(OutgoingResponse::unicast(payload, props, reqp, API_VERSION))
}

pub(crate) fn build_notification(
    label: &'static str,
    path: &str,
    payload: impl Serialize + Send + 'static,
    reqp: &IncomingRequestProperties,
    start_timestamp: DateTime<Utc>,
) -> Box<dyn IntoPublishableMessage + Send> {
    let timing = ShortTermTimingProperties::until_now(start_timestamp);
    let mut props = OutgoingEventProperties::new(label, timing);
    props.set_tracking(reqp.tracking().to_owned());
    Box::new(OutgoingEvent::broadcast(payload, props, path))
}

////////////////////////////////////////////////////////////////////////////////

pub(crate) enum RoomTimeRequirement {
    Any,
    NotClosed,
    Open,
}

pub(crate) async fn find_room<C: Context>(
    context: &mut C,
    id: Uuid,
    opening_requirement: RoomTimeRequirement,
    key: &str,
) -> Result<db::room::Object, AppError> {
    context.add_logger_tags(o!("room_id" => id.to_string()));

    let query = db::room::FindQuery::new(id);
    let mut conn = context.get_ro_conn().await?;

    let room = context
        .profiler()
        .measure(
            (ProfilerKeys::RoomFindQuery, Some(key.to_owned())),
            query.execute(&mut conn),
        )
        .await
        .context("Failed to find room")
        .error(AppErrorKind::DbQueryFailed)?
        .ok_or_else(|| anyhow!("Room not found"))
        .error(AppErrorKind::RoomNotFound)?;

    add_room_logger_tags(context, &room);

    match opening_requirement {
        // Room time doesn't matter.
        RoomTimeRequirement::Any => Ok(room),
        // Current time must be before room closing, including not yet opened rooms.
        RoomTimeRequirement::NotClosed => {
            let closed_at = match room.time() {
                (_, Bound::Excluded(c)) => Ok(c),
                _ => Err(anyhow!("Invalid room time")).error(AppErrorKind::InvalidRoomTime),
            }?;

            if Utc::now() < closed_at {
                Ok(room)
            } else {
                Err(anyhow!("Room already closed")).error(AppErrorKind::RoomClosed)
            }
        }
        // Current time must be exactly in the room's time range.
        RoomTimeRequirement::Open => {
            let (opened_at, closed_at) = match room.time() {
                (Bound::Included(o), Bound::Excluded(c)) => Ok((o, c)),
                _ => Err(anyhow!("Invalid room time")).error(AppErrorKind::InvalidRoomTime),
            }?;

            let now = Utc::now();

            if now >= opened_at && now < closed_at {
                Ok(room)
            } else {
                Err(anyhow!("Room already closed or not yet opened"))
                    .error(AppErrorKind::RoomClosed)
            }
        }
    }
}

pub(crate) fn add_room_logger_tags<C: Context>(context: &mut C, room: &db::room::Object) {
    context.add_logger_tags(o!("room_id" => room.id().to_string()));

    if let Some(tags) = room.tags() {
        if let Some(scope) = tags.get("scope") {
            context.add_logger_tags(o!("scope" => scope.to_string()));
        }
    }
}
