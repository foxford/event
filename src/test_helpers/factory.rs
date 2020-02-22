use diesel::pg::PgConnection;
use serde_json::Value as JsonValue;
use svc_agent::AgentId;
use uuid::Uuid;

use crate::db;

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct Room {
    audience: Option<String>,
    time: Option<db::room::Time>,
    tags: Option<JsonValue>,
}

impl Room {
    pub(crate) fn new() -> Self {
        Self {
            audience: None,
            time: None,
            tags: None,
        }
    }

    pub(crate) fn audience(self, audience: &str) -> Self {
        Self {
            audience: Some(audience.to_owned()),
            ..self
        }
    }

    pub(crate) fn time(self, time: db::room::Time) -> Self {
        Self {
            time: Some(time),
            ..self
        }
    }

    pub(crate) fn tags(self, tags: &JsonValue) -> Self {
        Self {
            tags: Some(tags.to_owned()),
            ..self
        }
    }

    pub(crate) fn insert(self, conn: &PgConnection) -> db::room::Object {
        let audience = self.audience.expect("Audience not set");
        let time = self.time.expect("Time not set");

        let mut query = db::room::InsertQuery::new(&audience, time);

        if let Some(tags) = self.tags {
            query = query.tags(tags)
        }

        query.execute(conn).expect("Failed to insert room")
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct Agent {
    agent_id: Option<AgentId>,
    room_id: Option<Uuid>,
    status: Option<db::agent::Status>,
}

impl Agent {
    pub(crate) fn new() -> Self {
        Self {
            agent_id: None,
            room_id: None,
            status: None,
        }
    }

    pub(crate) fn agent_id(self, agent_id: AgentId) -> Self {
        Self {
            agent_id: Some(agent_id),
            ..self
        }
    }

    pub(crate) fn room_id(self, room_id: Uuid) -> Self {
        Self {
            room_id: Some(room_id),
            ..self
        }
    }

    pub(crate) fn status(self, status: db::agent::Status) -> Self {
        Self {
            status: Some(status),
            ..self
        }
    }

    pub(crate) fn insert(self, conn: &PgConnection) -> db::agent::Object {
        let agent_id = self.agent_id.expect("Agent ID not set");
        let room_id = self.room_id.expect("Room ID not set");

        let mut query = db::agent::InsertQuery::new(&agent_id, room_id);

        if let Some(status) = self.status {
            query = query.status(status);
        }

        query.execute(conn).expect("Failed to insert agent")
    }
}
