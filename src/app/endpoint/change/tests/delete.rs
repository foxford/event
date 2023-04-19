use super::super::*;
use crate::db::{
    self,
    change::{ChangeType, Object as Change},
};
use crate::test_helpers::prelude::*;
use serde_json::json;
use svc_agent::mqtt::ResponseStatus;
use uuid::Uuid;

#[tokio::test]
async fn delete_change() {
    let db = TestDb::new().await;
    let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

    let (room, edition, changes) = {
        let mut conn = db.get_conn().await;
        let room = shared_helpers::insert_room(&mut conn).await;

        let edition = shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

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

        let edition = shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

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

        let edition = shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

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
    let classroom_id = room.classroom_id().to_string();
    let object = vec!["classrooms", &classroom_id];
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
