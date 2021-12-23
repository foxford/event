use std::ops::Bound;

use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::extract::{Extension, Path, RawQuery};
use serde_derive::Deserialize;
use serde_json::{map::Map as JsonMap, Value as JsonValue};
use svc_agent::mqtt::ResponseStatus;
use svc_utils::extractors::AuthnExtractor;
use tracing::{field::display, instrument, Span};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::db;

///////////////////////////////////////////////////////////////////////////////

const MAX_SETS: usize = 10;
const MAX_LIMIT_PER_SET: i64 = 100;

#[derive(Debug, Deserialize)]
pub(crate) struct ReadPayload {
    sets: Vec<String>,
    attribute: Option<String>,
    occurred_at: Option<i64>,
    original_occurred_at: Option<i64>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReadRequest {
    room_id: Uuid,
    #[serde(flatten)]
    payload: ReadPayload,
}

pub async fn read(
    Extension(ctx): Extension<AppContext>,
    AuthnExtractor(agent_id): AuthnExtractor,
    Path(room_id): Path<Uuid>,
    RawQuery(query): RawQuery,
) -> RequestResult {
    let payload = serde_qs::from_str(&query.unwrap_or_default())
        .map_err(|e| anyhow!("Failed to parse qs, err = {:?}", e))
        .error(AppErrorKind::InvalidQueryString)?;
    let request = ReadRequest { room_id, payload };
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

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<C: Context>(
        context: &mut C,
        Self::Payload { room_id, payload }: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        Span::current().record("room_id", &display(room_id));

        // Validate parameters.
        let validation_error = match payload.sets.len() {
            0 => Some(anyhow!("'sets' can't be empty")),
            len if len > MAX_SETS => Some(anyhow!("too many 'sets'")),
            _ => None,
        };

        if let Some(err) = validation_error {
            return Err(err).error(AppErrorKind::InvalidStateSets);
        }

        // Choose limit.
        let limit = std::cmp::min(
            payload.limit.unwrap_or(MAX_LIMIT_PER_SET),
            MAX_LIMIT_PER_SET,
        );

        // Check whether the room exists.
        let room = helpers::find_room(context, room_id, helpers::RoomTimeRequirement::Any).await?;

        // Authorize room events listing.
        let room_id = room.id().to_string();
        let object = AuthzObject::new(&["rooms", &room_id]).into();

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "read".into(),
            )
            .await?;

        // Default `occurred_at`: closing time of the room.
        let time = room.time().map(|t| t.into());
        let original_occurred_at = if let Some(original_occurred_at) = payload.original_occurred_at
        {
            original_occurred_at
        } else if let Ok((_, Bound::Unbounded)) = time {
            std::i64::MAX
        } else if let Ok((Bound::Included(open), Bound::Excluded(close))) = time {
            (close - open)
                .num_nanoseconds()
                .map(|n| n + 1)
                .unwrap_or(std::i64::MAX)
        } else {
            return Err(anyhow!("Bad room time")).error(AppErrorKind::InvalidRoomTime);
        };

        // Retrieve state for each set from the DB and put them into a map.
        let mut state = JsonMap::new();
        let mut conn = context.get_ro_conn().await?;

        for set in payload.sets.iter() {
            Span::current().record("set", &set.as_str());

            // Build a query for the particular set state.
            let mut query =
                db::event::SetStateQuery::new(room.id(), set.clone(), original_occurred_at, limit);

            if let Some(ref attribute) = payload.attribute {
                query = query.attribute(attribute);
            }

            if let Some(occurred_at) = payload.occurred_at {
                query = query.occurred_at(occurred_at);
            }

            // If it is the only set specified at first execute a total count query and
            // add `has_next` pagination flag to the state.
            if payload.sets.len() == 1 {
                let total_count = context
                    .metrics()
                    .measure_query(QueryKey::StateTotalCountQuery, query.total_count(&mut conn))
                    .await
                    .context("Failed to get state total count")
                    .error(AppErrorKind::DbQueryFailed)?;

                let has_next = total_count as i64 > limit;
                state.insert(String::from("has_next"), JsonValue::Bool(has_next));
            }

            // Limit the query and retrieve the state.
            let set_state = context
                .metrics()
                .measure_query(QueryKey::StateQuery, query.execute(&mut conn))
                .await
                .context("Failed to get state")
                .error(AppErrorKind::DbQueryFailed)?;

            // Serialize to JSON and add to the state map.
            let serialized_set_state = serde_json::to_value(set_state)
                .context("Failed to serialize state")
                .error(AppErrorKind::SerializationFailed)?;

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
        Ok(AppResponse::new(
            ResponseStatus::OK,
            JsonValue::Object(state),
            context.start_timestamp(),
            Some(authz_time),
        ))
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

    #[tokio::test]
    async fn read_state_multiple_sets() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let (room, message_event, layout_event) = {
            // Create room.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_room(&mut conn).await;

            // Create events in the room.
            let message_event = factory::Event::new()
                .room_id(room.id())
                .kind("message")
                .set("messages")
                .label("message-1")
                .data(&json!({ "text": "hello", }))
                .occurred_at(1000)
                .created_by(&agent.agent_id())
                .insert(&mut conn)
                .await;

            let layout_event = factory::Event::new()
                .room_id(room.id())
                .kind("layout")
                .set("layout")
                .data(&json!({ "name": "presentation", }))
                .occurred_at(2000)
                .created_by(&agent.agent_id())
                .insert(&mut conn)
                .await;

            (room, message_event, layout_event)
        };

        // Allow agent to list events in the room.
        let mut authz = TestAuthz::new();
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id];
        authz.allow(agent.account_id(), object, "read");

        // Make state.read request.
        let mut context = TestContext::new(db, authz);

        let payload = ReadRequest {
            room_id: room.id(),
            payload: ReadPayload {
                sets: vec![String::from("messages"), String::from("layout")],
                attribute: None,
                occurred_at: None,
                original_occurred_at: None,
                limit: None,
            },
        };

        let messages = handle_request::<ReadHandler>(&mut context, &agent, payload)
            .await
            .expect("State reading failed");

        // Assert last two events response.
        let (state, respp, _) = find_response::<State>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].id(), message_event.id());
        assert_eq!(state.layout.id(), layout_event.id());
    }

    #[derive(Deserialize)]
    struct CollectionState {
        messages: Vec<Event>,
        has_next: bool,
    }

    #[tokio::test]
    async fn read_state_collection() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let (room, db_events) = {
            // Create room.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_room(&mut conn).await;

            // Create events in the room.
            let mut events = vec![];

            for i in 0..6 {
                let event = factory::Event::new()
                    .room_id(room.id())
                    .kind("message")
                    .set("messages")
                    .label(&format!("message-{}", i % 3 + 1))
                    .data(&json!({
                        "text": format!("message {}, version {}", i % 3 + 1, i / 3 + 1),
                    }))
                    .occurred_at(i * 1000)
                    .created_by(&agent.agent_id())
                    .insert(&mut conn)
                    .await;

                events.push(event);
            }

            (room, events)
        };

        // Allow agent to list events in the room.
        let mut authz = TestAuthz::new();
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id];
        authz.allow(agent.account_id(), object, "read");

        // Make state.read request.
        let mut context = TestContext::new(db, authz);

        let payload = ReadRequest {
            room_id: room.id(),
            payload: ReadPayload {
                sets: vec![String::from("messages")],
                attribute: None,
                occurred_at: Some(2001),
                original_occurred_at: None,
                limit: Some(2),
            },
        };

        let messages = handle_request::<ReadHandler>(&mut context, &agent, payload)
            .await
            .expect("State reading failed (page 1)");

        // Assert last two events response.
        let (state, respp, _) = find_response::<CollectionState>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.messages[0].id(), db_events[2].id());
        assert_eq!(state.messages[1].id(), db_events[1].id());
        assert_eq!(state.has_next, true);

        // Request the next page.
        let payload = ReadRequest {
            room_id: room.id(),
            payload: ReadPayload {
                sets: vec![String::from("messages")],
                attribute: None,
                occurred_at: Some(1),
                original_occurred_at: Some(state.messages[1].original_occurred_at()),
                limit: Some(2),
            },
        };

        let messages = handle_request::<ReadHandler>(&mut context, &agent, payload)
            .await
            .expect("State reading failed (page 2)");

        // Assert the first event.
        let (state, respp, _) = find_response::<CollectionState>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].id(), db_events[0].id());
        assert_eq!(state.has_next, false);
    }

    #[tokio::test]
    async fn read_state_collection_with_attribute_filter() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let room = {
            // Create room.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_room(&mut conn).await;

            // Create events in the room.
            let mut events = vec![];

            for i in 0..6 {
                let mut factory = factory::Event::new()
                    .room_id(room.id())
                    .kind("message")
                    .set("messages")
                    .label(&format!("message-{}", i % 3 + 1))
                    .data(&json!({
                        "text": format!("message {}, version {}", i % 3 + 1, i / 3 + 1),
                    }))
                    .occurred_at(i * 1000)
                    .created_by(&agent.agent_id());

                if i % 3 == 0 {
                    factory = factory.attribute("pinned");
                }

                let event = factory.insert(&mut conn).await;
                events.push(event);
            }

            room
        };

        // Allow agent to list events in the room.
        let mut authz = TestAuthz::new();
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id];
        authz.allow(agent.account_id(), object, "read");

        // Make state.read request.
        let mut context = TestContext::new(db, authz);

        let payload = ReadRequest {
            room_id: room.id(),
            payload: ReadPayload {
                sets: vec![String::from("messages")],
                attribute: Some(String::from("pinned")),
                occurred_at: None,
                original_occurred_at: None,
                limit: None,
            },
        };

        let messages = handle_request::<ReadHandler>(&mut context, &agent, payload)
            .await
            .expect("State reading failed");

        // Expect only an event with the expected attribute.
        let (state, respp, _) = find_response::<CollectionState>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].attribute(), Some("pinned"));
    }

    #[tokio::test]
    async fn read_state_collection_with_occurred_at_filter() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let (room, db_events) = {
            // Create room.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_room(&mut conn).await;

            // Create events in the room.
            let mut events = vec![];

            for i in 0..6 {
                let event = factory::Event::new()
                    .room_id(room.id())
                    .kind("message")
                    .set("messages")
                    .label(&format!("message-{}", i % 3 + 1))
                    .data(&json!({
                        "text": format!("message {}, version {}", i % 3 + 1, i / 3 + 1),
                    }))
                    .occurred_at(i * 1000)
                    .created_by(&agent.agent_id())
                    .insert(&mut conn)
                    .await;

                events.push(event);
            }

            (room, events)
        };

        // Allow agent to list events in the room.
        let mut authz = TestAuthz::new();
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id];
        authz.allow(agent.account_id(), object, "read");

        // Make state.read request.
        let mut context = TestContext::new(db, authz);

        let payload = ReadRequest {
            room_id: room.id(),
            payload: ReadPayload {
                sets: vec![String::from("messages")],
                attribute: None,
                occurred_at: Some(2001),
                original_occurred_at: None,
                limit: Some(2),
            },
        };

        let messages = handle_request::<ReadHandler>(&mut context, &agent, payload)
            .await
            .expect("State reading failed (page 1)");

        // Assert last two events response.
        let (state, respp, _) = find_response::<CollectionState>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.messages[0].id(), db_events[2].id());
        assert_eq!(state.messages[1].id(), db_events[1].id());
        assert_eq!(state.has_next, true);

        // Request the next page.
        let payload = ReadRequest {
            room_id: room.id(),
            payload: ReadPayload {
                sets: vec![String::from("messages")],
                attribute: None,
                occurred_at: Some(1),
                original_occurred_at: Some(state.messages[1].original_occurred_at()),
                limit: Some(2),
            },
        };

        let messages = handle_request::<ReadHandler>(&mut context, &agent, payload)
            .await
            .expect("State reading failed (page 2)");

        // Assert the first event.
        let (state, respp, _) = find_response::<CollectionState>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].id(), db_events[0].id());
        assert_eq!(state.has_next, false);
    }

    #[tokio::test]
    async fn read_state_pinned_messages() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let (room, pinned_message) = {
            // Create room.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_room(&mut conn).await;

            // Create a not pinned message.
            let base_message_factory = factory::Event::new()
                .room_id(room.id())
                .kind("message")
                .set("messages")
                .data(&json!({"text": "hello"}))
                .created_by(&agent.agent_id());

            base_message_factory
                .clone()
                .label("message-1")
                .occurred_at(1000)
                .insert(&mut conn)
                .await;

            // Create a pinned message.
            let pinned_message_factory = base_message_factory.clone().label("message-2");

            pinned_message_factory
                .clone()
                .occurred_at(2000)
                .insert(&mut conn)
                .await;

            let pinned_message = pinned_message_factory
                .occurred_at(3000)
                .attribute("pinned")
                .insert(&mut conn)
                .await;

            // Create an unpinned message.
            let unpinned_message_factory = base_message_factory.label("message-3");

            unpinned_message_factory
                .clone()
                .occurred_at(4000)
                .insert(&mut conn)
                .await;

            unpinned_message_factory
                .clone()
                .occurred_at(5000)
                .attribute("pinned")
                .insert(&mut conn)
                .await;

            unpinned_message_factory
                .occurred_at(6000)
                .insert(&mut conn)
                .await;

            (room, pinned_message)
        };

        // Allow agent to list events in the room.
        let mut authz = TestAuthz::new();
        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id];
        authz.allow(agent.account_id(), object, "read");

        // Make state.read request.
        let mut context = TestContext::new(db, authz);

        let payload = ReadRequest {
            room_id: room.id(),
            payload: ReadPayload {
                sets: vec![String::from("messages")],
                attribute: Some(String::from("pinned")),
                occurred_at: None,
                original_occurred_at: None,
                limit: None,
            },
        };

        let messages = handle_request::<ReadHandler>(&mut context, &agent, payload)
            .await
            .expect("State reading failed");

        // Assert last two events response.
        let (state, respp, _) = find_response::<CollectionState>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].id(), pinned_message.id());
    }

    #[tokio::test]
    async fn read_state_not_authorized() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let room = {
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        let mut context = TestContext::new(db, TestAuthz::new());

        let payload = ReadRequest {
            room_id: room.id(),
            payload: ReadPayload {
                sets: vec![String::from("messages"), String::from("layout")],
                attribute: None,
                occurred_at: None,
                original_occurred_at: None,
                limit: None,
            },
        };

        let err = handle_request::<ReadHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success reading state");

        assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
    }

    #[tokio::test]
    async fn read_state_missing_room() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

        let payload = ReadRequest {
            room_id: Uuid::new_v4(),
            payload: ReadPayload {
                sets: vec![String::from("messages"), String::from("layout")],
                attribute: None,
                occurred_at: None,
                original_occurred_at: None,
                limit: None,
            },
        };

        let err = handle_request::<ReadHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success reading state");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_not_found");
    }
}
