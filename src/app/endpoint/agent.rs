use async_trait::async_trait;
use chrono::{DateTime, Utc};
use failure::{format_err, Error};
use serde_derive::Deserialize;
use svc_agent::mqtt::{IncomingRequestProperties, IntoPublishableDump, ResponseStatus};
use uuid::Uuid;

use crate::app::endpoint::{helpers, RequestHandler};
use crate::app::Context;
use crate::db;

///////////////////////////////////////////////////////////////////////////////

const MAX_LIMIT: i64 = 25;

#[derive(Debug, Deserialize)]
pub(crate) struct ListRequest {
    room_id: Uuid,
    offset: Option<i64>,
    limit: Option<i64>,
}

pub(crate) struct ListHandler;

#[async_trait]
impl RequestHandler for ListHandler {
    type Payload = ListRequest;
    const ERROR_TITLE: &'static str = "Failed to list agents";

    async fn handle(
        context: &Context,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, Error> {
        let conn = context.db().get()?;

        // Check whether the room exists and open.
        let room = db::room::FindQuery::new(payload.room_id)
            .time(db::room::now())
            .execute(&conn)?
            .ok_or_else(|| format_err!("the room = '{}' is not found", payload.room_id))?;

        // Authorize agents listing in the room.
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id, "agents"];

        let authz_time = context
            .authz()
            .authorize(room.audience(), reqp, object, "list")
            .await?;

        // Get agents list in the room.
        let agents = db::agent::ListQuery::new()
            .room_id(payload.room_id)
            .status(db::agent::Status::Ready)
            .offset(payload.offset.unwrap_or_else(|| 0))
            .limit(std::cmp::min(
                payload.limit.unwrap_or_else(|| MAX_LIMIT),
                MAX_LIMIT,
            ))
            .execute(&conn)?;

        // Respond with agents list.
        Ok(vec![helpers::build_response(
            ResponseStatus::OK,
            agents,
            reqp,
            start_timestamp,
            Some(authz_time),
        )])
    }
}
