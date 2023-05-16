use super::super::*;
use crate::db::edition::Object as Edition;
use crate::test_helpers::prelude::*;

use svc_agent::mqtt::ResponseStatus;
use uuid::Uuid;

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
    authz.allow(
        agent.account_id(),
        vec!["classrooms", &room.classroom_id().to_string()],
        "update",
    );

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
