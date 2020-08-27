use std::ops::Bound;

use async_std::stream;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_derive::Deserialize;
use serde_json::{map::Map as JsonMap, Value as JsonValue};
use svc_agent::mqtt::{IncomingRequestProperties, ResponseStatus};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::db;

///////////////////////////////////////////////////////////////////////////////

const MAX_SETS: usize = 10;
const MAX_LIMIT_PER_SET: i64 = 100;

#[derive(Debug, Deserialize)]
pub(crate) struct ReadRequest {
    room_id: Uuid,
    sets: Vec<String>,
    occurred_at: Option<i64>,
    original_occurred_at: Option<i64>,
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
    ) -> Result {
        // Validate parameters.
        let validation_error = match payload.sets.len() {
            0 => Some("'sets' can't be empty"),
            len if len > MAX_SETS => Some("too many 'sets'"),
            _ => None,
        };

        if let Some(err) = validation_error {
            return Err(err).status(ResponseStatus::BAD_REQUEST);
        }

        // Choose limit.
        let limit = std::cmp::min(
            payload.limit.unwrap_or_else(|| MAX_LIMIT_PER_SET),
            MAX_LIMIT_PER_SET,
        );

        // Check whether the room exists.
        let room = {
            let conn = context.ro_db().get()?;

            db::room::FindQuery::new(payload.room_id)
                .execute(&conn)?
                .ok_or_else(|| format!("the room = '{}' is not found", payload.room_id))
                .status(ResponseStatus::NOT_FOUND)?
        };

        // Authorize room events listing.
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id, "events"];

        let authz_time = context
            .authz()
            .authorize(room.audience(), reqp, object, "list")
            .await?;

        // Default `occurred_at`: closing time of the room.
        let original_occurred_at = if let Some(original_occurred_at) = payload.original_occurred_at
        {
            original_occurred_at
        } else if let (Bound::Included(opened_at), Bound::Excluded(closed_at)) = room.time() {
            (*closed_at - *opened_at)
                .num_nanoseconds()
                .map(|n| n + 1)
                .unwrap_or(std::i64::MAX)
        } else {
            return Err("Bad room time").status(ResponseStatus::UNPROCESSABLE_ENTITY);
        };

        // Retrieve state for each set from the DB and put them into a map.
        let mut state = JsonMap::new();
        let conn = context.ro_db().get()?;

        for set in payload.sets.iter() {
            // Build a query for the particular set state.
            let mut query =
                db::event::SetStateQuery::new(room.id(), &set, original_occurred_at, limit);

            if let Some(occurred_at) = payload.occurred_at {
                query = query.occurred_at(occurred_at);
            }

            // If it is the only set specified at first execute a total count query and
            // add `has_next` pagination flag to the state.
            if payload.sets.len() == 1 {
                let total_count = query
                    .total_count(&conn)
                    .map_err(|err| {
                        format!(
                            "failed to query state total count for set = '{}': {}",
                            set, err
                        )
                    })
                    .status(ResponseStatus::UNPROCESSABLE_ENTITY)?;

                let has_next = total_count as i64 > limit;
                state.insert(String::from("has_next"), JsonValue::Bool(has_next));
            }

            // Limit the query and retrieve the state.
            let set_state = query
                .execute(&conn)
                .map_err(|err| format!("failed to query state for set = '{}': {}", set, err))
                .status(ResponseStatus::UNPROCESSABLE_ENTITY)?;

            // Serialize to JSON and add to the state map.
            let serialized_set_state = serde_json::to_value(set_state)
                .map_err(|err| format!("failed to serialize state for set = '{}': {}", set, err))
                .status(ResponseStatus::UNPROCESSABLE_ENTITY)?;

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
        Ok(Box::new(stream::once(helpers::build_response(
            ResponseStatus::OK,
            JsonValue::Object(state),
            reqp,
            start_timestamp,
            Some(authz_time),
        ))))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use serde_derive::Deserialize;
    use serde_json::json;

    use crate::db::event::Object as Event;
    use crate::test_helpers::prelude::*;

    use super::*;

    ///////////////////////////////////////////////////////////////////////////

    #[derive(Deserialize)]
    struct State {
        messages: Vec<Event>,
        layout: Event,
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
                original_occurred_at: None,
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
        messages: Vec<Event>,
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
                occurred_at: Some(2001),
                original_occurred_at: None,
                limit: Some(2),
            };

            let messages = handle_request::<ReadHandler>(&context, &agent, payload)
                .await
                .expect("State reading failed (page 1)");

            // Assert last two events response.
            let (state, respp) = find_response::<CollectionState>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(state.messages.len(), 2);
            assert_eq!(state.messages[0].id(), db_events[2].id());
            assert_eq!(state.messages[1].id(), db_events[1].id());
            assert_eq!(state.has_next, true);

            // Request the next page.
            let payload = ReadRequest {
                room_id: room.id(),
                sets: vec![String::from("messages")],
                occurred_at: Some(1),
                original_occurred_at: Some(state.messages[1].original_occurred_at()),
                limit: Some(2),
            };

            let messages = handle_request::<ReadHandler>(&context, &agent, payload)
                .await
                .expect("State reading failed (page 2)");

            // Assert the first event.
            let (state, respp) = find_response::<CollectionState>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(state.messages.len(), 1);
            assert_eq!(state.messages[0].id(), db_events[0].id());
            assert_eq!(state.has_next, false);
        });
    }

    #[test]
    fn read_state_collection_with_occurred_at_filter() {
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
                occurred_at: Some(2001),
                original_occurred_at: None,
                limit: Some(2),
            };

            let messages = handle_request::<ReadHandler>(&context, &agent, payload)
                .await
                .expect("State reading failed (page 1)");

            // Assert last two events response.
            let (state, respp) = find_response::<CollectionState>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(state.messages.len(), 2);
            assert_eq!(state.messages[0].id(), db_events[2].id());
            assert_eq!(state.messages[1].id(), db_events[1].id());
            assert_eq!(state.has_next, true);

            // Request the next page.
            let payload = ReadRequest {
                room_id: room.id(),
                sets: vec![String::from("messages")],
                occurred_at: Some(1),
                original_occurred_at: Some(state.messages[1].original_occurred_at()),
                limit: Some(2),
            };

            let messages = handle_request::<ReadHandler>(&context, &agent, payload)
                .await
                .expect("State reading failed (page 2)");

            // Assert the first event.
            let (state, respp) = find_response::<CollectionState>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(state.messages.len(), 1);
            assert_eq!(state.messages[0].id(), db_events[0].id());
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
                original_occurred_at: None,
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
                original_occurred_at: None,
                limit: None,
            };

            let err = handle_request::<ReadHandler>(&context, &agent, payload)
                .await
                .expect_err("Unexpected success reading state");

            assert_eq!(err.status_code(), ResponseStatus::NOT_FOUND);
        });
    }
}
