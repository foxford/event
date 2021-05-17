use anyhow::Context as AnyhowContext;
use async_std::stream;
use async_trait::async_trait;
use chrono::Utc;
use serde_derive::Deserialize;
use serde_json::Value as JsonValue;
use svc_agent::Authenticable;
use svc_agent::{
    mqtt::{IncomingRequestProperties, ResponseStatus},
    Addressable,
};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::db;

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct CreateRequest {
    pub room_id: Uuid,
    #[serde(rename = "type")]
    pub kind: String,
    pub set: Option<String>,
    pub label: Option<String>,
    pub attribute: Option<String>,
    pub data: JsonValue,
    #[serde(default = "CreateRequest::default_is_claim")]
    pub is_claim: bool,
    #[serde(default = "CreateRequest::default_is_persistent")]
    pub is_persistent: bool,
}

impl CreateRequest {
    fn default_is_claim() -> bool {
        false
    }

    fn default_is_persistent() -> bool {
        true
    }
}

pub(crate) struct CreateHandler;

#[async_trait]
impl RequestHandler for CreateHandler {
    type Payload = CreateRequest;

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        let (room, author) = {
            let room = helpers::find_room(
                context,
                payload.room_id,
                helpers::RoomTimeRequirement::Open,
                reqp.method(),
            )
            .await?;

            let author = match payload {
                // Get author of the original event with the same label if applicable.
                CreateRequest {
                    set: Some(ref set),
                    label: Some(ref label),
                    ..
                } => {
                    context.add_logger_tags(o!(
                        "set" => set.to_string(),
                        "set_label" => label.to_string(),
                    ));

                    let query = db::event::OriginalEventQuery::new(
                        room.id(),
                        set.to_owned(),
                        label.to_owned(),
                    );

                    let mut conn = context.get_ro_conn().await?;

                    context
                        .profiler()
                        .measure(
                            (
                                ProfilerKeys::EventOriginalEventQuery,
                                Some(reqp.method().to_owned()),
                            ),
                            query.execute(&mut conn),
                        )
                        .await
                        .context("Failed to find original event")
                        .error(AppErrorKind::DbQueryFailed)?
                        .map(|original_event| {
                            original_event.created_by().as_account_id().to_string()
                        })
                }
                _ => None,
            }
            .unwrap_or_else(|| {
                // If set & label are not given or there're no events for them use current account.
                reqp.as_account_id().to_string()
            });

            (room, author)
        };

        let is_claim = payload.is_claim;

        // Authorize event creation on tenant with cache.
        let key = if let Some(ref attribute) = payload.attribute {
            attribute
        } else if payload.is_claim {
            "claims"
        } else {
            "events"
        };

        let object = {
            let object = room.authz_object();
            let mut object = object.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
            object.extend([key, &payload.kind, "authors", &author].iter());
            AuthzObject::new(&object).into()
        };

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "create".into(),
            )
            .await?;

        // Calculate occurrence date.
        let occurred_at = match room.time().map(|t| t.start().to_owned()) {
            Ok(opened_at) => (Utc::now() - opened_at)
                .num_nanoseconds()
                .unwrap_or(std::i64::MAX),
            _ => {
                return Err(anyhow!("Invalid room time")).error(AppErrorKind::InvalidRoomTime);
            }
        };

        let event = if payload.is_persistent {
            // Insert event into the DB.
            let CreateRequest {
                kind,
                data,
                set,
                label,
                attribute,
                ..
            } = payload;

            let mut query = db::event::InsertQuery::new(
                room.id(),
                kind,
                data,
                occurred_at,
                reqp.as_agent_id().to_owned(),
            );

            if let Some(set) = set {
                query = query.set(set);
            }

            if let Some(label) = label {
                query = query.label(label);
            }

            if let Some(attribute) = attribute {
                query = query.attribute(attribute);
            }

            {
                let mut conn = context.get_conn().await?;

                let event = context
                    .profiler()
                    .measure(
                        (
                            ProfilerKeys::EventInsertQuery,
                            Some(reqp.method().to_owned()),
                        ),
                        query.execute(&mut conn),
                    )
                    .await
                    .context("Failed to insert event")
                    .error(AppErrorKind::DbQueryFailed)?;

                context.add_logger_tags(o!("event_id" => event.id().to_string()));
                event
            }
        } else {
            let CreateRequest {
                kind,
                data,
                set,
                label,
                attribute,
                ..
            } = payload;

            // Build transient event.
            let mut builder = db::event::Builder::new()
                .room_id(payload.room_id)
                .kind(&kind)
                .data(&data)
                .occurred_at(occurred_at)
                .created_by(reqp.as_agent_id());

            if let Some(ref set) = set {
                builder = builder.set(set)
            }

            if let Some(ref label) = label {
                builder = builder.label(label)
            }

            if let Some(ref attribute) = attribute {
                builder = builder.attribute(attribute)
            }

            builder
                .build()
                .map_err(|err| anyhow!("Error building transient event: {}", err,))
                .error(AppErrorKind::TransientEventCreationFailed)?
        };

        let mut messages = Vec::with_capacity(3);

        // Respond to the agent.
        messages.push(helpers::build_response(
            ResponseStatus::CREATED,
            event.clone(),
            reqp,
            context.start_timestamp(),
            Some(authz_time),
        ));

        // If the event is claim notify the tenant.
        if is_claim {
            messages.push(helpers::build_notification(
                "event.create",
                &format!("audiences/{}/events", room.audience()),
                event.clone(),
                reqp,
                context.start_timestamp(),
            ));
        }

        // Notify room subscribers.
        messages.push(helpers::build_notification(
            "event.create",
            &format!("rooms/{}/events", room.id()),
            event,
            reqp,
            context.start_timestamp(),
        ));

        Ok(Box::new(stream::from_iter(messages)))
    }
}

