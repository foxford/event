use async_trait::async_trait;
use chrono::{serde::ts_milliseconds_option, DateTime, Utc};
use serde_derive::Deserialize;
use serde_json::{map::Map as JsonMap, Value as JsonValue};
use svc_agent::mqtt::{IncomingRequestProperties, IntoPublishableDump, ResponseStatus};
use svc_error::Error as SvcError;
use uuid::Uuid;

use crate::app::endpoint::{helpers, RequestHandler};
use crate::app::Context;
use crate::db;

///////////////////////////////////////////////////////////////////////////////

const MAX_SETS: usize = 10;
const MAX_LIMIT_PER_SET: i64 = 100;

#[derive(Debug, Deserialize)]
pub(crate) struct ReadRequest {
    room_id: Uuid,
    sets: Vec<String>,
    occured_at: i64,
    #[serde(default, with = "ts_milliseconds_option")]
    last_created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    direction: db::event::Direction,
    limit: Option<i64>,
}

pub(crate) struct ReadHandler;

#[async_trait]
impl RequestHandler for ReadHandler {
    type Payload = ReadRequest;
    const ERROR_TITLE: &'static str = "Failed to read state";

    async fn handle(
        context: &Context,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError> {
        // Validate parameters.
        let validation_error = match payload.sets.len() {
            0 => Some("'sets' can't be empty"),
            len if len > MAX_SETS => Some("too many 'sets'"),
            _ => None,
        };

        if let Some(err) = validation_error {
            return Err(svc_error!(ResponseStatus::BAD_REQUEST, "{}", err));
        }

        // Choose limit.
        let limit = std::cmp::min(
            payload.limit.unwrap_or_else(|| MAX_LIMIT_PER_SET),
            MAX_LIMIT_PER_SET,
        );

        // Check whether the room exists.
        let conn = context.db().get()?;

        let room = db::room::FindQuery::new(payload.room_id)
            .execute(&conn)?
            .ok_or_else(|| {
                svc_error!(
                    ResponseStatus::NOT_FOUND,
                    "the room = '{}' is not found",
                    payload.room_id
                )
            })?;

        // Authorize room events listing.
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id, "events"];

        let authz_time = context
            .authz()
            .authorize(room.audience(), reqp, object, "list")
            .await?;

        // Retrieve state for each set from the DB and put them into a map.
        let mut state = JsonMap::new();

        for set in payload.sets.iter() {
            // Build a query for the particular set state.
            let mut query = db::event::SetStateQuery::new(room.id(), &set, payload.occured_at)
                .direction(payload.direction);

            if let Some(last_created_at) = payload.last_created_at {
                query = query.last_created_at(last_created_at);
            }

            // If it is the only set specified at first execute a total count query and
            // add `has_next` pagination flag to the state.
            if payload.sets.len() == 1 {
                let total_count = query.total_count(&conn).map_err(|err| {
                    svc_error!(
                        ResponseStatus::UNPROCESSABLE_ENTITY,
                        "failed to query state total count for set = '{}': {}",
                        set,
                        err
                    )
                })?;

                let has_next = total_count > limit;
                state.insert(String::from("has_next"), JsonValue::Bool(has_next));
            }

            // Limit the query and retrieve the state.
            let set_state = query.limit(limit).execute(&conn).map_err(|err| {
                svc_error!(
                    ResponseStatus::UNPROCESSABLE_ENTITY,
                    "failed to query state for set = '{}': {}",
                    set,
                    err
                )
            })?;

            // Serialize to JSON and add to the state map.
            let serialized_set_state = serde_json::to_value(set_state).map_err(|err| {
                svc_error!(
                    ResponseStatus::UNPROCESSABLE_ENTITY,
                    "failed to serialize state for set = '{}': {}",
                    set,
                    err
                )
            })?;

            state.insert(set.to_owned(), serialized_set_state);
        }

        // Respond with state.
        Ok(vec![helpers::build_response(
            ResponseStatus::OK,
            JsonValue::Object(state),
            reqp,
            start_timestamp,
            Some(authz_time),
        )])
    }
}
