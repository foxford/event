use chrono::serde::{ts_seconds, ts_seconds_option};
use chrono::{DateTime, Utc};
use diesel::{pg::PgConnection, result::Error};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use svc_agent::AgentId;
use uuid::Uuid;

use super::room::Object as Room;
use crate::schema::event;

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[belongs_to(Room, foreign_key = "room_id")]
#[table_name = "event"]
pub(crate) struct Object {
    id: Uuid,
    room_id: Uuid,
    kind: String,
    set: String,
    label: Option<String>,
    data: JsonValue,
    offset: i64,
    created_by: AgentId,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
    #[serde(with = "ts_seconds_option")]
    deleted_at: Option<DateTime<Utc>>,
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct FindQuery {
    id: Uuid,
}

impl FindQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self { id }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Option<Object>, Error> {
        use crate::schema::event::dsl::*;
        use diesel::prelude::*;

        event.find(self.id).get_result(conn).optional()
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Direction {
    Forward,
    Backward,
}

impl Default for Direction {
    fn default() -> Self {
        Self::Forward
    }
}

pub(crate) struct ListQuery<'a> {
    room_id: Option<Uuid>,
    kind: Option<&'a str>,
    set: Option<&'a str>,
    label: Option<&'a str>,
    last_id: Option<Uuid>,
    direction: Direction,
    limit: Option<i64>,
}

impl<'a> ListQuery<'a> {
    pub(crate) fn new() -> Self {
        Self {
            room_id: None,
            kind: None,
            set: None,
            label: None,
            last_id: None,
            direction: Default::default(),
            limit: None,
        }
    }

    pub(crate) fn room_id(self, room_id: Uuid) -> Self {
        Self {
            room_id: Some(room_id),
            ..self
        }
    }

    pub(crate) fn kind(self, kind: &'a str) -> Self {
        Self {
            kind: Some(kind),
            ..self
        }
    }

    pub(crate) fn set(self, set: &'a str) -> Self {
        Self {
            set: Some(set),
            ..self
        }
    }

    pub(crate) fn label(self, label: &'a str) -> Self {
        Self {
            label: Some(label),
            ..self
        }
    }

    pub(crate) fn last_id(self, last_id: Uuid) -> Self {
        Self {
            last_id: Some(last_id),
            ..self
        }
    }

    pub(crate) fn direction(self, direction: Direction) -> Self {
        Self { direction, ..self }
    }

    pub(crate) fn limit(self, limit: i64) -> Self {
        Self {
            limit: Some(limit),
            ..self
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Vec<Object>, Error> {
        use diesel::prelude::*;

        let mut q = event::table.into_boxed();

        if let Some(room_id) = self.room_id {
            q = q.filter(event::room_id.eq(room_id));
        }

        if let Some(ref kind) = self.kind {
            q = q.filter(event::kind.eq(kind));
        }

        if let Some(ref set) = self.set {
            q = q.filter(event::set.eq(set));
        }

        if let Some(ref label) = self.label {
            q = q.filter(event::label.eq(label));
        }

        if let Some(limit) = self.limit {
            q = q.limit(limit);
        }

        let last_event = match self.last_id {
            None => None,
            Some(id) => FindQuery::new(id).execute(conn)?,
        };

        q = match self.direction {
            Direction::Forward => {
                if let Some(event) = last_event {
                    q = q.filter(event::created_at.gt(event.created_at));
                }

                q.order_by(event::offset.asc())
                    .then_order_by(event::created_at.asc())
            }
            Direction::Backward => {
                if let Some(event) = last_event {
                    q = q.filter(event::created_at.lt(event.created_at));
                }

                q.order_by(event::offset.desc())
                    .then_order_by(event::created_at.desc())
            }
        };

        q.get_results(conn)
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Insertable)]
#[table_name = "event"]
pub(crate) struct InsertQuery<'a> {
    id: Option<Uuid>,
    room_id: Uuid,
    kind: &'a str,
    set: &'a str,
    label: Option<&'a str>,
    data: JsonValue,
    offset: i64,
    created_by: &'a AgentId,
}

impl<'a> InsertQuery<'a> {
    pub(crate) fn new(
        room_id: Uuid,
        kind: &'a str,
        data: JsonValue,
        offset: i64,
        created_by: &'a AgentId,
    ) -> Self {
        Self {
            id: None,
            room_id,
            kind,
            set: kind,
            label: None,
            data,
            offset,
            created_by,
        }
    }

    pub(crate) fn id(self, id: Uuid) -> Self {
        Self {
            id: Some(id),
            ..self
        }
    }

    pub(crate) fn set(self, set: &'a str) -> Self {
        Self { set, ..self }
    }

    pub(crate) fn label(self, label: &'a str) -> Self {
        Self {
            label: Some(label),
            ..self
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Object, Error> {
        use crate::diesel::RunQueryDsl;
        use crate::schema::event::dsl::event;

        diesel::insert_into(event).values(self).get_result(conn)
    }
}
