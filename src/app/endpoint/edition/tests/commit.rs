use serde_json::json;
use std::collections::HashMap;
use svc_agent::mqtt::ResponseStatus;

use crate::app::endpoint::change;
use crate::db::{
    change::{ChangeType, Object as Change},
    event,
};
use crate::test_helpers::prelude::*;

use super::super::*;

#[tokio::test]
async fn addition() {
    let db = TestDb::new().await;
    let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

    let (room, edition, events_map) = {
        let mut conn = db.get_conn().await;
        let room = shared_helpers::insert_room(&mut conn).await;

        let edition = shared_helpers::insert_edition(&mut conn, &room, &agent.agent_id()).await;

        let mut events_map = HashMap::new();

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

            events_map.insert(i * 1000, event);
        }

        (room, edition, events_map)
    };

    // Allow agent to create editions
    let mut authz = TestAuthz::new();
    authz.allow(
        agent.account_id(),
        vec!["classrooms", &room.classroom_id().to_string()],
        "update",
    );

    // Make edition.create request
    let mut context = TestContext::new(db.clone(), authz);

    let addition_event_data = json!({ "text": "modified message" });
    let payload = change::CreateRequest {
        edition_id: edition.id(),
        changeset: change::Changeset::Addition(change::AdditionData {
            kind: "added_kind".to_string(),
            data: addition_event_data.clone(),
            set: None,
            label: Some("added_label".to_string()),
            occurred_at: 5_000_000,
            created_by: agent.agent_id().clone(),
        }),
    };

    let messages = handle_request::<change::CreateHandler>(&mut context, &agent, payload)
        .await
        .expect("Failed to create change");

    // Assert response
    let (change, respp, _) = find_response::<Change>(messages.as_slice());
    assert_eq!(respp.status(), ResponseStatus::CREATED);
    assert_eq!(change.edition_id(), edition.id());
    assert_eq!(change.kind(), ChangeType::Addition);

    let payload = CommitRequest {
        id: edition.id(),
        payload: CommitPayload { offset: 0 },
    };

    let messages = handle_request::<CommitHandler>(&mut context, &agent, payload.clone())
        .await
        .expect("Failed to create edition");

    // Assert response
    let (_, respp, _) = find_response::<serde_json::Value>(messages.as_slice());
    assert_eq!(respp.status(), ResponseStatus::ACCEPTED);

    let (commit_notification, _, _) = find_event::<EditionCommitNotification>(messages.as_slice());
    let new_room_id = match commit_notification.result {
        EditionCommitResult::Error { .. } => panic!("error in edition commit notification"),
        EditionCommitResult::Success {
            committed_room_id, ..
        } => committed_room_id,
    };

    //let mut conn = db.get_conn().await;
    let mut conn = db.get_conn().await;
    let events = event::ListQuery::new()
        .room_id(new_room_id)
        .execute(&mut conn)
        .await
        .expect("Failed to fetch events");

    assert_eq!(events.len(), 4);
    let mut exists_count = 0;
    for ev in events {
        if let Some(event) = events_map.get(&ev.occurred_at()) {
            assert_eq!(ev.created_by(), event.created_by());
            assert_eq!(ev.data(), event.data());
            assert_eq!(ev.label(), event.label());
            assert_eq!(ev.kind(), event.kind());
            assert_eq!(ev.set(), event.set());
            assert_eq!(ev.occurred_at(), event.occurred_at());
            exists_count += 1;
        } else {
            assert_eq!(ev.created_by(), agent.agent_id());
            assert_eq!(ev.data(), &addition_event_data);
            assert_eq!(ev.label(), Some("added_label"));
            assert_eq!(ev.kind(), "added_kind");
            assert_eq!(ev.occurred_at(), 5_000_000);
        }
    }
    assert_eq!(exists_count, 3);
}

#[tokio::test]
async fn modification() {
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
                .set("set1")
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
    let mut context = TestContext::new(db.clone(), authz);

    let modified_event = &events[1];
    let modified_event_data = json!({ "text": "modified message" });
    let payload = change::CreateRequest {
        edition_id: edition.id(),
        changeset: change::Changeset::Modification(change::ModificationData {
            event_id: modified_event.id(),
            kind: None,
            data: Some(modified_event_data.clone()),
            set: None,
            label: Some("new_label".to_string()),
            occurred_at: None,
            created_by: None,
        }),
    };

    let messages = handle_request::<change::CreateHandler>(&mut context, &agent, payload)
        .await
        .expect("Failed to create change");

    // Assert response
    let (change, respp, _) = find_response::<Change>(messages.as_slice());
    assert_eq!(respp.status(), ResponseStatus::CREATED);
    assert_eq!(change.edition_id(), edition.id());
    assert_eq!(change.kind(), ChangeType::Modification);

    let payload = CommitRequest {
        id: edition.id(),
        payload: CommitPayload { offset: 0 },
    };

    let messages = handle_request::<CommitHandler>(&mut context, &agent, payload.clone())
        .await
        .expect("Failed to create edition");

    // Assert response
    let (_, respp, _) = find_response::<serde_json::Value>(messages.as_slice());
    assert_eq!(respp.status(), ResponseStatus::ACCEPTED);

    let (commit_notification, _, _) = find_event::<EditionCommitNotification>(messages.as_slice());
    let new_room_id = match commit_notification.result {
        EditionCommitResult::Error { .. } => panic!("error in edition commit notification"),
        EditionCommitResult::Success {
            committed_room_id, ..
        } => committed_room_id,
    };

    //let mut conn = db.get_conn().await;
    let mut conn = db.get_conn().await;
    let events = event::ListQuery::new()
        .room_id(new_room_id)
        .execute(&mut conn)
        .await
        .expect("Failed to fetch events");

    assert_eq!(events.len(), 3);
    for ev in events {
        if ev.occurred_at() == modified_event.occurred_at() {
            assert_eq!(ev.data(), &modified_event_data);
            assert_eq!(ev.label(), Some("new_label"));
            assert_eq!(ev.kind(), modified_event.kind());
            assert_eq!(ev.created_by(), modified_event.created_by());
        }
    }
}

