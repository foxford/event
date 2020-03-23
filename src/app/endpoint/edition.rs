use async_std::stream;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_derive::Deserialize;
use svc_agent::{
    mqtt::{IncomingRequestProperties, ResponseStatus},
    Addressable,
};

use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::db;

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
        let conn = context.db().get()?;

        let room = db::room::FindQuery::new(payload.room_id)
            .execute(&conn)?
            .ok_or_else(|| format!("Room not found, id = '{}'", payload.room_id))
            .status(ResponseStatus::NOT_FOUND)?;

        let authz_time = context
            .authz()
            .authorize(
                room.audience(),
                reqp,
                vec!["rooms", &room.id().to_string()],
                "update",
            )
            .await?;

        let query = db::edition::InsertQuery::new(&payload.room_id, reqp.as_agent_id());
        let edition = query.execute(&conn)?;

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
        let conn = context.db().get()?;

        let room = db::room::FindQuery::new(payload.room_id)
            .execute(&conn)?
            .ok_or_else(|| format!("Room not found, id = '{}'", payload.room_id))
            .status(ResponseStatus::NOT_FOUND)?;

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

        let mut query = db::edition::ListQuery::new(&room_id);

        if let Some(ref last_created_at) = payload.last_created_at {
            query = query.last_created_at(last_created_at);
        }

        if let Some(limit) = payload.limit {
            query = query.limit(limit);
        }

        let editions = query.execute(&conn)?;

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
}
