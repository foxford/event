use std::ops::Bound;

use chrono::{Duration, SubsecRound, Utc};
use serde_json::json;
use sqlx::postgres::PgConnection;
use svc_agent::AgentId;
use uuid::Uuid;

use crate::app::s3_client::S3Client;
use crate::db::agent::{Object as Agent, Status as AgentStatus};
use crate::db::edition::Object as Edition;
use crate::db::room::Object as Room;

use super::{factory, USR_AUDIENCE};

///////////////////////////////////////////////////////////////////////////////

pub async fn insert_room(conn: &mut PgConnection) -> Room {
    let now = Utc::now().trunc_subsecs(0);

    factory::Room::new(Uuid::new_v4())
        .audience(USR_AUDIENCE)
        .time((
            Bound::Included(now),
            Bound::Excluded(now + Duration::hours(1)),
        ))
        .tags(&json!({ "webinar_id": "123" }))
        .insert(conn)
        .await
}

pub async fn insert_unbounded_room(conn: &mut PgConnection) -> Room {
    let now = Utc::now().trunc_subsecs(0);

    factory::Room::new(Uuid::new_v4())
        .audience(USR_AUDIENCE)
        .time((Bound::Included(now), Bound::Unbounded))
        .tags(&json!({ "webinar_id": "123" }))
        .insert(conn)
        .await
}

pub async fn insert_closed_room(conn: &mut PgConnection) -> Room {
    let now = Utc::now().trunc_subsecs(0);

    factory::Room::new(Uuid::new_v4())
        .audience(USR_AUDIENCE)
        .time((
            Bound::Included(now - Duration::hours(10)),
            Bound::Excluded(now - Duration::hours(8)),
        ))
        .tags(&json!({ "webinar_id": "123" }))
        .insert(conn)
        .await
}

pub async fn insert_validating_whiteboard_access_room(conn: &mut PgConnection) -> Room {
    let now = Utc::now().trunc_subsecs(0);

    factory::Room::new(Uuid::new_v4())
        .audience(USR_AUDIENCE)
        .time((
            Bound::Included(now),
            Bound::Excluded(now + Duration::hours(1)),
        ))
        .tags(&json!({ "webinar_id": "123" }))
        .validate_whiteboard_access(true)
        .insert(conn)
        .await
}

pub async fn insert_agent(conn: &mut PgConnection, agent_id: &AgentId, room_id: Uuid) -> Agent {
    factory::Agent::new()
        .agent_id(agent_id.to_owned())
        .room_id(room_id)
        .status(AgentStatus::Ready)
        .insert(conn)
        .await
}

pub async fn insert_edition(conn: &mut PgConnection, room: &Room, agent_id: &AgentId) -> Edition {
    factory::Edition::new(room.id(), &agent_id)
        .insert(conn)
        .await
}

pub fn mock_s3() -> S3Client {
    use rusoto_mock::{MockCredentialsProvider, MockRequestDispatcher};

    let s3 = rusoto_s3::S3Client::new_with(
        MockRequestDispatcher::default(),
        MockCredentialsProvider,
        Default::default(),
    );

    S3Client::new_with_client(s3).expect("Failed to init S3 client")
}
