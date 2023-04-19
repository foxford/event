use serde_json::json;
use svc_agent::mqtt::ResponseStatus;
use uuid::Uuid;

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

        let edition = shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

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

        let edition = shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

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
