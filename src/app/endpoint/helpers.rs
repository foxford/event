use anyhow::Context as AnyhowContext;
use chrono::{DateTime, Duration, Utc};
use serde::ser::Serialize;
use svc_agent::mqtt::{
    IncomingRequestProperties, IntoPublishableMessage,
    OutgoingResponse, ResponseStatus, ShortTermTimingProperties,
};
use tracing::field::display;
use uuid::Uuid;

use crate::app::error::{Error as AppError, ErrorExt, ErrorKind as AppErrorKind};
use crate::app::API_VERSION;
use crate::db;
use crate::{app::context::Context, metrics::QueryKey};

////////////////////////////////////////////////////////////////////////////////

pub(crate) fn build_response(
    status: ResponseStatus,
    payload: impl Serialize + Send + Sync + 'static,
    reqp: &IncomingRequestProperties,
    start_timestamp: DateTime<Utc>,
    maybe_authz_time: Option<Duration>,
) -> Box<dyn IntoPublishableMessage + Send + Sync> {
    let mut timing = ShortTermTimingProperties::until_now(start_timestamp);

    if let Some(authz_time) = maybe_authz_time {
        timing.set_authorization_time(authz_time);
    }

    let props = reqp.to_response(status, timing);
    Box::new(OutgoingResponse::unicast(payload, props, reqp, API_VERSION))
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
) -> Result<db::room::Object, AppError> {
    tracing::Span::current().record("room_id", &display(id));

    let query = db::room::FindQuery::new(id);
    let mut conn = context.get_ro_conn().await?;

    let room = context
        .metrics()
        .measure_query(QueryKey::RoomFindQuery, query.execute(&mut conn))
        .await
        .context("Failed to find room")
        .error(AppErrorKind::DbQueryFailed)?
        .ok_or_else(|| anyhow!("Room not found"))
        .error(AppErrorKind::RoomNotFound)?;

    add_room_logger_tags(&room);

    match opening_requirement {
        // Room time doesn't matter.
        RoomTimeRequirement::Any => Ok(room),
        // Current time must be before room closing, including not yet opened rooms.
        RoomTimeRequirement::NotClosed => {
            if room.is_closed() {
                Err(anyhow!("Room already closed")).error(AppErrorKind::RoomClosed)
            } else {
                Ok(room)
            }
        }
        // Current time must be exactly in the room's time range.
        RoomTimeRequirement::Open => {
            if room.is_open() {
                Ok(room)
            } else {
                Err(anyhow!("Room already closed or not yet opened"))
                    .error(AppErrorKind::RoomClosed)
            }
        }
    }
}

pub(crate) fn add_room_logger_tags(room: &db::room::Object) {
    let span = tracing::Span::current();
    span.record("room_id", &display(room.id()));

    if let Some(tags) = room.tags() {
        if let Some(scope) = tags.get("scope") {
            span.record("scope", &display(scope));
        }
    }

    if let Some(classroom_id) = room.classroom_id() {
        span.record("classroom_id", &display(classroom_id));
    }
}
