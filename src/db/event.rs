use std::convert::TryInto;

use chrono::serde::{ts_milliseconds, ts_milliseconds_option};
use chrono::{DateTime, Utc};
use diesel::{pg::PgConnection, result::Error};
use futures::TryStreamExt;
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::postgres::PgConnection as SqlxPgConnection;
use sqlx::Error as SqlxError;
use svc_agent::AgentId;
use uuid06::Uuid as Uuid06;
use uuid08::Uuid as Uuid08;

use super::room::Object as Room;
use crate::schema::event;

////////////////////////////////////////////////////////////////////////////////

#[derive(
    Clone, Debug, Serialize, Deserialize, Identifiable, Queryable, QueryableByName, Associations,
)]
#[belongs_to(Room, foreign_key = "room_id")]
#[table_name = "event"]
pub(crate) struct Object {
    id: Uuid06,
    room_id: Uuid06,
    #[serde(rename = "type")]
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
    original_occurred_at: i64,
}

impl Object {
    pub(crate) fn id(&self) -> Uuid06 {
        self.id
    }

    #[cfg(test)]
    pub(crate) fn room_id(&self) -> Uuid06 {
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

    pub(crate) fn created_by(&self) -> &AgentId {
        &self.created_by
    }

    #[cfg(test)]
    pub(crate) fn original_occurred_at(&self) -> i64 {
        self.original_occurred_at
    }
}

// TODO: Get rid of it after resolving uuid version conflict.
struct SqlxObject {
    id: Uuid08,
    room_id: Uuid08,
    kind: String,
    set: String,
    label: Option<String>,
    data: JsonValue,
    occurred_at: i64,
    created_by: AgentId,
    created_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    original_occurred_at: i64,
}

impl TryInto<Object> for SqlxObject {
    type Error = SqlxError;

