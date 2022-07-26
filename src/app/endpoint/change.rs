use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::{
    extract::{Extension, Path, Query},
    Json,
};
use chrono::{DateTime, Utc};
use serde_derive::Deserialize;
use svc_agent::mqtt::ResponseStatus;
use svc_authn::Authenticable;
use svc_utils::extractors::AuthnExtractor;
use tracing::{field::display, instrument, Span};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::change::create_request::{Changeset, CreateRequest};
use crate::app::endpoint::prelude::*;
use crate::db;

////////////////////////////////////////////////////////////////////////////////

pub(crate) struct CreateHandler;

pub async fn create(
    Extension(ctx): Extension<Arc<AppContext>>,
    AuthnExtractor(agent_id): AuthnExtractor,
    Path(edition_id): Path<Uuid>,
    Json(changeset): Json<Changeset>,
) -> RequestResult {
    let request = CreateRequest {
        edition_id,
        changeset,
    };
    CreateHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

#[async_trait]
impl RequestHandler for CreateHandler {
    type Payload = CreateRequest;

    #[instrument(
        skip_all,
        fields(
            edition_id = %payload.edition_id,
            scope, room_id, classroom_id, change_id
        )
    )]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let (_edition, room) = {
            let query = db::edition::FindWithRoomQuery::new(payload.edition_id);
            let mut conn = context.get_ro_conn().await?;

            let maybe_edition_with_room = context
                .metrics()
                .measure_query(QueryKey::EditionFindWithRoomQuery, query.execute(&mut conn))
                .await
                .context("Failed to find edition with room")
                .error(AppErrorKind::DbQueryFailed)?;

            match maybe_edition_with_room {
                Some(edition_with_room) => edition_with_room,
                None => {
                    return Err(anyhow!("Edition not found"))
                        .error(AppErrorKind::EditionNotFound)?
                }
            }
        };

        helpers::add_room_logger_tags(&room);

        let object = AuthzObject::room(&room).into();

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "update".into(),
            )
            .await?;

        let query =
            db::change::InsertQuery::new(payload.edition_id, payload.changeset.as_changetype());

        let query = match payload.changeset {
            Changeset::Addition(event) => query
                .event_kind(event.kind)
                .event_set(event.set)
                .event_label(event.label)
                .event_data(event.data)
                .event_occurred_at(event.occurred_at)
                .event_created_by(event.created_by),
            Changeset::Modification(event) => {
                let query = query.event_id(event.event_id);

                let query = match event.kind {
                    Some(kind) => query.event_kind(kind),
                    None => query,
                };

                let query = query.event_set(event.set);
                let query = query.event_label(event.label);

                let query = match event.data {
                    Some(data) => query.event_data(data),
                    None => query,
                };

                let query = match event.occurred_at {
                    Some(v) => query.event_occurred_at(v),
                    None => query,
                };

                match event.created_by {
                    Some(agent_id) => query.event_created_by(agent_id),
                    None => query,
                }
            }
            Changeset::Removal(event) => query.event_id(event.event_id),
        };

        let change = {
            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::ChangeInsertQuery, query.execute(&mut conn))
                .await
                .context("Failed to insert change")
                .error(AppErrorKind::DbQueryFailed)?
        };

        Span::current().record("change_id", &display(change.id()));

        Ok(AppResponse::new(
            ResponseStatus::CREATED,
            change,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}

////////////////////////////////////////////////////////////////////////////////

pub(crate) struct ListHandler;

#[derive(Debug, Deserialize)]
pub struct ListPayload {
    last_created_at: Option<DateTime<Utc>>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ListRequest {
    id: Uuid,
    #[serde(flatten)]
    payload: ListPayload,
}

pub async fn list(
    Extension(ctx): Extension<Arc<AppContext>>,
    AuthnExtractor(agent_id): AuthnExtractor,
    Path(id): Path<Uuid>,
    Query(payload): Query<ListPayload>,
) -> RequestResult {
    let request = ListRequest { id, payload };
    ListHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

#[async_trait]
impl RequestHandler for ListHandler {
    type Payload = ListRequest;

    #[instrument(skip_all, fields(edition_id, scope, room_id, classroom_id, change_id))]
    async fn handle<C: Context>(
        context: &mut C,
        Self::Payload { id, payload }: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        Span::current().record("edition_id", &display(id));
        let (edition, room) = {
            let query = db::edition::FindWithRoomQuery::new(id);
            let mut conn = context.get_ro_conn().await?;

            let maybe_edition_with_room = context
                .metrics()
                .measure_query(QueryKey::EditionFindWithRoomQuery, query.execute(&mut conn))
                .await
                .context("Failed to find edition")
                .error(AppErrorKind::DbQueryFailed)?;

            match maybe_edition_with_room {
                Some(edition_with_room) => edition_with_room,
                None => {
                    return Err(anyhow!("Edition not found"))
                        .error(AppErrorKind::EditionNotFound)?;
                }
            }
        };

        helpers::add_room_logger_tags(&room);

        let object = AuthzObject::room(&room).into();

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "update".into(),
            )
            .await?;

        let mut query = db::change::ListQuery::new(edition.id());

        if let Some(last_created_at) = payload.last_created_at {
            query = query.last_created_at(last_created_at);
        }

        if let Some(limit) = payload.limit {
            query = query.limit(limit);
        }

        let changes = {
            let mut conn = context.get_ro_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::ChangeListQuery, query.execute(&mut conn))
                .await
                .context("Failed to list changes")
                .error(AppErrorKind::DbQueryFailed)?
        };

        Ok(AppResponse::new(
            ResponseStatus::OK,
            changes,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}

////////////////////////////////////////////////////////////////////////////////

pub(crate) struct DeleteHandler;

#[derive(Debug, Deserialize)]
pub(crate) struct DeleteRequest {
    id: Uuid,
}

pub async fn delete(
    Extension(ctx): Extension<Arc<AppContext>>,
    AuthnExtractor(agent_id): AuthnExtractor,
    Path(id): Path<Uuid>,
) -> RequestResult {
    let request = DeleteRequest { id };
    DeleteHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

#[async_trait]
impl RequestHandler for DeleteHandler {
    type Payload = DeleteRequest;

    #[instrument(
        skip_all,
        fields(
            change_id = %payload.id,
            scope, room_id, classroom_id, edition_id
        )
    )]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let (change, room) = {
            let query = db::change::FindWithRoomQuery::new(payload.id);
            let mut conn = context.get_ro_conn().await?;

            let maybe_change_with_room = context
                .metrics()
                .measure_query(QueryKey::ChangeFindWithRoomQuery, query.execute(&mut conn))
                .await
                .context("Failed to find change with room")
                .error(AppErrorKind::DbQueryFailed)?;

            match maybe_change_with_room {
                Some(change_with_room) => change_with_room,
                None => {
                    return Err(anyhow!("Change not found")).error(AppErrorKind::ChangeNotFound)?;
                }
            }
        };

        helpers::add_room_logger_tags(&room);
        Span::current().record("edition_id", &display(change.edition_id()));

        let object = AuthzObject::room(&room).into();

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "update".into(),
            )
            .await?;

        {
            let query = db::change::DeleteQuery::new(change.id());
            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::ChangeDeleteQuery, query.execute(&mut conn))
                .await
                .context("Failed to delete change")
                .error(AppErrorKind::DbQueryFailed)?;
        }

        Ok(AppResponse::new(
            ResponseStatus::OK,
            change,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    mod create {
        use serde_json::json;

        use crate::app::endpoint::change::create_request::{
            AdditionData, Changeset, CreateRequest, ModificationData, RemovalData,
        };
        use crate::db::change::{ChangeType, Object as Change};
        use crate::test_helpers::prelude::*;

        use super::super::*;

        #[tokio::test]
        async fn create_addition_change() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, edition) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                let edition =
                    shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

                (room, edition)
            };

            // Allow agent to create editions
            let mut authz = TestAuthz::new();
            authz.allow(
                agent.account_id(),
                vec!["classrooms", &room.classroom_id().to_string()],
                "update",
            );

            // Make edition.create request
            let mut context = TestContext::new(db, authz);

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

            let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect("Failed to create change");

            // Assert response
            let (change, respp, _) = find_response::<Change>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::CREATED);
            assert_eq!(change.edition_id(), edition.id());
            assert_eq!(change.kind(), ChangeType::Addition);
            assert_eq!(
                change
                    .event_data()
                    .as_ref()
                    .expect("Couldn't get event data from Change"),
                &json![{"key": "value"}]
            );
        }

        #[tokio::test]
        async fn create_removal_change() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, edition, events) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                let edition =
                    shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

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

                (room, edition, events)
            };

            // Allow agent to create editions
            let mut authz = TestAuthz::new();
            authz.allow(
                agent.account_id(),
                vec!["classrooms", &room.classroom_id().to_string()],
                "update",
            );

            // Make edition.create request
            let mut context = TestContext::new(db, authz);

            let payload = CreateRequest {
                edition_id: edition.id(),
                changeset: Changeset::Removal(RemovalData {
                    event_id: events[0].id(),
                    kind: None,
                    occurred_at: None,
                    set: None,
                }),
            };

            let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect("Failed to create change");

            // Assert response
            let (change, respp, _) = find_response::<Change>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::CREATED);
            assert_eq!(change.edition_id(), edition.id());
            assert_eq!(change.kind(), ChangeType::Removal);
            assert_eq!(change.event_id().unwrap(), events[0].id());
        }

        #[tokio::test]
        async fn create_modification_change() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, edition, events) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                let edition =
                    shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

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

                (room, edition, events)
            };

            // Allow agent to create editions
            let mut authz = TestAuthz::new();
            authz.allow(
                agent.account_id(),
                vec!["classrooms", &room.classroom_id().to_string()],
                "update",
            );

            // Make edition.create request
            let mut context = TestContext::new(db, authz);

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

            let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect("Failed to create change");

            // Assert response
            let (change, respp, _) = find_response::<Change>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::CREATED);
            assert_eq!(change.edition_id(), edition.id());
            assert_eq!(change.kind(), ChangeType::Modification);
            assert_eq!(change.event_id().unwrap(), events[0].id());
            assert_eq!(
                change
                    .event_data()
                    .as_ref()
                    .expect("Couldn't get event data from ChangeWithChangeEvent"),
                &json![{"key": "value"}]
            );
        }

        #[tokio::test]
        async fn create_change_with_improper_event_id() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, edition) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                let edition =
                    shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

                (room, edition)
            };

            // Allow agent to create editions
            let mut authz = TestAuthz::new();
            authz.allow(
                agent.account_id(),
                vec!["classrooms", &room.classroom_id().to_string()],
                "update",
            );

            // Make edition.create request
            let mut context = TestContext::new(db, authz);

            let payload = CreateRequest {
                edition_id: edition.id(),
                changeset: Changeset::Removal(RemovalData {
                    event_id: Uuid::new_v4(),
                    kind: None,
                    occurred_at: None,
                    set: None,
                }),
            };

            let err = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success creating change with wrong params");

            assert_eq!(err.status(), ResponseStatus::UNPROCESSABLE_ENTITY);
            assert_eq!(err.kind(), "database_query_failed");
        }

        #[tokio::test]
        async fn create_change_not_authorized() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (_room, edition) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                let edition =
                    shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

                (room, edition)
            };

            let mut context = TestContext::new(db, TestAuthz::new());

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

            let response = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success creating change with no authorization");

            assert_eq!(response.status(), ResponseStatus::FORBIDDEN);
        }

        #[tokio::test]
        async fn create_change_missing_edition() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

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

            let err = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success creating change for no edition");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "edition_not_found");
        }
    }

    mod list {
        use serde_json::json;

        use super::super::*;
        use crate::db::change::{ChangeType, Object as Change};
        use crate::test_helpers::prelude::*;

        #[tokio::test]
        async fn list_changes() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, edition, changes) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                let edition =
                    shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

                let mut changes = vec![];

                for idx in 1..35 {
                    let event = factory::Event::new()
                        .room_id(room.id())
                        .kind("message")
                        .data(&json!({ "text": format!("message {}", idx) }))
                        .occurred_at(idx * 1000)
                        .created_by(&agent.agent_id())
                        .insert(&mut conn)
                        .await;

                    let change = factory::Change::new(edition.id(), ChangeType::Modification)
                        .event_id(event.id())
                        .event_data(json![{"key": "value"}])
                        .insert(&mut conn)
                        .await;

                    changes.push(change);
                }

                (room, edition, changes)
            };

            let mut authz = TestAuthz::new();
            authz.allow(
                agent.account_id(),
                vec!["classrooms", &room.classroom_id().to_string()],
                "update",
            );

            let mut context = TestContext::new(db, authz);

            let payload = ListRequest {
                id: edition.id(),
                payload: ListPayload {
                    last_created_at: None,
                    limit: None,
                },
            };

            let messages = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect("Failed to list changes");

            let (response_changes, respp, _) = find_response::<Vec<Change>>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(response_changes.len(), 25);

            let ids = changes.into_iter().map(|c| c.id()).collect::<Vec<Uuid>>();

            assert!(ids.contains(&response_changes[0].id()));
        }

        #[tokio::test]
        async fn list_changes_not_authorized() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (_room, edition, _changes) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                let edition =
                    shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

                let mut changes = vec![];

                for idx in 1..35 {
                    let event = factory::Event::new()
                        .room_id(room.id())
                        .kind("message")
                        .data(&json!({ "text": format!("message {}", idx) }))
                        .occurred_at(idx * 1000)
                        .created_by(&agent.agent_id())
                        .insert(&mut conn)
                        .await;

                    let change = factory::Change::new(edition.id(), ChangeType::Modification)
                        .event_id(event.id())
                        .event_data(json![{"key": "value"}])
                        .insert(&mut conn)
                        .await;

                    changes.push(change);
                }

                (room, edition, changes)
            };

            let mut context = TestContext::new(db, TestAuthz::new());

            let payload = ListRequest {
                id: edition.id(),
                payload: ListPayload {
                    last_created_at: None,
                    limit: None,
                },
            };

            let resp = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success without authorization on changes list");

            assert_eq!(resp.status(), ResponseStatus::FORBIDDEN);
        }

        #[tokio::test]
        async fn list_changes_missing_edition() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

            let payload = ListRequest {
                id: Uuid::new_v4(),
                payload: ListPayload {
                    last_created_at: None,
                    limit: None,
                },
            };

            let err = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success listing changes for no edition");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "edition_not_found");
        }
    }

    mod delete {
        use super::super::*;
        use crate::db::change::{ChangeType, Object as Change};
        use crate::test_helpers::prelude::*;
        use serde_json::json;

        #[tokio::test]
        async fn delete_change() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, edition, changes) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                let edition =
                    shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

                let mut changes = vec![];

                for idx in 1..15 {
                    let event = factory::Event::new()
                        .room_id(room.id())
                        .kind("message")
                        .data(&json!({ "text": format!("message {}", idx) }))
                        .occurred_at(idx * 1000)
                        .created_by(&agent.agent_id())
                        .insert(&mut conn)
                        .await;

                    let change = factory::Change::new(edition.id(), ChangeType::Modification)
                        .event_id(event.id())
                        .event_data(json![{"key": "value"}])
                        .insert(&mut conn)
                        .await;

                    changes.push(change);
                }

                (room, edition, changes)
            };

            let mut authz = TestAuthz::new();
            authz.allow(
                agent.account_id(),
                vec!["classrooms", &room.classroom_id().to_string()],
                "update",
            );

            let mut context = TestContext::new(db, authz);

            let payload = DeleteRequest {
                id: changes[0].id(),
            };

            let messages = handle_request::<DeleteHandler>(&mut context, &agent, payload)
                .await
                .expect("Failed to list editions");

            let (_, resp, _) = find_response::<Change>(messages.as_slice());

            assert_eq!(resp.status(), ResponseStatus::OK);

            let mut conn = context
                .db()
                .acquire()
                .await
                .expect("Failed to get DB connection");

            let db_changes = db::change::ListQuery::new(edition.id())
                .execute(&mut conn)
                .await
                .expect("Couldn't load changes from db");

            assert_eq!(db_changes.len(), changes.len() - 1);
        }

        #[tokio::test]
        async fn delete_change_not_authorized() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (_room, edition, changes) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                let edition =
                    shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

                let mut changes = vec![];

                for idx in 1..15 {
                    let event = factory::Event::new()
                        .room_id(room.id())
                        .kind("message")
                        .data(&json!({ "text": format!("message {}", idx) }))
                        .occurred_at(idx * 1000)
                        .created_by(&agent.agent_id())
                        .insert(&mut conn)
                        .await;

                    let change = factory::Change::new(edition.id(), ChangeType::Modification)
                        .event_id(event.id())
                        .event_data(json![{"key": "value"}])
                        .insert(&mut conn)
                        .await;

                    changes.push(change);
                }

                (room, edition, changes)
            };

            let mut context = TestContext::new(db, TestAuthz::new());

            let payload = DeleteRequest {
                id: changes[0].id(),
            };

            let response = handle_request::<DeleteHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success deleting change without authorization");

            assert_eq!(response.status(), ResponseStatus::FORBIDDEN);

            let mut conn = context
                .db()
                .acquire()
                .await
                .expect("Failed to get DB connection");

            let db_changes = db::change::ListQuery::new(edition.id())
                .execute(&mut conn)
                .await
                .expect("Couldn't load changes from db");

            assert_eq!(db_changes.len(), changes.len());
        }

        #[tokio::test]
        async fn delete_change_with_wrong_uuid() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, edition, changes) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                let edition =
                    shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

                let mut changes = vec![];

                for idx in 1..15 {
                    let event = factory::Event::new()
                        .room_id(room.id())
                        .kind("message")
                        .data(&json!({ "text": format!("message {}", idx) }))
                        .occurred_at(idx * 1000)
                        .created_by(&agent.agent_id())
                        .insert(&mut conn)
                        .await;

                    let change = factory::Change::new(edition.id(), ChangeType::Modification)
                        .event_id(event.id())
                        .event_data(json![{"key": "value"}])
                        .insert(&mut conn)
                        .await;

                    changes.push(change);
                }

                (room, edition, changes)
            };

            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let object = vec!["rooms", &room_id];
            authz.allow(agent.account_id(), object, "update");

            let mut context = TestContext::new(db, authz);
            let payload = DeleteRequest { id: Uuid::new_v4() };

            let err = handle_request::<DeleteHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Failed to list changes");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "change_not_found");

            let mut conn = context
                .db()
                .acquire()
                .await
                .expect("Failed to get DB connection");

            let db_changes = db::change::ListQuery::new(edition.id())
                .execute(&mut conn)
                .await
                .expect("Couldn't load changes from db");

            assert_eq!(db_changes.len(), changes.len());
        }
    }
}

mod create_request;
