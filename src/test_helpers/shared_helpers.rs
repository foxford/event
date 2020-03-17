use std::ops::Bound;

use chrono::{Duration, SubsecRound, Utc};
use diesel::pg::PgConnection;
use serde_json::json;
use svc_agent::AgentId;
use uuid::Uuid;

use crate::db::agent_session::{Object as Agent, Status as AgentStatus};
use crate::db::room::Object as Room;

use super::{factory, USR_AUDIENCE};

///////////////////////////////////////////////////////////////////////////////

pub(crate) fn insert_room(conn: &PgConnection) -> Room {
    let now = Utc::now().trunc_subsecs(0);

    factory::Room::new()
        .audience(USR_AUDIENCE)
        .time((
            Bound::Included(now),
            Bound::Excluded(now + Duration::hours(1)),
        ))
        .tags(&json!({ "webinar_id": "123" }))
        .insert(&conn)
}

pub(crate) fn insert_closed_room(conn: &PgConnection) -> Room {
    let now = Utc::now().trunc_subsecs(0);

    factory::Room::new()
        .audience(USR_AUDIENCE)
        .time((
            Bound::Included(now - Duration::hours(10)),
            Bound::Excluded(now - Duration::hours(8)),
        ))
        .tags(&json!({ "webinar_id": "123" }))
        .insert(&conn)
}

pub(crate) fn insert_agent_session(
    conn: &PgConnection,
    agent_id: &AgentId,
    room_id: Uuid,
) -> Agent {
    factory::AgentSession::new()
        .agent_id(agent_id.to_owned())
        .room_id(room_id)
        .status(AgentStatus::Started)
        .time((Bound::Included(Utc::now()), Bound::Unbounded))
        .insert(&conn)
}
