use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::{
    extract::{Extension, Path},
    Json,
};
use chrono::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use svc_agent::{
    mqtt::{
        IntoPublishableMessage, OutgoingEvent, OutgoingEventProperties, ResponseStatus,
        ShortTermTimingProperties,
    },
    Addressable,
};
use svc_authn::Authenticable;
use svc_error::Error as SvcError;
use svc_utils::extractors::AuthnExtractor;
use tracing::{error, field::display, instrument, Span};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::app::operations::commit_edition;
use crate::db;
use crate::db::adjustment::Segments;

////////////////////////////////////////////////////////////////////////////////

pub(crate) struct CreateHandler;

#[derive(Debug, Deserialize)]
pub(crate) struct CreateRequest {
    room_id: Uuid,
}

pub async fn create(
    Extension(ctx): Extension<Arc<AppContext>>,
    AuthnExtractor(agent_id): AuthnExtractor,
    Path(room_id): Path<Uuid>,
) -> RequestResult {
    let request = CreateRequest { room_id };
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
            room_id = %payload.room_id,
            scope, classroom_id, edition_id
        )
    )]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let room =
            helpers::find_room(context, payload.room_id, helpers::RoomTimeRequirement::Any).await?;

        let object = {
            let object = room.authz_object();
            let object = object.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
            AuthzObject::new(&object).into()
        };

        let authz_time = context
            .authz()
            .authorize(
                room.audience().to_owned(),
                reqp.as_account_id().to_owned(),
                object,
                "update".into(),
            )
            .await?;

        let edition = {
            let query = db::edition::InsertQuery::new(payload.room_id, reqp.as_agent_id());
            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::EditionInsertQuery, query.execute(&mut conn))
                .await
                .context("Failed to insert edition")
                .error(AppErrorKind::DbQueryFailed)?
        };

        Span::current().record("edition_id", &display(edition.id()));

        let mut response = AppResponse::new(
            ResponseStatus::CREATED,
            edition.clone(),
            context.start_timestamp(),
            Some(authz_time),
        );

        response.add_notification(
            "edition.create",
            &format!("rooms/{}/editions", payload.room_id),
            edition,
            context.start_timestamp(),
        );

        Ok(response)
    }
}

////////////////////////////////////////////////////////////////////////////////

pub(crate) struct ListHandler;

