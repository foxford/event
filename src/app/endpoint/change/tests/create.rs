use serde_json::json;
use svc_agent::mqtt::ResponseStatus;
use uuid::Uuid;

use crate::app::endpoint::change::create_request::{
    AdditionData, BulkRemovalData, Changeset, CreateRequest, ModificationData, RemovalData,
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

        let edition = shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

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

        let edition = shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

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

        let edition = shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

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

        let edition = shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

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

        let edition = shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

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

#[tokio::test]
async fn create_bulk_removal_change() {
    let db = TestDb::new().await;
    let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

    let (room, edition, _events) = {
        let mut conn = db.get_conn().await;
        let room = shared_helpers::insert_room(&mut conn).await;

        let edition = shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

        let mut events = vec![];

        for i in 1..4 {
            let event = factory::Event::new()
                .room_id(room.id())
                .set("set1")
                .kind("message")
                .data(&json!({ "text": format!("message {}", i) }))
                .occurred_at(i * 1000)
                .created_by(&agent.agent_id())
                .insert(&mut conn)
                .await;

            events.push(event);
        }

        for i in 1..2 {
            let event = factory::Event::new()
                .room_id(room.id())
                .set("set2")
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
        changeset: Changeset::BulkRemoval(BulkRemovalData {
            set: "set2".to_string(),
        }),
    };

    let messages = handle_request::<CreateHandler>(&mut context, &agent, payload)
        .await
        .expect("Failed to create change");

    // Assert response
    let (change, respp, _) = find_response::<Change>(messages.as_slice());
    assert_eq!(respp.status(), ResponseStatus::CREATED);
    assert_eq!(change.edition_id(), edition.id());
    assert_eq!(change.kind(), ChangeType::BulkRemoval);
    assert_eq!(change.set().unwrap(), "set2");
}
