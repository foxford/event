use std::ops::Bound;

use chrono::{Duration, SubsecRound, Utc};
use diesel::pg::PgConnection;
use serde_json::json;
use svc_agent::AgentId;
use uuid::Uuid;

use crate::db::agent::{Object as Agent, Status as AgentStatus};
use crate::db::edition::Object as Edition;
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

pub(crate) fn insert_agent(conn: &PgConnection, agent_id: &AgentId, room_id: Uuid) -> Agent {
    factory::Agent::new()
        .agent_id(agent_id.to_owned())
        .room_id(room_id)
        .status(AgentStatus::Ready)
        .insert(&conn)
}

pub(crate) fn insert_edition(conn: &PgConnection, room: &Room, agent_id: &AgentId) -> Edition {
    factory::Edition::new(room.id(), &agent_id).insert(&conn)
}