///////////////////////////////////////////////////////////////////////////////

const MAX_LIMIT: usize = 100;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
enum ListTypesFilter {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListRequest {
    room_id: Uuid,
    #[serde(rename = "type")]
    kind: Option<ListTypesFilter>,
    set: Option<String>,
    label: Option<String>,
    attribute: Option<String>,
    last_occurred_at: Option<i64>,
    #[serde(default)]
    direction: db::event::Direction,
    limit: Option<usize>,
}

pub(crate) struct ListHandler;

#[async_trait]
impl RequestHandler for ListHandler {
    type Payload = ListRequest;

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        let room = helpers::find_room(
            context,
            payload.room_id,
            helpers::RoomTimeRequirement::Any,
            reqp.method(),
        )
        .await?;

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

        // Retrieve events from the DB.
        let mut query = db::event::ListQuery::new().room_id(room.id());

        let ListRequest {
            kind,
            set,
            label,
            attribute,
            last_occurred_at,
            ..
        } = payload;

        query = match kind {
            Some(ListTypesFilter::Single(kind)) => query.kind(kind),
            Some(ListTypesFilter::Multiple(kinds)) => query.kinds(kinds),
            None => query,
        };

        if let Some(ref set) = set {
            query = query.set(set);
        }

        if let Some(ref label) = label {
            query = query.label(label);
        }

        if let Some(ref attribute) = attribute {
            query = query.attribute(attribute);
        }

        if let Some(last_occurred_at) = last_occurred_at {
            query = query.last_occurred_at(last_occurred_at);
        }

        let events = {
            let mut conn = context.get_ro_conn().await?;

            query = query
                .direction(payload.direction)
                .limit(std::cmp::min(payload.limit.unwrap_or(MAX_LIMIT), MAX_LIMIT));

            context
                .profiler()
                .measure(
                    (ProfilerKeys::EventListQuery, Some(reqp.method().to_owned())),
                    query.execute(&mut conn),
                )
                .await
                .context("Failed to list events")
                .error(AppErrorKind::DbQueryFailed)?
        };

        // Respond with events list.
        Ok(Box::new(stream::once(helpers::build_response(
            ResponseStatus::OK,
            events,
            reqp,
            context.start_timestamp(),
            Some(authz_time),
        ))))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::db::event::{Direction, Object as Event};
    use crate::test_helpers::outgoing_envelope::OutgoingEnvelopeProperties;
    use crate::test_helpers::prelude::*;

    use super::*;

    ///////////////////////////////////////////////////////////////////////////

    #[test]
    fn create_event() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
                room
            };

            // Allow agent to create events of type `message` in the room.
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let account_id = agent.account_id().to_string();

            let object = vec![
                "rooms",
                &room_id,
                "pinned",
                "message",
                "authors",
                &account_id,
            ];

            authz.allow(agent.account_id(), object, "create");

            // Make event.create request.
            let mut context = TestContext::new(db, authz);

