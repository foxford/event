use super::super::*;
use crate::db::edition::Object as Edition;
use crate::test_helpers::prelude::*;

use svc_agent::mqtt::ResponseStatus;
use uuid::Uuid;

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
    authz.allow(
        agent.account_id(),
        vec!["classrooms", &room.classroom_id().to_string()],
        "update",
    );

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
