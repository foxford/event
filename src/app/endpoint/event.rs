use std::ops::Bound;

use async_trait::async_trait;
use chrono::{serde::ts_milliseconds_option, DateTime, Utc};
use serde_derive::Deserialize;
use serde_json::Value as JsonValue;
use svc_agent::{
    mqtt::{IncomingRequestProperties, IntoPublishableDump, ResponseStatus},
    Addressable,
};
use svc_error::Error as SvcError;
use uuid::Uuid;

use crate::app::endpoint::{helpers, RequestHandler};
use crate::app::Context;
use crate::db;

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct CreateRequest {
    room_id: Uuid,
    #[serde(rename = "type")]
    kind: String,
    set: Option<String>,
    label: Option<String>,
    data: JsonValue,
}

pub(crate) struct CreateHandler;

#[async_trait]
impl RequestHandler for CreateHandler {
    type Payload = CreateRequest;
    const ERROR_TITLE: &'static str = "Failed to create event";

    async fn handle(
        context: &Context,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError> {
        let conn = context.db().get()?;

        // Check whether the room exists and open.
        let room = db::room::FindQuery::new(payload.room_id)
            .time(db::room::now())
            .execute(&conn)?
            .ok_or_else(|| {
                svc_error!(
                    ResponseStatus::NOT_FOUND,
                    "the room = '{}' is not found or closed",
                    payload.room_id
                )
            })?;

        // Check whether the agent has entered the room.
        let agents = db::agent::ListQuery::new()
            .agent_id(reqp.as_agent_id())
            .room_id(room.id())
            .status(db::agent::Status::Ready)
            .execute(&conn)?;

        if agents.len() != 1 {
            return Err(svc_error!(
                ResponseStatus::FORBIDDEN,
                "agent = '{}' has not entered the room = '{}'",
                reqp.as_agent_id(),
                room.id()
            ));
        }

        // Insert event into the DB.
        let occured_at = match room.time() {
            (Bound::Included(opened_at), _) => {
                (Utc::now() - opened_at.to_owned()).num_milliseconds()
            }
            _ => {
                return Err(svc_error!(
                    ResponseStatus::UNPROCESSABLE_ENTITY,
                    "invalid time for room = '{}'",
                    room.id()
                ))
            }
        };

        let mut query = db::event::InsertQuery::new(
            room.id(),
            &payload.kind,
            payload.data,
            occured_at,
            reqp.as_agent_id(),
        );

        if let Some(ref set) = payload.set {
            query = query.set(set);
        }

        if let Some(ref label) = payload.label {
            query = query.label(label);
        }

        let event = query.execute(&conn).map_err(|err| {
            svc_error!(
                ResponseStatus::UNPROCESSABLE_ENTITY,
                "failed to create event: {}",
                err
            )
        })?;

        // Respond to the user and notify room subscribers.
        let response = helpers::build_response(
            ResponseStatus::CREATED,
            event.clone(),
            reqp,
            start_timestamp,
            None,
        );

        let notification = helpers::build_notification(
            "event.create",
            &format!("rooms/{}/events", room.id()),
            event,
            reqp,
            start_timestamp,
        );

        Ok(vec![response, notification])
    }
}

///////////////////////////////////////////////////////////////////////////////

const MAX_LIMIT: i64 = 100;

#[derive(Debug, Deserialize)]
pub(crate) struct ListRequest {
    room_id: Uuid,
    #[serde(rename = "type")]
    kind: Option<String>,
    set: Option<String>,
    label: Option<String>,
    #[serde(default, with = "ts_milliseconds_option")]
    last_created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    direction: db::event::Direction,
    limit: Option<i64>,
}

pub(crate) struct ListHandler;

#[async_trait]
impl RequestHandler for ListHandler {
    type Payload = ListRequest;
    const ERROR_TITLE: &'static str = "Failed to list events";

    async fn handle(
        context: &Context,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<Vec<Box<dyn IntoPublishableDump>>, SvcError> {
        let conn = context.db().get()?;

        // Check whether the room exists.
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

        // Retrieve events from the DB.
        let mut query = db::event::ListQuery::new().room_id(room.id());

        if let Some(ref kind) = payload.kind {
            query = query.kind(kind);
        }

        if let Some(ref set) = payload.set {
            query = query.set(set);
        }

        if let Some(ref label) = payload.label {
            query = query.label(label);
        }

        if let Some(last_created_at) = payload.last_created_at {
            query = query.last_created_at(last_created_at);
        }

        let events = query
            .direction(payload.direction)
            .limit(std::cmp::min(
                payload.limit.unwrap_or_else(|| MAX_LIMIT),
                MAX_LIMIT,
            ))
            .execute(&conn)?;

        // Respond with events list.
        Ok(vec![helpers::build_response(
            ResponseStatus::OK,
            events,
            reqp,
            start_timestamp,
            Some(authz_time),
        )])
    }
}
