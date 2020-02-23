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
use crate::schema::{event, event_state_backward, event_state_forward};

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[belongs_to(Room, foreign_key = "room_id")]
#[table_name = "event"]
pub(crate) struct Object {
    id: Uuid,
    room_id: Uuid,
    kind: String,
    set: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    data: JsonValue,
    occurred_at: i64,
    created_by: AgentId,
    #[serde(with = "ts_milliseconds")]
    created_at: DateTime<Utc>,
    #[serde(
        with = "ts_milliseconds_option",
        skip_serializing_if = "Option::is_none",
        skip_deserializing,
        default
    )]
    deleted_at: Option<DateTime<Utc>>,
}

impl Object {
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }

    #[cfg(test)]
    pub(crate) fn room_id(&self) -> Uuid {
        self.room_id
    }

    #[cfg(test)]
    pub(crate) fn kind(&self) -> &str {
        &self.kind
    }

    #[cfg(test)]
    pub(crate) fn set(&self) -> &str {
        &self.set
    }

    #[cfg(test)]
    pub(crate) fn label(&self) -> Option<&str> {
        self.label.as_ref().map(|val| val.as_ref())
    }

    pub(crate) fn data(&self) -> &JsonValue {
        &self.data
    }

    pub(crate) fn occurred_at(&self) -> i64 {
        self.occurred_at
    }

    #[cfg(test)]
    pub(crate) fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
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
    last_occurred_at: Option<i64>,
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
            last_occurred_at: None,
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

    pub(crate) fn last_occurred_at(self, last_occurred_at: i64) -> Self {
        Self {
            last_occurred_at: Some(last_occurred_at),
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
                if let Some(last_occurred_at) = self.last_occurred_at {
                    q = q.filter(event::occurred_at.gt(last_occurred_at));

                    if let Some(last_created_at) = self.last_created_at {
                        q = q.or_filter(
                            event::occurred_at
                                .eq(last_occurred_at)
                                .and(event::created_at.gt(last_created_at)),
                        );
                    }
                }

                q.order_by((event::occurred_at, event::created_at))
            }
            Direction::Backward => {
                if let Some(last_occurred_at) = self.last_occurred_at {
                    q = q.filter(event::occurred_at.lt(last_occurred_at));

                    if let Some(last_created_at) = self.last_created_at {
                        q = q.or_filter(
                            event::occurred_at
                                .eq(last_occurred_at)
                                .and(event::created_at.lt(last_created_at)),
                        );
                    }
                }

                q.order_by((event::occurred_at.desc(), event::created_at.desc()))
            }
        };

        q.get_results(conn)
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Insertable)]
#[table_name = "event"]
pub(crate) struct InsertQuery<'a> {
    room_id: Uuid,
    kind: &'a str,
    set: &'a str,
    label: Option<&'a str>,
    data: &'a JsonValue,
    occurred_at: i64,
    created_by: &'a AgentId,
    created_at: Option<DateTime<Utc>>,
}

impl<'a> InsertQuery<'a> {
    pub(crate) fn new(
        room_id: Uuid,
        kind: &'a str,
        data: &'a JsonValue,
        occurred_at: i64,
        created_by: &'a AgentId,
    ) -> Self {
        Self {
            room_id,
            kind,
            set: kind,
            label: None,
            data,
            occurred_at,
            created_by,
            created_at: None,
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

    #[cfg(test)]
    pub(crate) fn created_at(self, created_at: DateTime<Utc>) -> Self {
        Self {
            created_at: Some(created_at),
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

pub(crate) struct DeleteQuery<'a> {
    room_id: Uuid,
    kind: &'a str,
}

impl<'a> DeleteQuery<'a> {
    pub(crate) fn new(room_id: Uuid, kind: &'a str) -> Self {
        Self { room_id, kind }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<(), Error> {
        use diesel::prelude::*;

        let q = event::table
            .filter(event::room_id.eq(self.room_id))
            .filter(event::kind.eq(self.kind));

        diesel::delete(q).execute(conn)?;
        Ok(())
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct SetStateQuery<'a> {
    room_id: Uuid,
    set: &'a str,
    occurred_at: i64,
    last_created_at: Option<DateTime<Utc>>,
    direction: Direction,
    limit: Option<i64>,
}

impl<'a> SetStateQuery<'a> {
    pub(crate) fn new(room_id: Uuid, set: &'a str, occurred_at: i64) -> Self {
        Self {
            room_id,
            set,
            occurred_at,
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
                q = q.filter(event::occurred_at.gt(self.occurred_at));

                if let Some(last_created_at) = self.last_created_at {
                    q = q.or_filter(
                        event::occurred_at
                            .eq(self.occurred_at)
                            .and(event::created_at.gt(last_created_at)),
                    );
                }

                q
            }
            Direction::Backward => {
                q = q.filter(event::occurred_at.lt(self.occurred_at));

                if let Some(last_created_at) = self.last_created_at {
                    q = q.or_filter(
                        event::occurred_at
                            .eq(self.occurred_at)
                            .and(event::created_at.lt(last_created_at)),
                    );
                }

                q
            }
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Vec<Object>, Error> {
        use crate::diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};

        match self.direction {
            Direction::Forward => {
                let mut q = self
                    .build_query()
                    .inner_join(event_state_forward::table)
                    .order_by((event::occurred_at.asc(), event::created_at.asc()));

                if let Some(limit) = self.limit {
                    q = q.limit(limit);
                }

                q.get_results(conn)
            }
            Direction::Backward => {
                let mut q = self
                    .build_query()
                    .inner_join(event_state_backward::table)
                    .order_by((event::occurred_at.desc(), event::created_at.desc()));

                if let Some(limit) = self.limit {
                    q = q.limit(limit);
                }

                q.get_results(conn)
            }
        }
    }

    pub(crate) fn total_count(&self, conn: &PgConnection) -> Result<i64, Error> {
        use crate::db::total_count::TotalCount;
        use crate::diesel::QueryDsl;

        match self.direction {
            Direction::Forward => self
                .build_query()
                .inner_join(event_state_forward::table)
                .total_count()
                .execute(conn),
            Direction::Backward => self
                .build_query()
                .inner_join(event_state_backward::table)
                .total_count()
                .execute(conn),
        }
    }
}
