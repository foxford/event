use chrono::serde::{ts_milliseconds, ts_milliseconds_option};
use chrono::{DateTime, Utc};
use diesel::{
    pg::{Pg, PgConnection},
    result::Error,
};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use svc_agent::AgentId;
use uuid::Uuid;

use super::room::Object as Room;
use crate::schema::{event, event_state};

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
    occured_at: i64,
    created_by: AgentId,
    #[serde(with = "ts_milliseconds")]
    created_at: DateTime<Utc>,
    #[serde(with = "ts_milliseconds_option", skip_serializing_if = "Option::is_none")]
    deleted_at: Option<DateTime<Utc>>,
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug, Deserialize)]
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

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct ListQuery<'a> {
    room_id: Option<Uuid>,
    kind: Option<&'a str>,
    set: Option<&'a str>,
    label: Option<&'a str>,
    last_created_at: Option<DateTime<Utc>>,
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
            last_created_at: None,
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

    pub(crate) fn last_created_at(self, last_created_at: DateTime<Utc>) -> Self {
        Self {
            last_created_at: Some(last_created_at),
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

        let mut q = event::table
            .filter(event::deleted_at.is_null())
            .into_boxed();

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

        q = match self.direction {
            Direction::Forward => {
                if let Some(last_created_at) = self.last_created_at {
                    q = q.filter(event::created_at.gt(last_created_at));
                }

                q.order_by((event::occured_at, event::created_at))
            }
            Direction::Backward => {
                if let Some(last_created_at) = self.last_created_at {
                    q = q.filter(event::created_at.lt(last_created_at));
                }

                q.order_by((event::occured_at.desc(), event::created_at.desc()))
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
    occured_at: i64,
    created_by: &'a AgentId,
}

impl<'a> InsertQuery<'a> {
    pub(crate) fn new(
        room_id: Uuid,
        kind: &'a str,
        data: JsonValue,
        occured_at: i64,
        created_by: &'a AgentId,
    ) -> Self {
        Self {
            id: None,
            room_id,
            kind,
            set: kind,
            label: None,
            data,
            occured_at,
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

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct SetStateQuery<'a> {
    room_id: Uuid,
    set: &'a str,
    occured_at: i64,
    last_created_at: Option<DateTime<Utc>>,
    direction: Direction,
    limit: Option<i64>,
}

impl<'a> SetStateQuery<'a> {
    pub(crate) fn new(room_id: Uuid, set: &'a str, occured_at: i64) -> Self {
        Self {
            room_id,
            set,
            occured_at,
            last_created_at: None,
            direction: Default::default(),
            limit: None,
        }
    }

    pub(crate) fn last_created_at(self, last_created_at: DateTime<Utc>) -> Self {
        Self {
            last_created_at: Some(last_created_at),
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

    fn build_query(&'a self) -> event::BoxedQuery<'a, Pg> {
        use diesel::prelude::*;

        let mut q = event::table
            .filter(event::deleted_at.is_null())
            .filter(event::room_id.eq(self.room_id))
            .filter(event::set.eq(self.set))
            .into_boxed();

        match self.direction {
            Direction::Forward => {
                if let Some(last_created_at) = self.last_created_at {
                    q = q.filter(event::created_at.gt(last_created_at));
                }

                q.filter(event::occured_at.gt(self.occured_at))
            }
            Direction::Backward => {
                if let Some(last_created_at) = self.last_created_at {
                    q = q.filter(event::created_at.lt(last_created_at));
                }

                q.filter(event::occured_at.lt(self.occured_at))
            }
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Vec<Object>, Error> {
        use crate::diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};

        let mut q = self.build_query().inner_join(event_state::table);

        q = match self.direction {
            Direction::Forward => q.order_by((event::occured_at.asc(), event::created_at.asc())),
            Direction::Backward => q.order_by((event::occured_at.desc(), event::created_at.desc())),
        };

        if let Some(limit) = self.limit {
            q = q.limit(limit);
        }

        println!("{}", diesel::debug_query::<Pg, _>(&q));
        q.get_results(conn)
    }

    pub(crate) fn total_count(&self, conn: &PgConnection) -> Result<i64, Error> {
        use crate::db::total_count::TotalCount;
        use crate::diesel::QueryDsl;

        self.build_query()
            .inner_join(event_state::table)
            .total_count()
            .execute(conn)
    }
}