    fn try_into(self) -> Result<Object, Self::Error> {
        Ok(Object {
            id: Uuid06::from_uuid_bytes(*self.id.as_bytes()),
            room_id: Uuid06::from_uuid_bytes(*self.room_id.as_bytes()),
            kind: self.kind,
            set: self.set,
            label: self.label,
            data: self.data,
            occurred_at: self.occurred_at,
            created_by: self.created_by,
            created_at: self.created_at,
            deleted_at: self.deleted_at,
            original_occurred_at: self.original_occurred_at,
        })
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct Builder {
    room_id: Option<Uuid06>,
    kind: Option<String>,
    set: Option<String>,
    label: Option<String>,
    data: Option<JsonValue>,
    occurred_at: Option<i64>,
    created_by: Option<AgentId>,
}

impl Builder {
    pub(crate) fn new() -> Self {
        Self {
            room_id: None,
            kind: None,
            set: None,
            label: None,
            data: None,
            occurred_at: None,
            created_by: None,
        }
    }

    pub(crate) fn room_id(self, room_id: Uuid06) -> Self {
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

    pub(crate) fn build(self) -> Result<Object, &'static str> {
        let room_id = self.room_id.ok_or("Missing `room_id`")?;
        let kind = self.kind.ok_or("Missing `kind`")?;
        let set = self.set.unwrap_or_else(|| kind.clone());
        let data = self.data.ok_or("Missing `data`")?;
        let occurred_at = self.occurred_at.ok_or("Missing `occurred_at`")?;
        let created_by = self.created_by.ok_or("Missing `created_by`")?;

        Ok(Object {
            id: Uuid06::new_v4(),
            room_id,
            kind,
            set,
            label: self.label,
            data,
            occurred_at,
            created_by,
            created_at: Utc::now(),
            deleted_at: None,
            original_occurred_at: occurred_at,
        })
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
enum KindFilter {
    Single(String),
    Multiple(Vec<String>),
}

pub(crate) struct ListQuery {
    room_id: Option<Uuid06>,
    kind: Option<KindFilter>,
    set: Option<String>,
    label: Option<String>,
    last_occurred_at: Option<i64>,
    direction: Direction,
    limit: Option<i64>,
}

impl ListQuery {
    pub(crate) fn new() -> Self {
        Self {
            room_id: None,
            kind: None,
            set: None,
            label: None,
            last_occurred_at: None,
            direction: Default::default(),
            limit: None,
        }
    }

    pub(crate) fn room_id(self, room_id: Uuid06) -> Self {
        Self {
            room_id: Some(room_id),
            ..self
        }
    }

    pub(crate) fn kind(self, kind: String) -> Self {
        Self {
            kind: Some(KindFilter::Single(kind)),
            ..self
        }
    }

    pub(crate) fn kinds(self, kinds: Vec<String>) -> Self {
        Self {
            kind: Some(KindFilter::Multiple(kinds)),
            ..self
        }
    }

    pub(crate) fn set(self, set: String) -> Self {
        Self {
            set: Some(set),
            ..self
        }
    }

    pub(crate) fn label(self, label: String) -> Self {
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

        q = match self.kind {
            Some(KindFilter::Single(ref kind)) => q.filter(event::kind.eq(kind)),
            Some(KindFilter::Multiple(ref kinds)) => q.filter(event::kind.eq_any(kinds.iter())),
            None => q,
        };

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
                }

                q.order_by((event::occurred_at, event::created_at))
            }
            Direction::Backward => {
                if let Some(last_occurred_at) = self.last_occurred_at {
                    q = q.filter(event::occurred_at.lt(last_occurred_at));
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
pub(crate) struct InsertQuery {
    room_id: Uuid06,
    kind: String,
    set: String,
    label: Option<String>,
    data: JsonValue,
    occurred_at: i64,
    created_by: AgentId,
    created_at: Option<DateTime<Utc>>,
}

impl InsertQuery {
    pub(crate) fn new(
        room_id: Uuid06,
        kind: String,
        data: JsonValue,
        occurred_at: i64,
        created_by: AgentId,
    ) -> Self {
        Self {
            room_id,
            set: kind.clone(),
            kind,
            label: None,
            data,
            occurred_at,
            created_by,
            created_at: None,
        }
    }

    pub(crate) fn set(self, set: String) -> Self {
        Self { set, ..self }
    }

    pub(crate) fn label(self, label: String) -> Self {
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
    room_id: Uuid06,
    kind: &'a str,
}

impl<'a> DeleteQuery<'a> {
    pub(crate) fn new(room_id: Uuid06, kind: &'a str) -> Self {
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

#[derive(Clone)]
pub(crate) struct SetStateQuery {
    room_id: Uuid08,
    set: String,
    occurred_at: Option<i64>,
    original_occurred_at: i64,
    limit: i64,
}

impl SetStateQuery {
    pub(crate) fn new(room_id: Uuid06, set: String, original_occurred_at: i64, limit: i64) -> Self {
        Self {
            room_id: Uuid08::from_bytes(*room_id.as_bytes()),
            set,
            occurred_at: None,
            original_occurred_at,
            limit,
        }
    }

    pub(crate) fn occurred_at(self, occurred_at: i64) -> Self {
        Self {
            occurred_at: Some(occurred_at),
            ..self
        }
    }

    pub(crate) async fn execute(
        &self,
        conn: &mut SqlxPgConnection,
    ) -> Result<Vec<Object>, SqlxError> {
        let mut stream = sqlx::query_as!(
            SqlxObject,
            r#"
            SELECT DISTINCT ON(original_occurred_at, label)
                id,
                room_id,
                kind,
                set,
                label,
                data,
                occurred_at,
                created_by as "created_by!: AgentId",
                created_at,
                deleted_at,
                original_occurred_at
            FROM event
            WHERE deleted_at IS NULL
            AND   room_id = $1
            AND   set = $2
            AND   original_occurred_at < $3
            AND   occurred_at < COALESCE($4, 9223372036854775807)
            ORDER BY original_occurred_at DESC, label ASC, occurred_at DESC
            LIMIT $5
            "#,
            self.room_id,
            self.set,
            self.original_occurred_at,
            self.occurred_at,
            self.limit,
        )
        .fetch(conn);

        let mut objects = Vec::with_capacity(self.limit as usize);

        while let Some(sqlx_object) = stream.try_next().await? {
            objects.push(sqlx_object.try_into()?);
        }

        Ok(objects)
    }

    pub(crate) async fn total_count(&self, conn: &mut SqlxPgConnection) -> Result<i64, SqlxError> {
        sqlx::query!(
            "
            SELECT COUNT(DISTINCT label) AS total
            FROM event
            WHERE deleted_at IS NULL
            AND   room_id = $1
            AND   set = $2
            AND   original_occurred_at < $3
            AND   occurred_at < COALESCE($4, 9223372036854775807)
            ",
            self.room_id,
            self.set,
            self.original_occurred_at,
            self.occurred_at,
        )
        .fetch_one(conn)
        .await
        .map(|r| r.total.unwrap_or(0))
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct OriginalEventQuery {
    room_id: Uuid06,
    set: String,
    label: String,
}

impl OriginalEventQuery {
    pub(crate) fn new(room_id: Uuid06, set: String, label: String) -> Self {
        Self {
            room_id,
            set,
            label,
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Option<Object>, Error> {
        use crate::diesel::RunQueryDsl;
        use diesel::prelude::*;

        event::table
            .filter(event::deleted_at.is_null())
            .filter(event::room_id.eq(self.room_id))
            .filter(event::set.eq(&self.set))
            .filter(event::label.eq(&self.label))
            .order_by(event::occurred_at)
            .limit(1)
            .get_result(conn)
            .optional()
    }
}