#[derive(Debug, Deserialize)]
pub struct ListPayload {
    last_created_at: Option<DateTime<Utc>>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ListRequest {
    room_id: Uuid,
    #[serde(flatten)]
    payload: ListPayload,
}

pub async fn list(
    Extension(ctx): Extension<Arc<AppContext>>,
    AuthnExtractor(agent_id): AuthnExtractor,
    Path(room_id): Path<Uuid>,
    Json(payload): Json<ListPayload>,
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

#[async_trait]
impl RequestHandler for ListHandler {
    type Payload = ListRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<C: Context>(
        context: &mut C,
        Self::Payload { room_id, payload }: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let room = helpers::find_room(context, room_id, helpers::RoomTimeRequirement::Any).await?;

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

        let mut query = db::edition::ListQuery::new(room.id());

        if let Some(last_created_at) = payload.last_created_at {
            query = query.last_created_at(last_created_at);
        }

        if let Some(limit) = payload.limit {
            query = query.limit(limit);
        }

        let editions = {
            let mut conn = context.get_ro_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::EditionListQuery, query.execute(&mut conn))
                .await
                .context("Failed to list editions")
                .error(AppErrorKind::DbQueryFailed)?
        };

        // Respond with events list.
        Ok(AppResponse::new(
            ResponseStatus::OK,
            editions,
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
            edition_id = %payload.id,
            room_id, scope, classroom_id
        )
    )]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let (edition, room) = {
            let query = db::edition::FindWithRoomQuery::new(payload.id);
            let mut conn = context.get_ro_conn().await?;

            let maybe_edition = context
                .metrics()
                .measure_query(QueryKey::EditionFindWithRoomQuery, query.execute(&mut conn))
                .await
                .context("Failed to find edition with room")
                .error(AppErrorKind::DbQueryFailed)?;

            match maybe_edition {
                Some(edition_with_room) => edition_with_room,
                None => {
                    return Err(anyhow!("Edition not found")).error(AppErrorKind::EditionNotFound);
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

        {
            let query = db::edition::DeleteQuery::new(edition.id());
            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::EditionDeleteQuery, query.execute(&mut conn))
                .await
                .context("Failed to delete edition")
                .error(AppErrorKind::DbQueryFailed)?;
        }

        Ok(AppResponse::new(
            ResponseStatus::OK,
            edition,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}

////////////////////////////////////////////////////////////////////////////////

pub(crate) struct CommitHandler;

#[derive(Debug, Deserialize)]
pub(crate) struct CommitRequest {
    id: Uuid,
}

pub async fn commit(
    Extension(ctx): Extension<Arc<AppContext>>,
    AuthnExtractor(agent_id): AuthnExtractor,
    Path(id): Path<Uuid>,
) -> RequestResult {
    let request = CommitRequest { id };
    CommitHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

#[async_trait]
impl RequestHandler for CommitHandler {
    type Payload = CommitRequest;

    #[instrument(
        skip_all,
        fields(
            edition_id = %payload.id,
            room_id, scope, classroom_id
        )
    )]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        // Find edition with its source room.
        let (edition, room) = {
            let query = db::edition::FindWithRoomQuery::new(payload.id);
            let mut conn = context.get_ro_conn().await?;

            let maybe_edition = context
                .metrics()
                .measure_query(QueryKey::EditionFindWithRoomQuery, query.execute(&mut conn))
                .await
                .context("Failed to find edition with room")
                .error(AppErrorKind::DbQueryFailed)?;

            match maybe_edition {
                Some(edition_with_room) => edition_with_room,
                None => {
                    return Err(anyhow!("Edition not found")).error(AppErrorKind::EditionNotFound);
                }
            }
        };

        helpers::add_room_logger_tags(&room);

        // Authorize room update.
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

        // Run commit task asynchronously.
        let db = context.db().to_owned();
        let metrics = context.metrics();

        let notification_future = tokio::task::spawn(async move {
            let result = commit_edition(&db, &metrics, &edition, &room).await;

            // Handle result.
            let result = match result {
                Ok((destination, modified_segments)) => EditionCommitResult::Success {
                    source_room_id: edition.source_room_id(),
                    committed_room_id: destination.id(),
                    modified_segments,
                },
                Err(err) => {
                    error!("Room adjustment job failed: {:?}", err);
                    let app_error = AppError::new(AppErrorKind::EditionCommitTaskFailed, err);
                    app_error.notify_sentry();
                    EditionCommitResult::Error {
                        error: app_error.to_svc_error(),
                    }
                }
            };

            // Publish success/failure notification.
            let notification = EditionCommitNotification {
                status: result.status(),
                tags: room.tags().map(|t| t.to_owned()),
                result,
            };

            let timing = ShortTermTimingProperties::new(Utc::now());
            let props = OutgoingEventProperties::new("edition.commit", timing);
            let path = format!("audiences/{}/events", room.audience());
            let event = OutgoingEvent::broadcast(notification, props, &path);

            Box::new(event) as Box<dyn IntoPublishableMessage + Send + Sync + 'static>
        });

        // Respond with 202.
        // The actual task result will be broadcasted to the room topic when finished.
        let mut response = AppResponse::new(
            ResponseStatus::ACCEPTED,
            json!({}),
            context.start_timestamp(),
            Some(authz_time),
        );

        response.add_async_task(notification_future);

        Ok(response)
    }
}

#[derive(Serialize)]
struct EditionCommitNotification {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<JsonValue>,
    #[serde(flatten)]
    result: EditionCommitResult,
}

#[derive(Serialize)]
#[serde(untagged)]
enum EditionCommitResult {
    Success {
        source_room_id: Uuid,
        committed_room_id: Uuid,
        #[serde(with = "crate::db::adjustment::serde::segments")]
        modified_segments: Segments,
    },
    Error {
        error: SvcError,
    },
}

impl EditionCommitResult {
    fn status(&self) -> &'static str {
        match self {
            Self::Success { .. } => "success",
            Self::Error { .. } => "error",
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    mod create {
        use super::super::*;
        use crate::db::edition::Object as Edition;
        use crate::test_helpers::prelude::*;

        #[tokio::test]
        async fn create_edition() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                let mut conn = db.get_conn().await;
                shared_helpers::insert_room(&mut conn).await
            };

            // Allow agent to create editions
            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let object = vec!["rooms", &room_id];
            authz.allow(agent.account_id(), object, "update");

            // Make edition.create request
            let mut context = TestContext::new(db, authz);
            let payload = CreateRequest { room_id: room.id() };

            let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect("Failed to create edition");

            // Assert response
            let (edition, respp, _) = find_response::<Edition>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::CREATED);
            assert_eq!(edition.source_room_id(), room.id());
        }

        #[tokio::test]
        async fn create_edition_not_authorized() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let room = {
                let mut conn = db.get_conn().await;
                shared_helpers::insert_room(&mut conn).await
            };

            let mut context = TestContext::new(db, TestAuthz::new());
            let payload = CreateRequest { room_id: room.id() };

            let response = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success creating edition with no authorization");

            assert_eq!(response.status(), ResponseStatus::FORBIDDEN);
        }

        #[tokio::test]
        async fn create_edition_missing_room() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

            let payload = CreateRequest {
                room_id: Uuid::new_v4(),
            };

            let err = handle_request::<CreateHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success creating edition for no room");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_not_found");
        }
    }

    mod list {
        use super::super::*;
        use crate::db::edition::Object as Edition;
        use crate::test_helpers::prelude::*;

        #[tokio::test]
        async fn list_editions() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, editions) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                let edition = factory::Edition::new(room.id(), agent.agent_id())
                    .insert(&mut conn)
                    .await;

                (room, vec![edition])
            };

            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let object = vec!["rooms", &room_id];
            authz.allow(agent.account_id(), object, "update");

            let mut context = TestContext::new(db, authz);

            let payload = ListRequest {
                room_id: room.id(),
                payload: ListPayload {
                    last_created_at: None,
                    limit: None,
                },
            };

            let messages = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect("Failed to list editions");

            let (resp_editions, respp, _) = find_response::<Vec<Edition>>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::OK);
            assert_eq!(resp_editions.len(), editions.len());
            assert_eq!(resp_editions[0].id(), editions[0].id());
        }

        #[tokio::test]
        async fn list_editions_not_authorized() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, _editions) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                let edition = factory::Edition::new(room.id(), agent.agent_id())
                    .insert(&mut conn)
                    .await;

                (room, vec![edition])
            };

            let mut context = TestContext::new(db, TestAuthz::new());

            let payload = ListRequest {
                room_id: room.id(),
                payload: ListPayload {
                    last_created_at: None,
                    limit: None,
                },
            };

            let resp = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success without authorization on editions list");

            assert_eq!(resp.status(), ResponseStatus::FORBIDDEN);
        }

        #[tokio::test]
        async fn list_editions_missing_room() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

            let payload = ListRequest {
                room_id: Uuid::new_v4(),
                payload: ListPayload {
                    last_created_at: None,
                    limit: None,
                },
            };

            let err = handle_request::<ListHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success listing editions for no room");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "room_not_found");
        }
    }

    mod delete {
        use super::super::*;
        use crate::db::edition::Object as Edition;
        use crate::test_helpers::prelude::*;

        #[tokio::test]
        async fn delete_edition() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, editions) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;
                let mut editions = vec![];

                for _ in 1..4 {
                    let edition = factory::Edition::new(room.id(), agent.agent_id())
                        .insert(&mut conn)
                        .await;

                    editions.push(edition);
                }

                (room, editions)
            };

            let mut authz = TestAuthz::new();
            let room_id = room.id().to_string();
            let object = vec!["rooms", &room_id];
            authz.allow(agent.account_id(), object, "update");

            let mut context = TestContext::new(db, authz);

            let payload = DeleteRequest {
                id: editions[0].id(),
            };

            let messages = handle_request::<DeleteHandler>(&mut context, &agent, payload)
                .await
                .expect("Failed to find deleted edition");

            let (resp_edition, resp, _) = find_response::<Edition>(messages.as_slice());
            assert_eq!(resp.status(), ResponseStatus::OK);
            assert_eq!(resp_edition.id(), editions[0].id());

            let mut conn = context
                .db()
                .acquire()
                .await
                .expect("Failed to get DB connection");

            let db_editions = db::edition::ListQuery::new(room.id())
                .execute(&mut conn)
                .await
                .expect("Failed to fetch editions");

            assert_eq!(db_editions.len(), editions.len() - 1);
        }

        #[tokio::test]
        async fn delete_edition_not_authorized() {
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let (room, editions) = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;
                let mut editions = vec![];

                for _ in 1..4 {
                    let edition = factory::Edition::new(room.id(), agent.agent_id())
                        .insert(&mut conn)
                        .await;

                    editions.push(edition)
                }

                (room, editions)
            };

            let mut context = TestContext::new(db, TestAuthz::new());

            let payload = DeleteRequest {
                id: editions[0].id(),
            };

            let resp = handle_request::<DeleteHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success without authorization on editions list");

            assert_eq!(resp.status(), ResponseStatus::FORBIDDEN);

            let mut conn = context
                .db()
                .acquire()
                .await
                .expect("Failed to get DB connection");

            let db_editions = db::edition::ListQuery::new(room.id())
                .execute(&mut conn)
                .await
                .expect("Failed to fetch editions");

            assert_eq!(db_editions.len(), editions.len());
        }

        #[tokio::test]
        async fn delete_editions_missing_room() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());
            let payload = DeleteRequest { id: Uuid::new_v4() };

            let err = handle_request::<DeleteHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success listing editions for no room");

            assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
            assert_eq!(err.kind(), "edition_not_found");
        }
    }
}
