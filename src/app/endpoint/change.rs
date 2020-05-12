use async_std::stream;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_derive::Deserialize;
use svc_agent::mqtt::{IncomingRequestProperties, ResponseStatus};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::db;

pub(crate) struct CreateHandler;

use crate::app::endpoint::change::create_request::{Changeset, CreateRequest};

#[async_trait]
impl RequestHandler for CreateHandler {
    type Payload = CreateRequest;
    const ERROR_TITLE: &'static str = "Failed to create change";

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result {
        let conn = context.db().get()?;

        let room = match db::edition::FindWithRoomQuery::new(payload.edition_id).execute(&conn)? {
            Some((_edition, room)) => room,
            None => {
                return Err(format!("Edition not found, id = '{}'", payload.edition_id))
                    .status(ResponseStatus::NOT_FOUND)?;
            }
        };

        let authz_time = context
            .authz()
            .authorize(
                room.audience(),
                reqp,
                vec!["rooms", &room.id().to_string()],
                "update",
            )
            .await?;

        let query =
            db::change::InsertQuery::new(payload.edition_id, payload.changeset.as_changetype());

        let query = match payload.changeset {
            Changeset::Addition(ref event) => query
                .event_kind(&event.kind)
                .event_set(&event.set)
                .event_label(&event.label)
                .event_data(&event.data)
                .event_occurred_at(event.occurred_at)
                .event_created_by(&event.created_by),
            Changeset::Modification(ref event) => {
                let query = query.event_id(event.event_id);

                let query = match event.kind {
                    Some(ref kind) => query.event_kind(kind),
                    None => query,
                };

                let query = query.event_set(&event.set);
                let query = query.event_label(&event.label);

                let query = match event.data {
                    Some(ref data) => query.event_data(data),
                    None => query,
                };

                let query = match event.occurred_at {
                    Some(v) => query.event_occurred_at(v),
                    None => query,
                };

                match event.created_by {
                    Some(ref agent_id) => query.event_created_by(agent_id),
                    None => query,
                }
            }
            Changeset::Removal(ref event) => query.event_id(event.event_id),
        };

        let change = query.execute(&conn)?;

        let response = helpers::build_response(
            ResponseStatus::CREATED,
            change,
            reqp,
            start_timestamp,
            Some(authz_time),
        );

        Ok(Box::new(stream::from_iter(vec![response])))
    }
}

pub(crate) struct ListHandler;

#[derive(Debug, Deserialize)]
pub(crate) struct ListRequest {
    id: Uuid,
    last_created_at: Option<DateTime<Utc>>,
    limit: Option<i64>,
}

#[async_trait]
impl RequestHandler for ListHandler {
    type Payload = ListRequest;
    const ERROR_TITLE: &'static str = "Failed to list changes";

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result {
        let conn = context.db().get()?;

        let (edition, room) =
            match db::edition::FindWithRoomQuery::new(payload.id).execute(&conn)? {
                Some((edition, room)) => (edition, room),
                None => {
                    return Err(format!("Edition not found, id = '{}'", payload.id))
                        .status(ResponseStatus::NOT_FOUND)?;
                }
            };

        let authz_time = context
            .authz()
            .authorize(
                room.audience(),
                reqp,
                vec!["rooms", &room.id().to_string()],
                "update",
            )
            .await?;

        let mut query = db::change::ListQuery::new(edition.id());

        if let Some(last_created_at) = payload.last_created_at {
            query = query.last_created_at(last_created_at.clone());
        }

        if let Some(limit) = payload.limit {
            query = query.limit(limit);
        }

        let changes = query.execute(&conn)?;

        Ok(Box::new(stream::from_iter(vec![helpers::build_response(
            ResponseStatus::OK,
            changes,
            reqp,
            start_timestamp,
            Some(authz_time),
        )])))
    }
}

pub(crate) struct DeleteHandler;

#[derive(Debug, Deserialize)]
pub(crate) struct DeleteRequest {
    id: Uuid,
}

