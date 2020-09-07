use async_std::prelude::*;
use async_std::stream;
use async_std::task::spawn_blocking;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::future::FutureExt;
use log::{error, warn};
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use svc_agent::{
    mqtt::{
        IncomingRequestProperties, IntoPublishableMessage, OutgoingEvent, OutgoingEventProperties,
        ResponseStatus, ShortTermTimingProperties,
    },
    Addressable,
};
use svc_error::{extension::sentry, Error as SvcError};

use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::{metric::ProfilerKeys, prelude::*};
use crate::app::operations::commit_edition;
use crate::db;
use crate::db::adjustment::Segment;

pub(crate) struct CreateHandler;

#[derive(Debug, Deserialize)]
pub(crate) struct CreateRequest {
    room_id: Uuid,
}

#[async_trait]
impl RequestHandler for CreateHandler {
    type Payload = CreateRequest;
    const ERROR_TITLE: &'static str = "Failed to create edition";

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result {
        let room = {
            let query = db::room::FindQuery::new(payload.room_id);
            let conn = context.get_ro_conn().await?;

            context
                .profiler()
                .measure(
                    ProfilerKeys::RoomFindQuery,
                    spawn_blocking(move || query.execute(&conn)),
                )
                .await?
                .ok_or_else(|| format!("Room not found, id = '{}'", payload.room_id))
                .status(ResponseStatus::NOT_FOUND)?
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

        let edition = {
            let query =
                db::edition::InsertQuery::new(payload.room_id, reqp.as_agent_id().to_owned());
            let conn = context.get_conn().await?;

            context
                .profiler()
                .measure(
                    ProfilerKeys::EditionInsertQuery,
                    spawn_blocking(move || query.execute(&conn)),
                )
                .await?
        };

        let response = helpers::build_response(
            ResponseStatus::CREATED,
            edition.clone(),
            reqp,
            start_timestamp,
            Some(authz_time),
        );

        let notification = helpers::build_notification(
            "edition.create",
            &format!("rooms/{}/editions", payload.room_id),
            edition,
            reqp,
            start_timestamp,
        );

        Ok(Box::new(stream::from_iter(vec![response, notification])))
    }
}

pub(crate) struct ListHandler;

#[derive(Debug, Deserialize)]
pub(crate) struct ListRequest {
    room_id: Uuid,
    last_created_at: Option<DateTime<Utc>>,
    limit: Option<i64>,
}

#[async_trait]
impl RequestHandler for ListHandler {
    type Payload = ListRequest;
    const ERROR_TITLE: &'static str = "Failed to list editions";

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result {
        let room = {
            let query = db::room::FindQuery::new(payload.room_id);
            let conn = context.get_ro_conn().await?;

            context
                .profiler()
                .measure(
                    ProfilerKeys::RoomFindQuery,
                    spawn_blocking(move || query.execute(&conn)),
                )
                .await?
                .ok_or_else(|| format!("Room not found, id = '{}'", payload.room_id))
                .status(ResponseStatus::NOT_FOUND)?
        };

        let room_id = room.id();

        let authz_time = context
            .authz()
            .authorize(
                room.audience(),
                reqp,
                vec!["rooms", &room_id.to_string()],
                "update",
            )
            .await?;

        let mut query = db::edition::ListQuery::new(room_id);

        if let Some(last_created_at) = payload.last_created_at {
            query = query.last_created_at(last_created_at);
        }

        if let Some(limit) = payload.limit {
            query = query.limit(limit);
        }

        let editions = {
            let conn = context.get_ro_conn().await?;

            context
                .profiler()
                .measure(
                    ProfilerKeys::EditionListQuery,
                    spawn_blocking(move || query.execute(&conn)),
                )
                .await?
        };

        // Respond with events list.
        Ok(Box::new(stream::once(helpers::build_response(
            ResponseStatus::OK,
            editions,
            reqp,
            start_timestamp,
            Some(authz_time),
        ))))
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
    const ERROR_TITLE: &'static str = "Failed to delete edition";

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result {
        let (edition, room) = {
            let query = db::edition::FindWithRoomQuery::new(payload.id);
            let conn = context.get_ro_conn().await?;

            let maybe_edition_and_room = context
                .profiler()
                .measure(
                    ProfilerKeys::EditionFindWithRoomQuery,
                    spawn_blocking(move || query.execute(&conn)),
                )
                .await?;

            match maybe_edition_and_room {
                Some((edition, room)) => (edition, room),
                None => {
                    return Err(format!("Change not found, id = '{}'", payload.id))
                        .status(ResponseStatus::NOT_FOUND);
                }
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

        {
            let query = db::edition::DeleteQuery::new(edition.id());
            let conn = context.get_conn().await?;

            context
                .profiler()
                .measure(
                    ProfilerKeys::EditionDeleteQuery,
                    spawn_blocking(move || query.execute(&conn)),
                )
                .await?;
        }

        let response = helpers::build_response(
            ResponseStatus::OK,
            edition,
            reqp,
            start_timestamp,
            Some(authz_time),
        );

        Ok(Box::new(stream::from_iter(vec![response])))
    }
}

pub(crate) struct CommitHandler;

#[derive(Debug, Deserialize)]
pub(crate) struct CommitRequest {
    id: Uuid,
}

#[async_trait]
impl RequestHandler for CommitHandler {
    type Payload = CommitRequest;
    const ERROR_TITLE: &'static str = "Failed to commit edition";

    async fn handle<C: Context>(
        context: &C,
        payload: Self::Payload,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result {
        let (edition, room) = {
            let query = db::edition::FindWithRoomQuery::new(payload.id);
            let conn = context.get_ro_conn().await?;

            let maybe_edition_and_room = context
                .profiler()
                .measure(
                    ProfilerKeys::EditionFindWithRoomQuery,
                    spawn_blocking(move || query.execute(&conn)),
                )
                .await?;

            match maybe_edition_and_room {
                Some((edition, room)) => (edition, room),
                None => {
                    return Err(format!("Edition not found, id = '{}'", payload.id))
                        .status(ResponseStatus::NOT_FOUND);
                }
            }
        };

        let room_id = room.id().to_string();
        let object = vec!["rooms", &room_id];

        let authz_time = context
            .authz()
            .authorize(room.audience(), reqp, object, "update")
            .await?;

        let db = context.db().to_owned();

        // Respond with 202.
        // The actual task result will be broadcasted to the room topic when finished.
        let response = stream::once(helpers::build_response(
            ResponseStatus::ACCEPTED,
            json!({}),
            reqp,
            start_timestamp,
            Some(authz_time),
        ));

        let notification_future = async_std::task::spawn_blocking(move || {
            let result = commit_edition(&db, &edition, &room);

            // Handle result.
            let result = match result {
                Ok((destination, modified_segments)) => EditionCommitResult::Success {
                    source_room_id: edition.source_room_id(),
                    committed_room_id: destination.id(),
                    modified_segments,
                },
                Err(err) => {
                    error!(
                        "Room adjustment job failed for room_id = '{}': {}",
                        room.id(),
                        err
                    );

                    let error = SvcError::builder()
                        .status(ResponseStatus::UNPROCESSABLE_ENTITY)
                        .kind("edition.commit", CommitHandler::ERROR_TITLE)
                        .detail(&err.to_string())
                        .build();

                    sentry::send(error.clone())
                        .unwrap_or_else(|err| warn!("Error sending error to Sentry: {}", err));

                    EditionCommitResult::Error { error }
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

            Box::new(event) as Box<dyn IntoPublishableMessage + Send>
        });

        let notification = notification_future.into_stream();
        Ok(Box::new(response.chain(notification)))
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
        #[serde(with = "crate::serde::milliseconds_bound_tuples")]
        modified_segments: Vec<Segment>,
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

#[cfg(test)]
mod tests {
    mod create {
        use super::super::*;
        use crate::db::edition::Object as Edition;
        use crate::test_helpers::prelude::*;

        #[test]
        fn create_edition() {
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

                // Allow agent to create editions
                let mut authz = TestAuthz::new();
                let room_id = room.id().to_string();

                let object = vec!["rooms", &room_id];

                authz.allow(agent.account_id(), object, "update");

                // Make edition.create request
                let context = TestContext::new(db, authz);

                let payload = CreateRequest { room_id: room.id() };

                let messages = handle_request::<CreateHandler>(&context, &agent, payload)
                    .await
                    .expect("Failed to create edition");

                // Assert response
                let (edition, respp) = find_response::<Edition>(messages.as_slice());
                assert_eq!(respp.status(), ResponseStatus::CREATED);
                assert_eq!(edition.source_room_id(), room.id());
            });
        }

        #[test]
        fn create_edition_not_authorized() {
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

                let payload = CreateRequest { room_id: room.id() };

                let response = handle_request::<CreateHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success creating edition with no authorization");

                assert_eq!(response.status_code(), ResponseStatus::FORBIDDEN);
            });
        }
        #[test]
        fn create_edition_missing_room() {
            futures::executor::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let context = TestContext::new(TestDb::new(), TestAuthz::new());

                let payload = CreateRequest {
                    room_id: Uuid::new_v4(),
                };

                let response = handle_request::<CreateHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success creating edition for no room");

                assert_eq!(response.status_code(), ResponseStatus::NOT_FOUND);
            });
        }
    }

    mod list {
        use super::super::*;
        use crate::db::edition::Object as Edition;
        use crate::test_helpers::prelude::*;

        #[test]
        fn list_editions() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (room, editions) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);

                    let editions = (1..2)
                        .map(|_idx| {
                            factory::Edition::new(room.id(), agent.agent_id()).insert(&conn)
                        })
                        .collect::<Vec<Edition>>();

                    (room, editions)
                };

                let mut authz = TestAuthz::new();
                let room_id = room.id().to_string();

                let object = vec!["rooms", &room_id];

                authz.allow(agent.account_id(), object, "update");

                let context = TestContext::new(db, authz);

                let payload = ListRequest {
                    room_id: room.id(),
                    last_created_at: None,
                    limit: None,
                };

                let messages = handle_request::<ListHandler>(&context, &agent, payload)
                    .await
                    .expect("Failed to list editions");

                let (resp_editions, respp) = find_response::<Vec<Edition>>(messages.as_slice());
                assert_eq!(respp.status(), ResponseStatus::OK);
                assert_eq!(resp_editions.len(), editions.len());
                assert_eq!(resp_editions[0].id(), editions[0].id());
            });
        }

        #[test]
        fn list_editions_not_authorized() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (room, _editions) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);

                    let editions = (1..2)
                        .map(|_idx| {
                            factory::Edition::new(room.id(), agent.agent_id()).insert(&conn)
                        })
                        .collect::<Vec<Edition>>();

                    (room, editions)
                };

                let context = TestContext::new(db, TestAuthz::new());

                let payload = ListRequest {
                    room_id: room.id(),
                    last_created_at: None,
                    limit: None,
                };

                let resp = handle_request::<ListHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success without authorization on editions list");

                assert_eq!(resp.status_code(), ResponseStatus::FORBIDDEN);
            });
        }

        #[test]
        fn list_editions_missing_room() {
            futures::executor::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let context = TestContext::new(TestDb::new(), TestAuthz::new());

                let payload = ListRequest {
                    room_id: Uuid::new_v4(),
                    last_created_at: None,
                    limit: None,
                };

                let resp = handle_request::<ListHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success listing editions for no room");

                assert_eq!(resp.status_code(), ResponseStatus::NOT_FOUND);
            });
        }
    }

    mod delete {
        use super::super::*;
        use crate::db::edition::Object as Edition;
        use crate::test_helpers::prelude::*;

        #[test]
        fn delete_edition() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (room, editions) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);

                    let editions = (1..4)
                        .map(|_idx| {
                            factory::Edition::new(room.id(), agent.agent_id()).insert(&conn)
                        })
                        .collect::<Vec<Edition>>();

                    (room, editions)
                };

                let mut authz = TestAuthz::new();
                let room_id = room.id().to_string();

                let object = vec!["rooms", &room_id];

                authz.allow(agent.account_id(), object, "update");

                let context = TestContext::new(db, authz);

                let payload = DeleteRequest {
                    id: editions[0].id(),
                };

                let messages = handle_request::<DeleteHandler>(&context, &agent, payload)
                    .await
                    .expect("Failed to find deleted edition");

                let (resp_edition, resp) = find_response::<Edition>(messages.as_slice());
                assert_eq!(resp.status(), ResponseStatus::OK);
                assert_eq!(resp_edition.id(), editions[0].id());

                let conn = context.db().get().expect("Failed to get DB connection");
                let db_editions = db::edition::ListQuery::new(room.id())
                    .execute(&conn)
                    .expect("Failed to fetch editions");
                assert_eq!(db_editions.len(), editions.len() - 1);
            });
        }

        #[test]
        fn delete_edition_not_authorized() {
            futures::executor::block_on(async {
                let db = TestDb::new();
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let (room, editions) = {
                    let conn = db
                        .connection_pool()
                        .get()
                        .expect("Failed to get DB connection");

                    let room = shared_helpers::insert_room(&conn);

                    let editions = (1..4)
                        .map(|_idx| {
                            factory::Edition::new(room.id(), agent.agent_id()).insert(&conn)
                        })
                        .collect::<Vec<Edition>>();

                    (room, editions)
                };

                let context = TestContext::new(db, TestAuthz::new());

                let payload = DeleteRequest {
                    id: editions[0].id(),
                };

                let resp = handle_request::<DeleteHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success without authorization on editions list");

                assert_eq!(resp.status_code(), ResponseStatus::FORBIDDEN);

                let conn = context.db().get().expect("Failed to get DB connection");
                let db_editions = db::edition::ListQuery::new(room.id())
                    .execute(&conn)
                    .expect("Failed to fetch editions");
                assert_eq!(db_editions.len(), editions.len());
            });
        }

        #[test]
        fn delete_editions_missing_room() {
            futures::executor::block_on(async {
                let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

                let context = TestContext::new(TestDb::new(), TestAuthz::new());

                let payload = DeleteRequest { id: Uuid::new_v4() };

                let resp = handle_request::<DeleteHandler>(&context, &agent, payload)
                    .await
                    .expect_err("Unexpected success listing editions for no room");

                assert_eq!(resp.status_code(), ResponseStatus::NOT_FOUND);
            });
        }
    }
}
