use async_trait::async_trait;
use chrono::{DateTime, Utc};
use failure::Error;
use serde_derive::Deserialize;
use serde_json::Value as JsonValue;
use svc_agent::mqtt::{IncomingRequestProperties, IntoPublishableDump, ResponseStatus};
use uuid::Uuid;

use crate::app::{
    endpoint::{helpers, RequestHandler},
    Context,
};
use crate::db::room::{FindQuery, InsertQuery, Time};

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

        let room = {
            let mut query = InsertQuery::new(&payload.audience, backend_room.id, payload.time);

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
        let room = {
            let conn = context.db().get()?;

            match FindQuery::new(payload.id).execute(&conn)? {
                Some(room) => room,
                None => {
                    return Ok(vec![helpers::build_error_response(
                        ResponseStatus::NOT_FOUND,
                        ReadHandler::ERROR_TITLE,
                        &format!("Room not found, id = '{}'", &payload.id),
                        reqp,
                        start_timestamp,
                    )])
                }
            }
        };

        Ok(vec![helpers::build_response(
            ResponseStatus::OK,
            room,
            reqp,
            start_timestamp,
        )])
    }
}