#[async_trait]
impl RequestHandler for DeleteHandler {
    type Payload = DeleteRequest;
    const ERROR_TITLE: &'static str = "Failed to delete change";

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result {
        let conn = context.db().get()?;
        let (change, room) =
            match db::change::FindWithEditionAndRoomQuery::new(payload.id).execute(&conn)? {
                Some((change, (_edition, room))) => (change, room),
                None => {
                    return Err(format!("Change not found, id = '{}'", payload.id))
                        .status(ResponseStatus::NOT_FOUND)?;
                }
            };

        let authz_time = context
            .authz()
            .authorize(
                room.audience(),
                reqp,
                vec!["rooms", &room.id().to_string()],
                "update",
            )
            .await?;

        db::change::DeleteQuery::new(change.id()).execute(&conn)?;

        let response = helpers::build_response(
            ResponseStatus::OK,
            change,
            reqp,
            start_timestamp,
            Some(authz_time),
        );

        Ok(Box::new(stream::from_iter(vec![response])))
    }
}

#[cfg(test)]
mod tests {
    mod create {
        use super::super::*;
        use crate::db::change::{ChangeType, Object as Change};
        use crate::test_helpers::prelude::*;

        use serde_json::json;

        use crate::app::endpoint::change::create_request::{
            AdditionData, Changeset, CreateRequest, ModificationData, RemovalData,
        };

