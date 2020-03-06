use std::ops::Bound;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_derive::Deserialize;
use serde_json::{map::Map as JsonMap, Value as JsonValue};
use svc_agent::mqtt::{IncomingRequestProperties, IntoPublishableDump, ResponseStatus};
use svc_error::Error as SvcError;
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::{helpers, RequestHandler};
use crate::db;

///////////////////////////////////////////////////////////////////////////////

const MAX_SETS: usize = 10;
const MAX_LIMIT_PER_SET: i64 = 100;

#[derive(Debug, Deserialize)]
pub(crate) struct ReadRequest {
    room_id: Uuid,
    sets: Vec<String>,
    occurred_at: Option<i64>,
    limit: Option<i64>,
}

pub(crate) struct ReadHandler;

#[async_trait]
impl RequestHandler for ReadHandler {
    type Payload = ReadRequest;
    const ERROR_TITLE: &'static str = "Failed to read state";

    async fn handle<C: Context>(
        context: &C,
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

        // Default `occurred_at`: closing time of the room.
        let occurred_at = if let Some(occurred_at) = payload.occurred_at {
            occurred_at
        } else if let (Bound::Included(opened_at), Bound::Excluded(closed_at)) = room.time() {
            (*closed_at - *opened_at)
                .num_nanoseconds()
                .unwrap_or(std::i64::MAX)
        } else {
            return Err(svc_error!(
                ResponseStatus::UNPROCESSABLE_ENTITY,
                "Bad room time"
            ));
        };

        // Retrieve state for each set from the DB and put them into a map.
        let mut state = JsonMap::new();

        for set in payload.sets.iter() {
            // Build a query for the particular set state.
            let query = db::event::SetStateQuery::new(room.id(), &set, occurred_at, limit);

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

                let has_next = total_count as i64 > limit;
                state.insert(String::from("has_next"), JsonValue::Bool(has_next));
            }

            // Limit the query and retrieve the state.
            let set_state = query.execute(&conn).map_err(|err| {
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

            match serialized_set_state.as_array().and_then(|a| a.first()) {
                Some(event) if event.get("label").is_none() => {
                    // The first event has no label => simple set with a single event…
                    state.insert(set.to_owned(), event.to_owned());
                }
                _ => {
                    // …or it's a collection.
                    state.insert(set.to_owned(), serialized_set_state);
                }
            }
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

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use serde_derive::Deserialize;
    use serde_json::json;

    use crate::db::event::{Object as Event, SetStateItem};
    use crate::test_helpers::prelude::*;

    use super::*;

    ///////////////////////////////////////////////////////////////////////////

    #[derive(Deserialize)]
    struct State {
        messages: Vec<SetStateItem>,
        layout: SetStateItem,
    }

    #[test]
    fn read_state_multiple_sets() {
        futures::executor::block_on(async {
            let db = TestDb::new();
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, message_event, layout_event) = {
                let conn = db
                    .connection_pool()
                    .get()
                    .expect("Failed to get DB connection");

                // Create room.
                let room = shared_helpers::insert_room(&conn);

                // Create events in the room.
                let message_event = factory::Event::new()
                    .room_id(room.id())
                    .kind("message")
                    .set("messages")
                    .label("message-1")
                    .data(&json!({ "text": "hello", }))
                    .occurred_at(1000)
                    .created_by(&agent.agent_id())
                    .insert(&conn);

                let layout_event = factory::Event::new()
                    .room_id(room.id())
                    .kind("layout")
                    .set("layout")
                    .data(&json!({ "name": "presentation", }))
                    .occurred_at(2000)
                    .created_by(&agent.agent_id())
                    .insert(&conn);

                (room, message_event, layout_event)
            };

            // Allow agent to list events in the room.
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let object = vec!["rooms", &room_id, "events"];
            authz.allow(agent.account_id(), object, "list");

            // Make state.read request.
            let context = TestContext::new(db, authz);

            let payload = ReadRequest {
                room_id: room.id(),
                sets: vec![String::from("messages"), String::from("layout")],
                occurred_at: None,
                limit: None,
            };

            let messages = handle_request::<ReadHandler>(&context, &agent, payload)
                .await
                .expect("State reading failed");

            // Assert last two events response.
            let (state, respp) = find_response::<State>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(state.messages.len(), 1);
            assert_eq!(state.messages[0].id(), message_event.id());
            assert_eq!(state.layout.id(), layout_event.id());
        });
    }

    #[derive(Deserialize)]
    struct CollectionState {
        messages: Vec<SetStateItem>,
        has_next: bool,
    }

    #[test]
    fn read_state_collection() {
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
                let events = (0..6)
                    .map(|i| {
                        factory::Event::new()
                            .room_id(room.id())
                            .kind("message")
                            .set("messages")
                            .label(&format!("message-{}", i % 3 + 1))
                            .data(&json!({
                                "text": format!("message {}, version {}", i % 3 + 1, i / 3 + 1),
                            }))
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

            // Make state.read request.
            let context = TestContext::new(db, authz);

            let payload = ReadRequest {
                room_id: room.id(),
                sets: vec![String::from("messages")],
                occurred_at: None,
                limit: Some(2),
            };

            let messages = handle_request::<ReadHandler>(&context, &agent, payload)
                .await
                .expect("State reading failed (page 1)");

            // Assert last two events response.
            let (state, respp) = find_response::<CollectionState>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(state.messages.len(), 2);
            assert_eq!(state.messages[0].id(), db_events[5].id());
            assert_eq!(state.messages[1].id(), db_events[4].id());
            assert_eq!(state.has_next, true);

            // Request the next page.
            let payload = ReadRequest {
                room_id: room.id(),
                sets: vec![String::from("messages")],
                occurred_at: Some(state.messages[1].original_occurred_at()),
                limit: Some(2),
            };

            let messages = handle_request::<ReadHandler>(&context, &agent, payload)
                .await
                .expect("State reading failed (page 2)");

            // Assert the first event.
            let (state, respp) = find_response::<CollectionState>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(state.messages.len(), 1);
            assert_eq!(state.messages[0].id(), db_events[3].id());
            assert_eq!(state.has_next, false);
        });
    }

    #[test]
    fn read_state_not_authorized() {
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

            let payload = ReadRequest {
                room_id: room.id(),
                sets: vec![String::from("messages"), String::from("layout")],
                occurred_at: None,
                limit: None,
            };

            let err = handle_request::<ReadHandler>(&context, &agent, payload)
                .await
                .expect_err("Unexpected success reading state");

            assert_eq!(err.status_code(), ResponseStatus::FORBIDDEN);
        });
    }

    #[test]
    fn read_state_missing_room() {
        futures::executor::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let context = TestContext::new(TestDb::new(), TestAuthz::new());

            let payload = ReadRequest {
                room_id: Uuid::new_v4(),
                sets: vec![String::from("messages"), String::from("layout")],
                occurred_at: None,
                limit: None,
            };

            let err = handle_request::<ReadHandler>(&context, &agent, payload)
                .await
                .expect_err("Unexpected success reading state");

            assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
        });
    }
}