            let payload = CreateRequest {
                room_id: room.id(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: Some(String::from("pinned")),
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
            };

            let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect("Event creation failed");

            assert_eq!(messages.len(), 2);

            // Assert response.
            let (event, respp, _) = find_response::<Event>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::CREATED);
            assert_eq!(event.room_id(), room.id());
            assert_eq!(event.kind(), "message");
            assert_eq!(event.set(), "messages");
            assert_eq!(event.label(), Some("message-1"));
            assert_eq!(event.attribute(), Some("pinned"));
            assert_eq!(event.data(), &json!({ "text": "hello" }));

            // Assert notification.
            let (event, evp, topic) = find_event::<Event>(messages.as_slice());
            assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
            assert_eq!(evp.label(), "event.create");
            assert_eq!(event.room_id(), room.id());
            assert_eq!(event.kind(), "message");
            assert_eq!(event.set(), "messages");
            assert_eq!(event.label(), Some("message-1"));
            assert_eq!(event.attribute(), Some("pinned"));
            assert_eq!(event.data(), &json!({ "text": "hello" }));
        });
    }

    #[test]
    fn create_next_event() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let original_author = TestAgent::new("web", "user123", USR_AUDIENCE);
            let agent = TestAgent::new("web", "moderator", USR_AUDIENCE);

            let room = {
                // Create room.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                // Add an event to the room.
                factory::Event::new()
                    .room_id(room.id())
                    .kind("message")
                    .set("messages")
                    .label("message-1")
                    .data(&json!({ "text": "original text" }))
                    .occurred_at(1_000_000_000)
                    .created_by(&original_author.agent_id())
                    .insert(&mut conn)
                    .await;

                // Put the agent online.
                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
                room
            };

            // Allow agent to create events of type `message` in the room.
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();

            // Should authorize with the author of the original event.
            let account_id = original_author.agent_id().as_account_id().to_string();

            let object = vec![
                "rooms",
                &room_id,
                "events",
                "message",
                "authors",
                &account_id,
            ];

            authz.allow(agent.account_id(), object, "create");

            // Make event.create request with the same set/label as existing event.
            let mut context = TestContext::new(db, authz);

            let payload = CreateRequest {
                room_id: room.id(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "modified text" }),
                is_claim: false,
                is_persistent: true,
            };

            let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect("Event creation failed");

            // Assert response.
            let (event, respp, _) = find_response::<Event>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::CREATED);
            assert_eq!(event.created_by(), agent.agent_id());
        });
    }

    #[test]
    fn create_claim() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
                room
            };

            // Allow agent to create claims of type `block` in the room.
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let account_id = agent.account_id().to_string();
            let object = vec!["rooms", &room_id, "claims", "block", "authors", &account_id];
            authz.allow(agent.account_id(), object, "create");

            // Make event.create request.
            let mut context = TestContext::new(db, authz);

            let payload = CreateRequest {
                room_id: room.id(),
                kind: String::from("block"),
                set: Some(String::from("blocks")),
                label: Some(String::from("user-1")),
                attribute: None,
                data: json!({ "blocked": true }),
                is_claim: true,
                is_persistent: true,
            };

            let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect("Event creation failed");

            assert_eq!(messages.len(), 3);

            // Assert response.
            let (event, respp, _) = find_response::<Event>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::CREATED);
            assert_eq!(event.room_id(), room.id());
            assert_eq!(event.kind(), "block");
            assert_eq!(event.set(), "blocks");
            assert_eq!(event.label(), Some("user-1"));
            assert_eq!(event.data(), &json!({ "blocked": true }));

            // Assert tenant & room notifications.
            let mut has_tenant_notification = false;
            let mut has_room_notification = false;

            for message in messages {
                if let OutgoingEnvelopeProperties::Event(evp) = message.properties() {
                    let topic = message.topic();

                    if topic.ends_with(&format!("/audiences/{}/events", room.audience())) {
                        has_tenant_notification = true;
                    }

                    if topic.ends_with(&format!("/rooms/{}/events", room.id())) {
                        has_room_notification = true;
                    }

                    assert_eq!(evp.label(), "event.create");

                    let event = message.payload::<Event>();
                    assert_eq!(event.room_id(), room.id());
                    assert_eq!(event.kind(), "block");
                    assert_eq!(event.set(), "blocks");
                    assert_eq!(event.label(), Some("user-1"));
                    assert_eq!(event.data(), &json!({ "blocked": true }));
                }
            }

            assert_eq!(has_tenant_notification, true);
            assert_eq!(has_room_notification, true);
        });
    }

    #[test]
    fn create_transient_event() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
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
                "cursor",
                "authors",
                &account_id,
            ];

            authz.allow(agent.account_id(), object, "create");

            // Make event.create request.
            let mut context = TestContext::new(db, authz);

            let data = json!({
                "agent_id": agent.agent_id().to_string(),
                "x": 123,
                "y": 456,
            });

            let payload = CreateRequest {
                room_id: room.id(),
                kind: String::from("cursor"),
                set: None,
                label: None,
                attribute: None,
                data: data.clone(),
                is_claim: false,
                is_persistent: false,
            };

            let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect("Event creation failed");

            assert_eq!(messages.len(), 2);

            // Assert response.
            let (event, respp, _) = find_response::<Event>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::CREATED);
            assert_eq!(event.room_id(), room.id());
            assert_eq!(event.kind(), "cursor");
            assert_eq!(event.set(), "cursor");
            assert_eq!(event.label(), None);
            assert_eq!(event.data(), &data);

            // Assert notification.
            let (event, evp, topic) = find_event::<Event>(messages.as_slice());
            assert!(topic.ends_with(&format!("/rooms/{}/events", room.id())));
            assert_eq!(evp.label(), "event.create");
            assert_eq!(event.room_id(), room.id());
            assert_eq!(event.kind(), "cursor");
            assert_eq!(event.set(), "cursor");
            assert_eq!(event.label(), None);
            assert_eq!(event.data(), &data);
        });
    }

    #[test]
    fn create_event_not_authorized() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room and put the agent online.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
                room
            };

            // Make event.create request.
            let mut context = TestContext::new(db, TestAuthz::new());

            let payload = CreateRequest {
                room_id: room.id(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
            };

            let err = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on event creation");

            assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
        });
    }

    #[test]
    fn create_event_not_entered() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room.
                let mut conn = db.get_conn().await;
                shared_helpers::insert_room(&mut conn).await
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
            let mut context = TestContext::new(db, authz);

            let payload = CreateRequest {
                room_id: room.id(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
            };

            let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect("Event creation failed");

            assert_eq!(messages.len(), 2);
        });
    }

    #[test]
    fn create_event_closed_room() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create closed room and put the agent online.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_closed_room(&mut conn).await;
                shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
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
            let mut context = TestContext::new(db, authz);

            let payload = CreateRequest {
                room_id: room.id(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
            };

            let err = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on event creation");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_closed");
        });
    }

    #[test]
    fn create_event_missing_room() {
        async_std::task::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

            let payload = CreateRequest {
                room_id: Uuid::new_v4(),
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
            };

            let err = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on event creation");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_not_found");
        });
    }

    ///////////////////////////////////////////////////////////////////////////

    #[test]
    fn list_events() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, db_events) = {
                // Create room.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                // Create events in the room.
                let mut events = vec![];

                for i in 1..4 {
                    let event = factory::Event::new()
                        .room_id(room.id())
                        .kind("message")
                        .data(&json!({ "text": format!("message {}", i) }))
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

            // Make event.list request.
            let mut context = TestContext::new(db, authz);

            let payload = ListRequest {
                room_id: room.id(),
                kind: None,
                set: None,
                label: None,
                attribute: None,
                last_occurred_at: None,
                direction: Direction::Backward,
                limit: Some(2),
            };

            let messages = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect("Events listing failed (page 1)");

            // Assert last two events response.
            let (events, respp, _) = find_response::<Vec<Event>>(messages.as_slice());
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
                attribute: None,
                last_occurred_at: Some(events[1].occurred_at()),
                direction: Direction::Backward,
                limit: Some(2),
            };

            let messages = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect("Events listing failed (page 2)");

            // Assert the first event.
            let (events, respp, _) = find_response::<Vec<Event>>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].id(), db_events[0].id());
        });
    }

    #[test]
    fn list_events_filtered_by_kinds() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                // Create events in the room.
                for (i, s) in ["A", "B", "A", "C"].iter().enumerate() {
                    factory::Event::new()
                        .room_id(room.id())
                        .kind(s)
                        .data(&json!({ "text": format!("message {}", i) }))
                        .occurred_at(i as i64 * 1000)
                        .created_by(&agent.agent_id())
                        .insert(&mut conn)
                        .await;
                }

                room
            };

            // Allow agent to list events in the room.
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let object = vec!["rooms", &room_id];
            authz.allow(agent.account_id(), object, "read");

            // Make event.list request.
            let mut context = TestContext::new(db, authz);

            let payload = ListRequest {
                room_id: room.id(),
                kind: Some(ListTypesFilter::Single("B".to_string())),
                set: None,
                label: None,
                attribute: None,
                last_occurred_at: None,
                direction: Direction::Backward,
                limit: None,
            };

            let messages = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect("Events listing failed");

            // we have only two kind=B events
            let (events, respp, _) = find_response::<Vec<Event>>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(events.len(), 1);

            let payload = ListRequest {
                room_id: room.id(),
                kind: Some(ListTypesFilter::Multiple(vec![
                    "B".to_string(),
                    "A".to_string(),
                ])),
                set: None,
                label: None,
                attribute: None,
                last_occurred_at: None,
                direction: Direction::Backward,
                limit: None,
            };

            let messages = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect("Events listing failed");

            // we have two kind=B events and one kind=A event
            let (events, respp, _) = find_response::<Vec<Event>>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(events.len(), 3);
        });
    }

    #[test]
    fn list_events_filter_by_attribute() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                // Create room.
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                // Create events in the room.
                for (i, attr) in [None, Some("pinned"), Some("other")].iter().enumerate() {
                    let mut factory = factory::Event::new()
                        .room_id(room.id())
                        .kind("message")
                        .data(&json!({ "text": format!("message {}", i) }))
                        .occurred_at(i as i64 * 1000)
                        .created_by(&agent.agent_id());

                    if let Some(attribute) = attr {
                        factory = factory.attribute(attribute);
                    }

                    factory.insert(&mut conn).await;
                }

                room
            };

            // Allow agent to list events in the room.
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let object = vec!["rooms", &room_id];
            authz.allow(agent.account_id(), object, "read");

            // Make event.list request.
            let mut context = TestContext::new(db, authz);

            let payload = ListRequest {
                room_id: room.id(),
                kind: None,
                set: None,
                label: None,
                attribute: Some(String::from("pinned")),
                last_occurred_at: None,
                direction: Direction::Backward,
                limit: None,
            };

            let messages = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect("Events listing failed");

            // Expect only the event with the `pinned` attribute value.
            let (events, respp, _) = find_response::<Vec<Event>>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].attribute(), Some("pinned"));
        });
    }

    #[test]
    fn list_events_not_authorized() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                let mut conn = db.get_conn().await;
                shared_helpers::insert_room(&mut conn).await
            };

            let mut context = TestContext::new(db, TestAuthz::new());

            let payload = ListRequest {
                room_id: room.id(),
                kind: None,
                set: None,
                label: None,
                attribute: None,
                last_occurred_at: None,
                direction: Direction::Backward,
                limit: Some(2),
            };

            let err = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on events listing");

            assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
        });
    }

    #[test]
    fn list_events_missing_room() {
        async_std::task::block_on(async {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

            let payload = ListRequest {
                room_id: Uuid::new_v4(),
                kind: None,
                set: None,
                label: None,
                attribute: None,
                last_occurred_at: None,
                direction: Direction::Backward,
                limit: Some(2),
            };

            let err = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on events listing");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_not_found");
        });
    }

    #[test]
    fn parse_list_request() {
        let x: ListRequest = serde_json::from_str(
            r#"
            {
                "room_id": "c1e48d94-8c7e-49bc-af1c-fc77a63f72e6"
            }
        "#,
        )
        .unwrap();

        assert_eq!(x.kind, None);

        let x: ListRequest = serde_json::from_str(
            r#"
            {
                "room_id": "c1e48d94-8c7e-49bc-af1c-fc77a63f72e6",
                "type": ["a", "c", "x"]
            }
        "#,
        )
        .unwrap();

        assert_eq!(
            x.kind,
            Some(ListTypesFilter::Multiple(vec![
                "a".to_string(),
                "c".to_string(),
                "x".to_string()
            ]))
        );

        let x: ListRequest = serde_json::from_str(
            r#"
            {
                "room_id": "c1e48d94-8c7e-49bc-af1c-fc77a63f72e6",
                "type": "test"
            }
        "#,
        )
        .unwrap();

        assert_eq!(x.kind, Some(ListTypesFilter::Single("test".to_string())));

        let x: ListRequest = serde_json::from_str(
            r#"
            {
                "room_id": "c1e48d94-8c7e-49bc-af1c-fc77a63f72e6",
                "type": ["test"]
            }
        "#,
        )
        .unwrap();

        assert_eq!(
            x.kind,
            Some(ListTypesFilter::Multiple(vec!["test".to_string()]))
        );
    }
}