        #[test]
        fn create_addition_change() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (room, edition) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);
                    let edition = shared_helpers::insert_edition(&conn, &room, &agent.agent_id());
                    (room, edition)
                };

                // Allow agent to create editions
                let mut authz = TestAuthz::new();
                let room_id = room.id().to_string();

                let object = vec!["rooms", &room_id];

                authz.allow(agent.account_id(), object, "update");

                // Make edition.create request
                let context = TestContext::new(db, authz);

                let payload = CreateRequest {
                    edition_id: edition.id(),
                    changeset: Changeset::Addition(AdditionData {
                        kind: "something".to_owned(),
                        set: Some("type".to_owned()),
                        label: None,
                        data: json![{"key": "value" }],
                        occurred_at: 0,
                        created_by: agent.agent_id().to_owned(),
                    }),
                };

                let messages = handle_request::<CreateHandler>(&context, &agent, payload)
                    .await
                    .expect("Failed to create change");

                // Assert response
                let (change, respp) = find_response::<Change>(messages.as_slice());
                assert_eq!(respp.status(), ResponseStatus::CREATED);
                assert_eq!(change.edition_id(), edition.id());
                assert_eq!(change.kind(), ChangeType::Addition);
                assert_eq!(
                    change
                        .event_data()
                        .as_ref()
                        .expect("Couldnt get event data from Change"),
                    &json![{"key": "value"}]
                );
            });
        }

        #[test]
        fn create_removal_change() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (room, edition, events) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);
                    let edition = shared_helpers::insert_edition(&conn, &room, &agent.agent_id());
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
                        .collect::<Vec<crate::db::event::Object>>();
                    (room, edition, events)
                };

                // Allow agent to create editions
                let mut authz = TestAuthz::new();
                let room_id = room.id().to_string();

                let object = vec!["rooms", &room_id];

                authz.allow(agent.account_id(), object, "update");

                // Make edition.create request
                let context = TestContext::new(db, authz);

                let payload = CreateRequest {
                    edition_id: edition.id(),
                    changeset: Changeset::Removal(RemovalData {
                        event_id: events[0].id(),
                        kind: None,
                        occurred_at: None,
                        set: None,
                    }),
                };

                let messages = handle_request::<CreateHandler>(&context, &agent, payload)
                    .await
                    .expect("Failed to create change");

                // Assert response
                let (change, respp) = find_response::<Change>(messages.as_slice());
                assert_eq!(respp.status(), ResponseStatus::CREATED);
                assert_eq!(change.edition_id(), edition.id());
                assert_eq!(change.kind(), ChangeType::Removal);
                assert_eq!(change.event_id().unwrap(), events[0].id());
            });
        }

        #[test]
        fn create_modification_change() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (room, edition, events) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);
                    let edition = shared_helpers::insert_edition(&conn, &room, &agent.agent_id());
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
                        .collect::<Vec<crate::db::event::Object>>();
                    (room, edition, events)
                };

                // Allow agent to create editions
                let mut authz = TestAuthz::new();
                let room_id = room.id().to_string();

                let object = vec!["rooms", &room_id];

                authz.allow(agent.account_id(), object, "update");

                // Make edition.create request
                let context = TestContext::new(db, authz);

                let payload = CreateRequest {
                    edition_id: edition.id(),
                    changeset: Changeset::Modification(ModificationData {
                        event_id: events[0].id(),
                        kind: Some("something".to_owned()),
                        set: Some("type".to_owned()),
                        label: None,
                        data: Some(json![{"key": "value"}]),
                        occurred_at: Some(0),
                        created_by: Some(agent.agent_id().to_owned()),
                    }),
                };

                let messages = handle_request::<CreateHandler>(&context, &agent, payload)
                    .await
                    .expect("Failed to create change");

                // Assert response
                let (change, respp) = find_response::<Change>(messages.as_slice());
                assert_eq!(respp.status(), ResponseStatus::CREATED);
                assert_eq!(change.edition_id(), edition.id());
                assert_eq!(change.kind(), ChangeType::Modification);
                assert_eq!(change.event_id().unwrap(), events[0].id());
                assert_eq!(
                    change
                        .event_data()
                        .as_ref()
                        .expect("Couldnt get event data from ChangeWithChangeEvent"),
                    &json![{"key": "value"}]
                );
            });
        }

        #[test]
        fn create_change_with_improper_event_id() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (room, edition) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);
                    let edition = shared_helpers::insert_edition(&conn, &room, &agent.agent_id());

                    (room, edition)
                };

                // Allow agent to create editions
                let mut authz = TestAuthz::new();
                let room_id = room.id().to_string();

                let object = vec!["rooms", &room_id];

                authz.allow(agent.account_id(), object, "update");

                // Make edition.create request
                let context = TestContext::new(db, authz);

                let payload = CreateRequest {
                    edition_id: edition.id(),
                    changeset: Changeset::Removal(RemovalData {
                        event_id: Uuid::new_v4(),
                        kind: None,
                        occurred_at: None,
                        set: None,
                    }),
                };

                let response = handle_request::<CreateHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success creating change with wrong params");

                assert_eq!(response.status_code(), ResponseStatus::UNPROCESSABLE_ENTITY);
            });
        }

        #[test]
        fn create_change_not_authorized() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (_room, edition) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);
                    let edition = shared_helpers::insert_edition(&conn, &room, &agent.agent_id());
                    (room, edition)
                };

                let context = TestContext::new(db, TestAuthz::new());

                let payload = CreateRequest {
                    edition_id: edition.id(),
                    changeset: Changeset::Addition(AdditionData {
                        kind: "something".to_owned(),
                        set: Some("type".to_owned()),
                        label: None,
                        data: json![{"key": "value"}],
                        occurred_at: 0,
                        created_by: agent.agent_id().to_owned(),
                    }),
                };

                let response = handle_request::<CreateHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success creating change with no authorization");

                assert_eq!(response.status_code(), ResponseStatus::FORBIDDEN);
            });
        }
        #[test]
        fn create_change_missing_edition() {
            futures::executor::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let context = TestContext::new(TestDb::new(), TestAuthz::new());

                let payload = CreateRequest {
                    edition_id: Uuid::new_v4(),
                    changeset: Changeset::Addition(AdditionData {
                        kind: "something".to_owned(),
                        label: None,
                        set: Some("type".to_owned()),
                        data: json![{"key": "value" }],
                        occurred_at: 0,
                        created_by: agent.agent_id().to_owned(),
                    }),
                };

                let response = handle_request::<CreateHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success creating change for no edition");

                assert_eq!(response.status_code(), ResponseStatus::NOT_FOUND);
            });
        }
    }

    mod list {
        use super::super::*;
        use crate::db::change::{ChangeType, Object as Change};
        use crate::test_helpers::prelude::*;
        use serde_json::json;

        #[test]
        fn list_changes() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (room, edition, changes) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);
                    let edition = shared_helpers::insert_edition(&conn, &room, &agent.agent_id());

                    let changes = (1..35)
                        .map(|idx| {
                            let event = factory::Event::new()
                                .room_id(room.id())
                                .kind("message")
                                .data(&json!({ "text": format!("message {}", idx) }))
                                .occurred_at(idx * 1000)
                                .created_by(&agent.agent_id())
                                .insert(&conn);

                            factory::Change::new(edition.id(), ChangeType::Modification)
                                .event_id(event.id())
                                .event_data(json![{"key": "value"}])
                                .insert(&conn)
                        })
                        .collect::<Vec<Change>>();

                    (room, edition, changes)
                };

                let mut authz = TestAuthz::new();
                let room_id = room.id().to_string();

                let object = vec!["rooms", &room_id];

                authz.allow(agent.account_id(), object, "update");

                let context = TestContext::new(db, authz);

                let payload = ListRequest {
                    id: edition.id(),
                    last_created_at: None,
                    limit: None,
                };

                let messages = handle_request::<ListHandler>(&context, &agent, payload)
                    .await
                    .expect("Failed to list changes");

                let (response_changes, respp) = find_response::<Vec<Change>>(messages.as_slice());
                assert_eq!(respp.status(), ResponseStatus::OK);
                assert_eq!(response_changes.len(), 25);
                assert_eq!(response_changes[0].id(), changes[0].id());
            });
        }

        #[test]
        fn list_changes_not_authorized() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (_room, edition, _changes) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);
                    let edition = shared_helpers::insert_edition(&conn, &room, &agent.agent_id());

                    let changes = (1..35)
                        .map(|idx| {
                            let event = factory::Event::new()
                                .room_id(room.id())
                                .kind("message")
                                .data(&json!({ "text": format!("message {}", idx) }))
                                .occurred_at(idx * 1000)
                                .created_by(&agent.agent_id())
                                .insert(&conn);

                            factory::Change::new(edition.id(), ChangeType::Modification)
                                .event_id(event.id())
                                .event_data(json![{"key": "value"}])
                                .insert(&conn)
                        })
                        .collect::<Vec<Change>>();

                    (room, edition, changes)
                };

                let context = TestContext::new(db, TestAuthz::new());

                let payload = ListRequest {
                    id: edition.id(),
                    last_created_at: None,
                    limit: None,
                };

                let resp = handle_request::<ListHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success without authorization on changes list");

                assert_eq!(resp.status_code(), ResponseStatus::FORBIDDEN);
            });
        }

        #[test]
        fn list_changes_missing_edition() {
            futures::executor::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let context = TestContext::new(TestDb::new(), TestAuthz::new());

                let payload = ListRequest {
                    id: Uuid::new_v4(),
                    last_created_at: None,
                    limit: None,
                };

                let resp = handle_request::<ListHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success listing changes for no edition");

                assert_eq!(resp.status_code(), ResponseStatus::NOT_FOUND);
            });
        }
    }

    mod delete {
        use super::super::*;
        use crate::db::change::{ChangeType, Object as Change};
        use crate::test_helpers::prelude::*;
        use serde_json::json;

        #[test]
        fn delete_change() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (room, edition, changes) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);
                    let edition = shared_helpers::insert_edition(&conn, &room, &agent.agent_id());

                    let changes = (1..15)
                        .map(|idx| {
                            let event = factory::Event::new()
                                .room_id(room.id())
                                .kind("message")
                                .data(&json!({ "text": format!("message {}", idx) }))
                                .occurred_at(idx * 1000)
                                .created_by(&agent.agent_id())
                                .insert(&conn);

                            factory::Change::new(edition.id(), ChangeType::Modification)
                                .event_id(event.id())
                                .event_data(json![{"key": "value"}])
                                .insert(&conn)
                        })
                        .collect::<Vec<Change>>();

                    (room, edition, changes)
                };

                let mut authz = TestAuthz::new();
                let room_id = room.id().to_string();

                let object = vec!["rooms", &room_id];

                authz.allow(agent.account_id(), object, "update");

                let context = TestContext::new(db, authz);

                let payload = DeleteRequest {
                    id: changes[0].id(),
                };

                let messages = handle_request::<DeleteHandler>(&context, &agent, payload)
                    .await
                    .expect("Failed to list editions");

                let (_, resp) = find_response::<Change>(messages.as_slice());

                assert_eq!(resp.status(), ResponseStatus::OK);
                let db_changes = db::change::ListQuery::new(edition.id())
                    .execute(&context.db().get().expect("Failed to get DB connection"))
                    .expect("Couldnt load changes from db");
                assert_eq!(db_changes.len(), changes.len() - 1);
            });
        }

        #[test]
        fn delete_change_not_authorized() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (_room, edition, changes) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);
                    let edition = shared_helpers::insert_edition(&conn, &room, &agent.agent_id());

                    let changes = (1..15)
                        .map(|idx| {
                            let event = factory::Event::new()
                                .room_id(room.id())
                                .kind("message")
                                .data(&json!({ "text": format!("message {}", idx) }))
                                .occurred_at(idx * 1000)
                                .created_by(&agent.agent_id())
                                .insert(&conn);

                            factory::Change::new(edition.id(), ChangeType::Modification)
                                .event_id(event.id())
                                .event_data(json![{"key": "value"}])
                                .insert(&conn)
                        })
                        .collect::<Vec<Change>>();

                    (room, edition, changes)
                };

                let context = TestContext::new(db, TestAuthz::new());

                let payload = DeleteRequest {
                    id: changes[0].id(),
                };

                let response = handle_request::<DeleteHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success deleting change without authorization");

                assert_eq!(response.status_code(), ResponseStatus::FORBIDDEN);

                let db_changes = db::change::ListQuery::new(edition.id())
                    .execute(&context.db().get().expect("Failed to get DB connection"))
                    .expect("Couldnt load changes from db");
                assert_eq!(db_changes.len(), changes.len());
            });
        }

        #[test]
        fn delete_change_with_wrong_uuid() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (room, edition, changes) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);
                    let edition = shared_helpers::insert_edition(&conn, &room, &agent.agent_id());

                    let changes = (1..15)
                        .map(|idx| {
                            let event = factory::Event::new()
                                .room_id(room.id())
                                .kind("message")
                                .data(&json!({ "text": format!("message {}", idx) }))
                                .occurred_at(idx * 1000)
                                .created_by(&agent.agent_id())
                                .insert(&conn);

                            factory::Change::new(edition.id(), ChangeType::Modification)
                                .event_id(event.id())
                                .event_data(json![{"key": "value"}])
                                .insert(&conn)
                        })
                        .collect::<Vec<Change>>();

                    (room, edition, changes)
                };

                let mut authz = TestAuthz::new();
                let room_id = room.id().to_string();

                let object = vec!["rooms", &room_id];

                authz.allow(agent.account_id(), object, "update");

                let context = TestContext::new(db, authz);

                let payload = DeleteRequest { id: Uuid::new_v4() };

                let resp = handle_request::<DeleteHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Failed to list changes");

                assert_eq!(resp.status_code(), ResponseStatus::NOT_FOUND);
                let db_changes = db::change::ListQuery::new(edition.id())
                    .execute(&context.db().get().expect("Failed to get DB connection"))
                    .expect("Couldnt load changes from db");
                assert_eq!(db_changes.len(), changes.len());
            });
        }
    }
}

mod create_request;
