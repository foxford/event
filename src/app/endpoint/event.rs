use std::sync::Arc;

use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::Utc;
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use svc_agent::Authenticable;
use svc_agent::{mqtt::ResponseStatus, Addressable};
use svc_utils::extractors::AgentIdExtractor;
use tracing::{field::display, instrument, Span};
use uuid::Uuid;

use crate::app::endpoint::prelude::*;
use crate::db;
use crate::db::event::Object as Event;

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePayload {
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
    #[serde(default)]
    pub removed: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateRequest {
    pub room_id: Uuid,
    #[serde(flatten)]
    pub payload: CreatePayload,
}

impl CreateRequest {
    fn default_is_claim() -> bool {
        false
    }

    fn default_is_persistent() -> bool {
        true
    }
}

pub async fn create(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
    Json(payload): Json<CreatePayload>,
) -> RequestResult {
    let request = CreateRequest { room_id, payload };
    CreateHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub(crate) struct CreateHandler;

#[derive(Serialize)]
pub(crate) struct TenantClaimNotification {
    #[serde(flatten)]
    event: Event,
    classroom_id: Uuid,
}

#[async_trait]
impl RequestHandler for CreateHandler {
    type Payload = CreateRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<'a, C: Context + Sync + Send>(
        context: &'a mut C,
        Self::Payload { room_id, payload }: Self::Payload,
        reqp: RequestParams<'a>,
    ) -> RequestResult {
        let (room, author) = {
            let room =
                helpers::find_room(context, room_id, helpers::RoomTimeRequirement::Open).await?;

            let author = match payload {
                // Get author of the original event with the same label if applicable.
                CreatePayload {
                    set: Some(ref set),
                    label: Some(ref label),
                    ..
                } => {
                    Span::current().record("set", set.as_str());
                    Span::current().record("set_label", label.as_str());

                    let query = db::event::OriginalEventQuery::new(
                        room.id(),
                        set.to_owned(),
                        label.to_owned(),
                    );

                    let mut conn = context.get_ro_conn().await?;

                    context
                        .metrics()
                        .measure_query(QueryKey::EventOriginalEventQuery, query.execute(&mut conn))
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

        // NOTE:
        // Currently we simply override authz object to room update if event type is in locked_types
        // or if room mandates whiteboard access validation and the user is not allowed access through whiteboard access map
        //
        // Assumption here is that admins can always update rooms and its ok for them to post messages in locked chat
        // So room update authz check works for them
        // But the same check always fails for common users
        //
        // This is probably a temporary solution, relying on room update being allowed only to those who can post in locked chat
        let (object, action) = {
            let object = room.authz_object();
            let mut object = object.iter().map(|s| s.as_ref()).collect::<Vec<_>>();

            if room.event_should_authz_room_update(&payload.kind, reqp.as_account_id()) {
                (AuthzObject::new(&object).into(), "update")
            } else {
                object.extend([key, &payload.kind, "authors", &author].iter());

                (AuthzObject::new(&object).into(), "create")
            }
        };

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                action.into(),
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
        let CreatePayload {
            kind,
            data,
            set,
            label,
            attribute,
            removed,
            ..
        } = payload;

        if data.to_string().len() >= context.config().constraint.payload_size {
            return Err(anyhow!("Payload size exceeded")).error(AppErrorKind::PayloadSizeExceeded);
        }

        let event = if payload.is_persistent {
            // Insert event into the DB.
            let mut query = db::event::InsertQuery::new(
                room.id(),
                kind,
                data,
                occurred_at,
                reqp.as_agent_id().to_owned(),
            )
            .error(AppErrorKind::InvalidEvent)?;

            if let Some(set) = set {
                query = query.set(set);
            }

            if let Some(label) = label {
                query = query.label(label);
            }

            if let Some(attribute) = attribute {
                query = query.attribute(attribute);
            }

            if removed {
                query = query.removed(true);
            }

            {
                let mut conn = context.get_conn().await?;

                let event = context
                    .metrics()
                    .measure_query(QueryKey::EventInsertQuery, query.execute(&mut conn))
                    .await
                    .context("Failed to insert event")
                    .error(AppErrorKind::DbQueryFailed)?;

                Span::current().record("event_id", &display(event.id()));
                event
            }
        } else {
            // Build transient event.
            let mut builder = db::event::Builder::new()
                .room_id(room_id)
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
                .map_err(|err| anyhow!("Error building transient event: {:?}", err))
                .error(AppErrorKind::TransientEventCreationFailed)?
        };

        // Respond to the agent.
        let mut response = AppResponse::new(
            ResponseStatus::CREATED,
            event.clone(),
            context.start_timestamp(),
            Some(authz_time),
        );

        // If the event is claim notify the tenant.
        if is_claim {
            let claim_notification = TenantClaimNotification {
                event: event.clone(),
                classroom_id: room.classroom_id(),
            };

            response.add_notification(
                "event.create",
                &format!("audiences/{}/events", room.audience()),
                claim_notification,
                context.start_timestamp(),
            );
        }

        // Notify room subscribers.
        response.add_notification(
            "event.create",
            &format!("rooms/{}/events", room.id()),
            event,
            context.start_timestamp(),
        );

        Ok(response)
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
pub struct ListPayload {
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

#[derive(Debug, Deserialize)]
pub struct ListRequest {
    room_id: Uuid,
    #[serde(flatten)]
    payload: ListPayload,
}

pub async fn list(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
    Query(payload): Query<ListPayload>,
) -> RequestResult {
    let request = ListRequest { room_id, payload };
    ListHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub(crate) struct ListHandler;

#[async_trait]
impl RequestHandler for ListHandler {
    type Payload = ListRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<'a, C: Context + Sync + Send>(
        context: &'a mut C,
        Self::Payload { room_id, payload }: Self::Payload,
        reqp: RequestParams<'a>,
    ) -> RequestResult {
        let room = helpers::find_room(context, room_id, helpers::RoomTimeRequirement::Any).await?;

        // Authorize room events listing.
        let classroom_id = room.classroom_id().to_string();
        let object = AuthzObject::new(&["classrooms", &classroom_id]).into();

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

        let ListPayload {
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
                .metrics()
                .measure_query(QueryKey::EventListQuery, query.execute(&mut conn))
                .await
                .context("Failed to list events")
                .error(AppErrorKind::DbQueryFailed)?
        };

        // Respond with events list.
        Ok(AppResponse::new(
            ResponseStatus::OK,
            events,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use crate::db::event::{Direction, Object as Event};
    use crate::test_helpers::outgoing_envelope::OutgoingEnvelopeProperties;
    use crate::test_helpers::prelude::*;

    use super::*;

    ///////////////////////////////////////////////////////////////////////////

    #[tokio::test]
    async fn create_event() {
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
        let classroom_id = room.classroom_id().to_string();
        let account_id = agent.account_id().to_string();

        let object = vec![
            "classrooms",
            &classroom_id,
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
            payload: CreatePayload {
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: Some(String::from("pinned")),
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
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
    }

    #[tokio::test]
    async fn exceed_payload_size() {
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
        let classroom_id = room.classroom_id().to_string();
        let account_id = agent.account_id().to_string();

        let object = vec![
            "classrooms",
            &classroom_id,
            "pinned",
            "message",
            "authors",
            &account_id,
        ];

        authz.allow(agent.account_id(), object, "create");

        // Make event.create request.
        let mut context = TestContext::new_with_payload_size(db, authz, 10);

        let payload = CreateRequest {
            room_id: room.id(),
            payload: CreatePayload {
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: Some(String::from("pinned")),
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Event creation succeeded");
    }

    #[tokio::test]
    async fn create_locked_event_as_user() {
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
        let classroom_id = room.classroom_id().to_string();
        let account_id = agent.account_id().to_string();

        let object = vec![
            "classrooms",
            &classroom_id,
            "events",
            "message",
            "authors",
            &account_id,
        ];

        authz.allow(agent.account_id(), object, "create");

        // Make event.create request. It should succeed
        let mut context = TestContext::new(db.clone(), authz);

        let payload = CreateRequest {
            room_id: room.id(),
            payload: CreatePayload {
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect("Event creation failed");

        // Lock messages for users
        {
            let mut m = HashMap::new();
            m.insert("message".into(), true);
            let q = db::room::UpdateQuery::new(room.id()).locked_types(m);
            let mut conn = db.get_conn().await;
            q.execute(&mut conn).await.expect("Failed to lock type");
        }

        // Make event.create request. Now it should fail since we locked kind='message' events
        let payload = CreateRequest {
            room_id: room.id(),
            payload: CreatePayload {
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-2")),
                attribute: None,
                data: json!({ "text": "locked chat hello" }),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Event creation succeeded");
    }

    #[tokio::test]
    async fn create_locked_event_as_room_updater() {
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
        let classroom_id = room.classroom_id().to_string();
        let account_id = agent.account_id().to_string();

        let object = vec![
            "classrooms",
            &classroom_id,
            "events",
            "message",
            "authors",
            &account_id,
        ];

        authz.allow(agent.account_id(), object, "create");

        let object = vec!["classrooms", &classroom_id];
        authz.allow(agent.account_id(), object, "update");

        // Make event.create request.
        let mut context = TestContext::new(db.clone(), authz);

        let payload = CreateRequest {
            room_id: room.id(),
            payload: CreatePayload {
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect("Event creation failed");

        // Lock messages for users
        {
            let mut m = HashMap::new();
            m.insert("message".into(), true);
            let q = db::room::UpdateQuery::new(room.id()).locked_types(m);
            let mut conn = db.get_conn().await;
            q.execute(&mut conn).await.expect("Failed to lock type");
        }

        let payload = CreateRequest {
            room_id: room.id(),
            payload: CreatePayload {
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-2")),
                attribute: None,
                data: json!({ "text": "locked chat hello" }),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect("Event creation failed");
    }

    #[tokio::test]
    async fn create_next_event() {
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
        let classroom_id = room.classroom_id().to_string();

        // Should authorize with the author of the original event.
        let account_id = original_author.agent_id().as_account_id().to_string();

        let object = vec![
            "classrooms",
            &classroom_id,
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
            payload: CreatePayload {
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "modified text" }),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect("Event creation failed");

        // Assert response.
        let (event, respp, _) = find_response::<Event>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::CREATED);
        assert_eq!(event.created_by(), agent.agent_id());
    }

    #[tokio::test]
    async fn create_claim() {
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
        let classroom_id = room.classroom_id().to_string();
        let account_id = agent.account_id().to_string();
        let object = vec![
            "classrooms",
            &classroom_id,
            "claims",
            "block",
            "authors",
            &account_id,
        ];
        authz.allow(agent.account_id(), object, "create");

        // Make event.create request.
        let mut context = TestContext::new(db, authz);

        let payload = CreateRequest {
            room_id: room.id(),
            payload: CreatePayload {
                kind: String::from("block"),
                set: Some(String::from("blocks")),
                label: Some(String::from("user-1")),
                attribute: None,
                data: json!({ "blocked": true }),
                is_claim: true,
                is_persistent: true,
                removed: false,
            },
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
    }

    #[tokio::test]
    async fn create_transient_event() {
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
        let classroom_id = room.classroom_id().to_string();
        let account_id = agent.account_id().to_string();

        let object = vec![
            "classrooms",
            &classroom_id,
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
            payload: CreatePayload {
                kind: String::from("cursor"),
                set: None,
                label: None,
                attribute: None,
                data: data.clone(),
                is_claim: false,
                is_persistent: false,
                removed: false,
            },
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
    }

    #[tokio::test]
    async fn create_event_not_authorized() {
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
            payload: CreatePayload {
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        let err = handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on event creation");

        assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
    }

    #[tokio::test]
    async fn create_event_not_entered() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let room = {
            // Create room.
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        // Allow agent to create events of type `message` in the room.
        let mut authz = TestAuthz::new();
        let classroom_id = room.classroom_id().to_string();
        let account_id = agent.account_id().to_string();

        let object = vec![
            "classrooms",
            &classroom_id,
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
            payload: CreatePayload {
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect("Event creation failed");

        assert_eq!(messages.len(), 2);
    }

    #[tokio::test]
    async fn create_event_closed_room() {
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
        let classroom_id = room.classroom_id().to_string();
        let account_id = agent.account_id().to_string();

        let object = vec![
            "classrooms",
            &classroom_id,
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
            payload: CreatePayload {
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        let err = handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on event creation");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_closed");
    }

    #[tokio::test]
    async fn create_event_missing_room() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

        let payload = CreateRequest {
            room_id: Uuid::new_v4(),
            payload: CreatePayload {
                kind: String::from("message"),
                set: Some(String::from("messages")),
                label: Some(String::from("message-1")),
                attribute: None,
                data: json!({ "text": "hello" }),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        let err = handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on event creation");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_not_found");
    }

    #[tokio::test]
    async fn create_whiteboard_event_without_whiteboard_access() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let room = {
            // Create room and put the agent online.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_validating_whiteboard_access_room(&mut conn).await;
            shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
            room
        };

        // Allow agent to create events of type `draw` in the room.
        let mut authz = TestAuthz::new();
        let account_id = agent.account_id().to_string();
        let classroom_id = room.classroom_id().to_string();

        let object = vec![
            "classrooms",
            &classroom_id,
            "events",
            "draw",
            "authors",
            &account_id,
        ];

        authz.allow(agent.account_id(), object, "create");

        let mut context = TestContext::new(db.clone(), authz);

        // Make event.create request. It should fail
        let payload = CreateRequest {
            room_id: room.id(),
            payload: CreatePayload {
                kind: String::from("draw"),
                set: Some(String::from("set")),
                label: Some(String::from("label-1")),
                attribute: None,
                data: json!({ "foo": "bar" }),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Event creation succeeded");

        // Update whiteboard access for the agent
        {
            let mut m = HashMap::new();
            m.insert(agent.account_id().to_owned(), true);
            let q = db::room::UpdateQuery::new(room.id()).whiteboard_access(m);
            let mut conn = db.get_conn().await;
            q.execute(&mut conn)
                .await
                .expect("Failed to update whiteboard access");
        }

        // Make event.create request. Now it should succeed
        let payload = CreateRequest {
            room_id: room.id(),
            payload: CreatePayload {
                kind: String::from("draw"),
                set: Some(String::from("set")),
                label: Some(String::from("label-2")),
                attribute: None,
                data: crate::db::event::CompactEvent::test_rect_event()
                    .into_json()
                    .unwrap(),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect("Failed to create event");
    }

    #[tokio::test]
    async fn create_whiteboard_event_as_room_updater() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let room = {
            // Create room and put the agent online.
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_validating_whiteboard_access_room(&mut conn).await;
            shared_helpers::insert_agent(&mut conn, agent.agent_id(), room.id()).await;
            room
        };

        // Allow agent to create events of type `draw` in the room.
        let mut authz = TestAuthz::new();
        let classroom_id = room.classroom_id().to_string();
        let account_id = agent.account_id().to_string();

        let object = vec![
            "classrooms",
            &classroom_id,
            "events",
            "draw",
            "authors",
            &account_id,
        ];

        authz.allow(agent.account_id(), object, "create");

        let mut context = TestContext::new(db.clone(), authz.clone());

        let payload = CreateRequest {
            room_id: room.id(),
            payload: CreatePayload {
                kind: String::from("draw"),
                set: Some(String::from("set")),
                label: Some(String::from("label-2")),
                attribute: None,
                data: crate::db::event::CompactEvent::test_rect_event()
                    .into_json()
                    .unwrap(),
                is_claim: false,
                is_persistent: true,
                removed: false,
            },
        };

        // This must fail since user has no room-update access and whiteboard access map is empty
        handle_request::<CreateHandler>(&mut context, &agent, payload.clone())
            .await
            .expect_err("Event creation succeeded");

        let object = vec!["classrooms", &classroom_id];
        authz.allow(agent.account_id(), object, "update");
        let mut context = TestContext::new(db.clone(), authz);

        // This must succeed cause even though whiteboard access map is empty user is allowed to update the room
        handle_request::<CreateHandler>(&mut context, &agent, payload)
            .await
            .expect("Event creation failed");
    }

    ///////////////////////////////////////////////////////////////////////////

    #[tokio::test]
    async fn list_events() {
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
        let classroom_id = room.classroom_id().to_string();
        let object = vec!["classrooms", &classroom_id];
        authz.allow(agent.account_id(), object, "read");

        // Make event.list request.
        let mut context = TestContext::new(db, authz);

        let payload = ListRequest {
            room_id: room.id(),
            payload: ListPayload {
                kind: None,
                set: None,
                label: None,
                attribute: None,
                last_occurred_at: None,
                direction: Direction::Backward,
                limit: Some(2),
            },
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
            payload: ListPayload {
                kind: None,
                set: None,
                label: None,
                attribute: None,
                last_occurred_at: Some(events[1].occurred_at()),
                direction: Direction::Backward,
                limit: Some(2),
            },
        };

        let messages = handle_request::<ListHandler>(&mut context, &agent, payload)
            .await
            .expect("Events listing failed (page 2)");

        // Assert the first event.
        let (events, respp, _) = find_response::<Vec<Event>>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id(), db_events[0].id());
    }

    #[tokio::test]
    async fn list_events_filtered_by_kinds() {
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
        let classroom_id = room.classroom_id().to_string();
        let object = vec!["classrooms", &classroom_id];
        authz.allow(agent.account_id(), object, "read");

        // Make event.list request.
        let mut context = TestContext::new(db, authz);

        let payload = ListRequest {
            room_id: room.id(),
            payload: ListPayload {
                kind: Some(ListTypesFilter::Single("B".to_string())),
                set: None,
                label: None,
                attribute: None,
                last_occurred_at: None,
                direction: Direction::Backward,
                limit: None,
            },
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
            payload: ListPayload {
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
            },
        };

        let messages = handle_request::<ListHandler>(&mut context, &agent, payload)
            .await
            .expect("Events listing failed");

        // we have two kind=B events and one kind=A event
        let (events, respp, _) = find_response::<Vec<Event>>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(events.len(), 3);
    }

    #[tokio::test]
    async fn list_events_filter_by_attribute() {
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
        let classroom_id = room.classroom_id().to_string();
        let object = vec!["classrooms", &classroom_id];
        authz.allow(agent.account_id(), object, "read");

        // Make event.list request.
        let mut context = TestContext::new(db, authz);

        let payload = ListRequest {
            room_id: room.id(),
            payload: ListPayload {
                kind: None,
                set: None,
                label: None,
                attribute: Some(String::from("pinned")),
                last_occurred_at: None,
                direction: Direction::Backward,
                limit: None,
            },
        };

        let messages = handle_request::<ListHandler>(&mut context, &agent, payload)
            .await
            .expect("Events listing failed");

        // Expect only the event with the `pinned` attribute value.
        let (events, respp, _) = find_response::<Vec<Event>>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::OK);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].attribute(), Some("pinned"));
    }

    #[tokio::test]
    async fn list_events_not_authorized() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let room = {
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        let mut context = TestContext::new(db, TestAuthz::new());

        let payload = ListRequest {
            room_id: room.id(),
            payload: ListPayload {
                kind: None,
                set: None,
                label: None,
                attribute: None,
                last_occurred_at: None,
                direction: Direction::Backward,
                limit: Some(2),
            },
        };

        let err = handle_request::<ListHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on events listing");

        assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
    }

    #[tokio::test]
    async fn list_events_missing_room() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

        let payload = ListRequest {
            room_id: Uuid::new_v4(),
            payload: ListPayload {
                kind: None,
                set: None,
                label: None,
                attribute: None,
                last_occurred_at: None,
                direction: Direction::Backward,
                limit: Some(2),
            },
        };

        let err = handle_request::<ListHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on events listing");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_not_found");
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

        assert_eq!(x.payload.kind, None);

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
            x.payload.kind,
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

        assert_eq!(
            x.payload.kind,
            Some(ListTypesFilter::Single("test".to_string()))
        );

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
            x.payload.kind,
            Some(ListTypesFilter::Multiple(vec!["test".to_string()]))
        );
    }
}
