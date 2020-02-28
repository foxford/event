use std::ops::Bound;

use async_trait::async_trait;
use chrono::{serde::ts_milliseconds_option, DateTime, Duration, Utc};
use serde_derive::Deserialize;
use serde_json::Value as JsonValue;
use svc_agent::Authenticable;
use svc_agent::{
    mqtt::{IncomingRequestProperties, IntoPublishableDump, ResponseStatus},
    Addressable,
};
use svc_error::Error as SvcError;
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::{helpers, RequestHandler};
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

    async fn handle<C: Context>(
        context: &C,
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

        // Authorize event creation on tenant with cache.
        let room_id = room.id().to_string();
        let author = reqp.as_account_id().to_string();
        let object = vec![
            "rooms",
            &room_id,
            "events",
            &payload.kind,
            "authors",
            &author,
        ];

        let cached_authz = context
            .authz_cache()
            .get(reqp.as_account_id(), &object, "create")
            .map_err(|err| svc_error!(ResponseStatus::INTERNAL_SERVER_ERROR, "{}", err))?;

        let authz_time = match cached_authz {
            Some(true) => Duration::milliseconds(0),
            Some(false) => return Err(svc_error!(ResponseStatus::FORBIDDEN, "Not authorized")),
            None => {
                let result = context
                    .authz()
                    .authorize(room.audience(), reqp, object.clone(), "create")
                    .await;

                context
                    .authz_cache()
                    .set(reqp.as_account_id(), &object, "create", result.is_ok())
                    .map_err(|err| svc_error!(ResponseStatus::INTERNAL_SERVER_ERROR, "{}", err))?;

                result?
            }
        };

        // Insert event into the DB.
        let occurred_at = match room.time() {
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
            &payload.data,
            occurred_at,
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

        // Respond to the agent and notify room subscribers.
        let response = helpers::build_response(
            ResponseStatus::CREATED,
            event.clone(),
            reqp,
            start_timestamp,
            Some(authz_time),
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
    last_occurred_at: Option<i64>,
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

    async fn handle<C: Context>(
        context: &C,
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

        if let Some(last_occurred_at) = payload.last_occurred_at {
            query = query.last_occurred_at(last_occurred_at);
        }

        if let Some(last_created_at) = payload.last_created_at {
            if payload.last_occurred_at.is_some() {
                query = query.last_created_at(last_created_at);
            } else {
                return Err(svc_error!(
                    ResponseStatus::BAD_REQUEST,
                    "`last_created_at` given without `last_occurred_at`"
                ));
            }
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

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::db::event::{Direction, Object as Event};
    use crate::test_helpers::prelude::*;

    use super::*;

    ///////////////////////////////////////////////////////////////////////////

    #[test]
    fn create_event() {
        futures::executor::block_on(async {
            let db = TestDb::new();
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                let conn = db
                    .connection_pool()
                    .get()
                    .expect("Failed to get DB connection");

                // Create room and put the agent online.
                let room = shared_helpers::insert_room(&conn);
                shared_helpers::insert_agent(&conn, agent.agent_id(), room.id());
                room
            };

            // Allow agent to create events of type `message` in the room.
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let account_id = agent.account_id().to_string();

            let object = vec![
                "rooms",
                &room_id,
                "events",
                "message",
                "authors",
                &account_id,
            ];

            authz.allow(agent.account_id(), object, "create");

            // Make event.create request.
            let context = TestContext::new(db, authz);

            let payload = CreateRequest {
                room_id: room.id(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                data: json!({ "text": "hello" }),
            };

            let messages = handle_request::<CreateHandler>(&context, &agent, payload)
                .await
                .expect("Event creation failed");

            // Assert response.
            let (event, respp) = find_response::<Event>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::CREATED);
            assert_eq!(event.room_id(), room.id());
            assert_eq!(event.kind(), "message");
            assert_eq!(event.set(), "messages");
            assert_eq!(event.label(), Some("message-1"));
            assert_eq!(event.data(), &json!({ "text": "hello" }));

            // Assert notification.
            let (event, evp, topic) = find_event::<Event>(messages.as_slice());
            assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
            assert_eq!(evp.label(), "event.create");
            assert_eq!(event.room_id(), room.id());
            assert_eq!(event.kind(), "message");
            assert_eq!(event.set(), "messages");
            assert_eq!(event.label(), Some("message-1"));
            assert_eq!(event.data(), &json!({ "text": "hello" }));
        });
    }

    #[test]
    fn create_event_not_authorized() {
        futures::executor::block_on(async {
            let db = TestDb::new();
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                let conn = db
                    .connection_pool()
                    .get()
                    .expect("Failed to get DB connection");

                // Create room and put the agent online.
                let room = shared_helpers::insert_room(&conn);
                shared_helpers::insert_agent(&conn, agent.agent_id(), room.id());
                room
            };

            // Make event.create request.
            let context = TestContext::new(db, TestAuthz::new());

            let payload = CreateRequest {
                room_id: room.id(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                data: json!({ "text": "hello" }),
            };

            let err = handle_request::<CreateHandler>(&context, &agent, payload)
                .await
                .expect_err("Unexpected success on event creation");

            assert_eq!(err.status_code(), ResponseStatus::FORBIDDEN);
        });
    }

    #[test]
    fn create_event_not_entered() {
        futures::executor::block_on(async {
            let db = TestDb::new();
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                let conn = db
                    .connection_pool()
                    .get()
                    .expect("Failed to get DB connection");

                // Create room.
                shared_helpers::insert_room(&conn)
            };

            // Allow agent to create events of type `message` in the room.
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let account_id = agent.account_id().to_string();

            let object = vec![
                "rooms",
                &room_id,
                "events",
                "message",
                "authors",
                &account_id,
            ];

            authz.allow(agent.account_id(), object, "create");

            // Make event.create request.
            let context = TestContext::new(db, authz);

            let payload = CreateRequest {
                room_id: room.id(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                data: json!({ "text": "hello" }),
            };

            let err = handle_request::<CreateHandler>(&context, &agent, payload)
                .await
                .expect_err("Unexpected success on event creation");

            assert_eq!(err.status_code(), ResponseStatus::FORBIDDEN);
        });
    }

    #[test]
    fn create_event_closed_room() {
        futures::executor::block_on(async {
            let db = TestDb::new();
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                let conn = db
                    .connection_pool()
                    .get()
                    .expect("Failed to get DB connection");

                // Create closed room and put the agent online.
                let room = shared_helpers::insert_closed_room(&conn);
                shared_helpers::insert_agent(&conn, agent.agent_id(), room.id());
                room
            };

            // Allow agent to create events of type `message` in the room.
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let account_id = agent.account_id().to_string();

            let object = vec![
                "rooms",
                &room_id,
                "events",
                "message",
                "authors",
                &account_id,
            ];

            authz.allow(agent.account_id(), object, "create");

            // Make event.create request.
            let context = TestContext::new(db, authz);

            let payload = CreateRequest {
                room_id: room.id(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                data: json!({ "text": "hello" }),
            };

            let err = handle_request::<CreateHandler>(&context, &agent, payload)
                .await
                .expect_err("Unexpected success on event creation");

            assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
        });
    }

    #[test]
    fn create_event_missing_room() {
        futures::executor::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let context = TestContext::new(TestDb::new(), TestAuthz::new());

            let payload = CreateRequest {
                room_id: Uuid::new_v4(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                data: json!({ "text": "hello" }),
            };

            let err = handle_request::<CreateHandler>(&context, &agent, payload)
                .await
                .expect_err("Unexpected success on event creation");

            assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
        });
    }

    ///////////////////////////////////////////////////////////////////////////

    #[test]
    fn list_events() {
        futures::executor::block_on(async {
            let db = TestDb::new();
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, db_events) = {
                let conn = db
                    .connection_pool()
                    .get()
                    .expect("Failed to get DB connection");

                // Create room.
                let room = shared_helpers::insert_room(&conn);

                // Create events in the room.
                let events = (1..4)
                    .map(|i| {
                        factory::Event::new()
                            .room_id(room.id())
                            .kind("message")
                            .data(&json!({ "text": format!("message {}", i) }))
                            .occurred_at(i * 1000)
                            .created_by(&agent.agent_id())
                            .insert(&conn)
                    })
                    .collect::<Vec<Event>>();

                (room, events)
            };

            // Allow agent to list events in the room.
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let object = vec!["rooms", &room_id, "events"];
            authz.allow(agent.account_id(), object, "list");

            // Make event.list request.
            let context = TestContext::new(db, authz);

            let payload = ListRequest {
                room_id: room.id(),
                kind: None,
                set: None,
                label: None,
                last_occurred_at: None,
                last_created_at: None,
                direction: Direction::Backward,
                limit: Some(2),
            };

            let messages = handle_request::<ListHandler>(&context, &agent, payload)
                .await
                .expect("Events listing failed (page 1)");

            // Assert last two events response.
            let (events, respp) = find_response::<Vec<Event>>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].id(), db_events[2].id());
            assert_eq!(events[1].id(), db_events[1].id());

            // Request the next page.
            let payload = ListRequest {
                room_id: room.id(),
                kind: None,
                set: None,
                label: None,
                last_occurred_at: Some(events[1].occurred_at()),
                last_created_at: Some(events[1].created_at()),
                direction: Direction::Backward,
                limit: Some(2),
            };

            let messages = handle_request::<ListHandler>(&context, &agent, payload)
                .await
                .expect("Events listing failed (page 2)");

            // Assert the first event.
            let (events, respp) = find_response::<Vec<Event>>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].id(), db_events[0].id());
        });
    }

    #[test]
    fn list_events_not_authorized() {
        futures::executor::block_on(async {
            let db = TestDb::new();
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                let conn = db
                    .connection_pool()
                    .get()
                    .expect("Failed to get DB connection");

                shared_helpers::insert_room(&conn)
            };

            let context = TestContext::new(db, TestAuthz::new());

            let payload = ListRequest {
                room_id: room.id(),
                kind: None,
                set: None,
                label: None,
                last_occurred_at: None,
                last_created_at: None,
                direction: Direction::Backward,
                limit: Some(2),
            };

            let err = handle_request::<ListHandler>(&context, &agent, payload)
                .await
                .expect_err("Unexpected success on events listing");

            assert_eq!(err.status_code(), ResponseStatus::FORBIDDEN);
        });
    }

    #[test]
    fn list_events_missing_room() {
        futures::executor::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let context = TestContext::new(TestDb::new(), TestAuthz::new());

            let payload = ListRequest {
                room_id: Uuid::new_v4(),
                kind: None,
                set: None,
                label: None,
                last_occurred_at: None,
                last_created_at: None,
                direction: Direction::Backward,
                limit: Some(2),
            };

            let err = handle_request::<ListHandler>(&context, &agent, payload)
                .await
                .expect_err("Unexpected success on events listing");

            assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
        });
    }
}
