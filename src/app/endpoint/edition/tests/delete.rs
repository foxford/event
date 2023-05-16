use super::super::*;
use crate::db::{self, edition::Object as Edition};
use crate::test_helpers::prelude::*;

use svc_agent::mqtt::ResponseStatus;
use uuid::Uuid;

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
    authz.allow(
        agent.account_id(),
        vec!["classrooms", &room.classroom_id().to_string()],
        "update",
    );

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
