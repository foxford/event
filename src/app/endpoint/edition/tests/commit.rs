use serde_json::json;
use svc_agent::mqtt::ResponseStatus;

use crate::app::endpoint::change;
use crate::db::{
    change::{ChangeType, Object as Change},
    event,
};
use crate::test_helpers::prelude::*;

use super::super::*;

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
        payload: CommitPayload {
            offset: 0
        },
    };

    let messages = handle_request::<CommitHandler>(&mut context, &agent, payload.clone())
        .await
        .expect("Failed to create edition");

    // Assert response
    let (_, respp, _) = find_response::<serde_json::Value>(messages.as_slice());
    assert_eq!(respp.status(), ResponseStatus::ACCEPTED);

    let (commit_notification, _, _) = find_event::<EditionCommitNotification>(messages.as_slice());
    let new_room_id = match commit_notification.result {
        EditionCommitResult::Error{..} => panic!("error in edition commit notification"),
        EditionCommitResult::Success { committed_room_id, .. } => committed_room_id,
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
