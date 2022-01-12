use std::ops::Bound;

use chrono::{DateTime, Utc};
use serde_json::Value as JsonValue;
use sqlx::postgres::PgConnection;
use svc_agent::AgentId;
use svc_authn::Authenticable;
use uuid::Uuid;

use crate::db;

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Default)]
pub(crate) struct Room {
    audience: Option<String>,
    time: Option<db::room::Time>,
    tags: Option<JsonValue>,
    preserve_history: Option<bool>,
    classroom_id: Option<Uuid>,
    validate_whiteboard_access: Option<bool>,
}

impl Room {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn audience(self, audience: &str) -> Self {
        Self {
            audience: Some(audience.to_owned()),
            ..self
        }
    }

    pub(crate) fn time(self, time: (Bound<DateTime<Utc>>, Bound<DateTime<Utc>>)) -> Self {
        Self {
            time: Some(time.into()),
            ..self
        }
    }

    pub(crate) fn tags(self, tags: &JsonValue) -> Self {
        Self {
            tags: Some(tags.to_owned()),
            ..self
        }
    }

    pub(crate) fn preserve_history(self, preserve_history: bool) -> Self {
        Self {
            preserve_history: Some(preserve_history),
            ..self
        }
    }

    pub(crate) fn classroom_id(self, classroom_id: Uuid) -> Self {
        Self {
            classroom_id: Some(classroom_id),
            ..self
        }
    }

    pub(crate) fn validate_whiteboard_access(self, validate_whiteboard_access: bool) -> Self {
        Self {
            validate_whiteboard_access: Some(validate_whiteboard_access),
            ..self
        }
    }

    pub(crate) async fn insert(self, conn: &mut PgConnection) -> db::room::Object {
        let audience = self.audience.expect("Audience not set");
        let time = self.time.expect("Time not set");

        let mut query = db::room::InsertQuery::new(&audience, time);

        if let Some(tags) = self.tags {
            query = query.tags(tags)
        }

        if let Some(preserve_history) = self.preserve_history {
            query = query.preserve_history(preserve_history)
        }

        if let Some(classroom_id) = self.classroom_id {
            query = query.classroom_id(classroom_id)
        }

        if let Some(validate_whiteboard_access) = self.validate_whiteboard_access {
            query = query.validate_whiteboard_access(validate_whiteboard_access)
        }

        query.execute(conn).await.expect("Failed to insert room")
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

    pub(crate) async fn insert(self, conn: &mut PgConnection) -> db::agent::Object {
        let agent_id = self.agent_id.expect("Agent ID not set");
        let room_id = self.room_id.expect("Room ID not set");

        let mut query = db::agent::InsertQuery::new(agent_id, room_id);

        if let Some(status) = self.status {
            query = query.status(status);
        }

        query.execute(conn).await.expect("Failed to insert agent")
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Default, Clone)]
pub(crate) struct Event {
    room_id: Option<Uuid>,
    kind: Option<String>,
    set: Option<String>,
    label: Option<String>,
    attribute: Option<String>,
    data: Option<JsonValue>,
    occurred_at: Option<i64>,
    created_by: Option<AgentId>,
    created_at: Option<DateTime<Utc>>,
    removed: bool,
}

impl Event {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn room_id(self, room_id: Uuid) -> Self {
        Self {
            room_id: Some(room_id),
            ..self
        }
    }

    pub(crate) fn kind(self, kind: &str) -> Self {
        Self {
            kind: Some(kind.to_owned()),
            ..self
        }
    }

    pub(crate) fn set(self, set: &str) -> Self {
        Self {
            set: Some(set.to_owned()),
            ..self
        }
    }

    pub(crate) fn label(self, label: &str) -> Self {
        Self {
            label: Some(label.to_owned()),
            ..self
        }
    }

    pub(crate) fn attribute(self, attribute: &str) -> Self {
        Self {
            attribute: Some(attribute.to_owned()),
            ..self
        }
    }

    pub(crate) fn data(self, data: &JsonValue) -> Self {
        Self {
            data: Some(data.to_owned()),
            ..self
        }
    }

    pub(crate) fn occurred_at(self, occurred_at: i64) -> Self {
        Self {
            occurred_at: Some(occurred_at),
            ..self
        }
    }

    pub(crate) fn created_by(self, created_by: &AgentId) -> Self {
        Self {
            created_by: Some(created_by.to_owned()),
            ..self
        }
    }

    pub(crate) fn created_at(self, created_at: DateTime<Utc>) -> Self {
        Self {
            created_at: Some(created_at),
            ..self
        }
    }

    pub(crate) fn removed(self, removed: bool) -> Self {
        Self { removed, ..self }
    }