#[tokio::test]
async fn removal() {
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
                .set("set1")
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
    let mut context = TestContext::new(db.clone(), authz);

    let removed_event = &events[0];
    let payload = change::CreateRequest {
        edition_id: edition.id(),
        changeset: change::Changeset::Removal(change::RemovalData {
            event_id: removed_event.id(),
            kind: Some(removed_event.kind().to_string()),
            set: Some(removed_event.set().to_string()),
            occurred_at: Some(removed_event.occurred_at()),
        }),
    };

    let messages = handle_request::<change::CreateHandler>(&mut context, &agent, payload)
        .await
        .expect("Failed to create change");

    // Assert response
    let (change, respp, _) = find_response::<Change>(messages.as_slice());
    assert_eq!(respp.status(), ResponseStatus::CREATED);
    assert_eq!(change.edition_id(), edition.id());
    assert_eq!(change.kind(), ChangeType::Removal);

    let payload = CommitRequest {
        id: edition.id(),
        payload: CommitPayload { offset: 0 },
    };

    let messages = handle_request::<CommitHandler>(&mut context, &agent, payload.clone())
        .await
        .expect("Failed to create edition");

    // Assert response
    let (_, respp, _) = find_response::<serde_json::Value>(messages.as_slice());
    assert_eq!(respp.status(), ResponseStatus::ACCEPTED);

    let (commit_notification, _, _) = find_event::<EditionCommitNotification>(messages.as_slice());
    let new_room_id = match commit_notification.result {
        EditionCommitResult::Error { .. } => panic!("error in edition commit notification"),
        EditionCommitResult::Success {
            committed_room_id, ..
        } => committed_room_id,
    };

    //let mut conn = db.get_conn().await;
    let mut conn = db.get_conn().await;
    let events = event::ListQuery::new()
        .room_id(new_room_id)
        .execute(&mut conn)
        .await
        .expect("Failed to fetch events");

    assert_eq!(events.len(), 2);
    for ev in events {
        assert_ne!(ev.id(), removed_event.id());
    }
}

#[tokio::test]
async fn bulk_removal() {
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
    let mut context = TestContext::new(db.clone(), authz);

    let payload = change::CreateRequest {
        edition_id: edition.id(),
        changeset: change::Changeset::BulkRemoval(change::BulkRemovalData {
            set: "set2".to_string(),
        }),
    };

    let messages = handle_request::<change::CreateHandler>(&mut context, &agent, payload)
        .await
        .expect("Failed to create change");

    // Assert response
    let (change, respp, _) = find_response::<Change>(messages.as_slice());
    assert_eq!(respp.status(), ResponseStatus::CREATED);
    assert_eq!(change.edition_id(), edition.id());
    assert_eq!(change.kind(), ChangeType::BulkRemoval);
    assert_eq!(change.set().unwrap(), "set2");

    let payload = CommitRequest {
        id: edition.id(),
        payload: CommitPayload { offset: 0 },
    };

    let messages = handle_request::<CommitHandler>(&mut context, &agent, payload.clone())
        .await
        .expect("Failed to create edition");

    // Assert response
    let (_, respp, _) = find_response::<serde_json::Value>(messages.as_slice());
    assert_eq!(respp.status(), ResponseStatus::ACCEPTED);

    let (commit_notification, _, _) = find_event::<EditionCommitNotification>(messages.as_slice());
    let new_room_id = match commit_notification.result {
        EditionCommitResult::Error { .. } => panic!("error in edition commit notification"),
        EditionCommitResult::Success {
            committed_room_id, ..
        } => committed_room_id,
    };

    //let mut conn = db.get_conn().await;
    let mut conn = db.get_conn().await;
    let events = event::ListQuery::new()
        .room_id(new_room_id)
        .execute(&mut conn)
        .await
        .expect("Failed to fetch events");

    assert_eq!(events.len(), 3);
    for ev in events {
        assert_eq!(ev.set(), "set1");
        assert_eq!(ev.kind(), "message");
    }
}