    pub(crate) async fn insert(self, conn: &mut PgConnection) -> db::event::Object {
        let room_id = self.room_id.expect("Room ID not set");
        let kind = self.kind.expect("Kind not set");
        let data = self.data.expect("Data not set");
        let occurred_at = self.occurred_at.expect("Occurrence date not set");
        let created_by = self.created_by.expect("Creator not set");

        let mut query = db::event::InsertQuery::new(room_id, kind, data, occurred_at, created_by)
            .removed(self.removed);

        if let Some(set) = self.set {
            query = query.set(set);
        }

        if let Some(label) = self.label {
            query = query.label(label);
        }

        if let Some(attribute) = self.attribute {
            query = query.attribute(attribute);
        }

        if let Some(created_at) = self.created_at {
            query = query.created_at(created_at);
        }

        query.execute(conn).await.expect("Failed to insert event")
    }
}

pub(crate) struct Edition {
    source_room_id: Uuid,
    created_by: AgentId,
}

impl Edition {
    pub(crate) fn new(source_room_id: Uuid, created_by: &AgentId) -> Self {
        Self {
            source_room_id,
            created_by: created_by.to_owned(),
        }
    }

    pub(crate) async fn insert(self, conn: &mut PgConnection) -> db::edition::Object {
        let query = db::edition::InsertQuery::new(self.source_room_id, &self.created_by);
        query.execute(conn).await.expect("Failed to insert edition")
    }
}

pub(crate) struct Change {
    edition_id: Uuid,
    kind: crate::db::change::ChangeType,
    event_id: Option<Uuid>,
    event_data: Option<JsonValue>,
    event_kind: Option<String>,
    event_set: Option<String>,
    event_label: Option<String>,
    event_occurred_at: Option<i64>,
    event_created_by: Option<AgentId>,
}

impl Change {
    pub(crate) fn new(edition_id: Uuid, kind: crate::db::change::ChangeType) -> Self {
        Self {
            event_data: None,
            event_id: None,
            event_kind: None,
            event_set: None,
            event_label: None,
            event_occurred_at: None,
            event_created_by: None,
            edition_id,
            kind,
        }
    }

    pub(crate) fn event_id(self, event_id: Uuid) -> Self {
        Self {
            event_id: Some(event_id),
            ..self
        }
    }

    pub(crate) fn event_data(self, data: JsonValue) -> Self {
        Self {
            event_data: Some(data),
            ..self
        }
    }

    pub(crate) fn event_kind(self, kind: &str) -> Self {
        Self {
            event_kind: Some(kind.to_owned()),
            ..self
        }
    }

    pub(crate) fn event_set(self, set: &str) -> Self {
        Self {
            event_set: Some(set.to_owned()),
            ..self
        }
    }

    pub(crate) fn event_label(self, label: &str) -> Self {
        Self {
            event_label: Some(label.to_owned()),
            ..self
        }
    }

    pub(crate) fn event_occurred_at(self, occurred_at: i64) -> Self {
        Self {
            event_occurred_at: Some(occurred_at),
            ..self
        }
    }

    pub(crate) fn event_created_by(self, created_by: &AgentId) -> Self {
        Self {
            event_created_by: Some(created_by.to_owned()),
            ..self
        }
    }

    pub(crate) async fn insert(self, conn: &mut PgConnection) -> db::change::Object {
        let query = db::change::InsertQuery::new(self.edition_id, self.kind);

        let query = if let Some(event_id) = self.event_id {
            query.event_id(event_id)
        } else {
            query
        };

        let query = if let Some(event_data) = self.event_data {
            query.event_data(event_data)
        } else {
            query
        };

        let query = if let Some(event_kind) = self.event_kind {
            query.event_kind(event_kind)
        } else {
            query
        };

        let query = query.event_set(self.event_set);
        let query = query.event_label(self.event_label);

        let query = if let Some(occurred_at) = self.event_occurred_at {
            query.event_occurred_at(occurred_at)
        } else {
            query
        };

        let query = if let Some(created_by) = self.event_created_by {
            query.event_created_by(created_by)
        } else {
            query
        };

        query.execute(conn).await.expect("Failed to insert edition")
    }
}

////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub(crate) struct RoomBan<'a, T: Authenticable> {
    as_account_id: &'a T,
    room_id: Uuid,
    reason: Option<String>,
}

impl<'a, T: Authenticable> RoomBan<'a, T> {
    pub(crate) fn new(as_account_id: &'a T, room_id: Uuid) -> Self {
        Self {
            as_account_id,
            room_id,
            reason: None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn reason(self, reason: &str) -> Self {
        Self {
            reason: Some(reason.to_owned()),
            ..self
        }
    }

    pub(crate) async fn insert(self, conn: &mut PgConnection) -> db::room_ban::Object {
        let mut query = db::room_ban::InsertQuery::new(
            self.as_account_id.as_account_id().to_owned(),
            self.room_id,
        );

        if let Some(reason) = self.reason {
            query.reason(&reason);
        }

        query
            .execute(conn)
            .await
            .expect("Failed to insert room_ban")
    }
}
