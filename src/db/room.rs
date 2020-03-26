use std::ops::Bound;

use chrono::{serde::ts_seconds, DateTime, Utc};
use diesel::{pg::PgConnection, result::Error};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::schema::room;

///////////////////////////////////////////////////////////////////////////////

pub(crate) type Time = (Bound<DateTime<Utc>>, Bound<DateTime<Utc>>);

/// Use to filter by not expired room allowing time before room opening.
///
///    [-----room.time-----]
/// [---------------------------- OK
///              [--------------- OK
///                           [-- NOT OK
pub(crate) fn since_now() -> Time {
    (Bound::Included(Utc::now()), Bound::Unbounded)
}

/// Use to filter strictly by room time range.
///
///    [-----room.time-----]
///  |                            NOT OK
///              |                OK
///                          |    NOT OK
pub(crate) fn now() -> Time {
    let now = Utc::now();
    (Bound::Included(now), Bound::Included(now))
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Deserialize, Serialize, Identifiable, Associations, Queryable)]
#[table_name = "room"]
#[belongs_to(Object, foreign_key = "source_room_id")]
pub(crate) struct Object {
    id: Uuid,
    audience: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_room_id: Option<Uuid>,
    #[serde(with = "crate::serde::ts_seconds_bound_tuple")]
    time: Time,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<JsonValue>,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
}

impl Object {
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }

    pub(crate) fn audience(&self) -> &str {
        &self.audience
    }

    pub(crate) fn source_room_id(&self) -> Option<Uuid> {
        self.source_room_id
    }

    pub(crate) fn time(&self) -> &Time {
        &self.time
    }

    pub(crate) fn tags(&self) -> Option<&JsonValue> {
        self.tags.as_ref()
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct FindQuery {
    id: Uuid,
    time: Option<Time>,
}

impl FindQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self { id, time: None }
    }

    pub(crate) fn time(self, time: Time) -> Self {
        Self {
            time: Some(time),
            ..self
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Option<Object>, Error> {
        use diesel::prelude::*;
        use diesel::{dsl::sql, sql_types::Tstzrange};

        let mut q = room::table.into_boxed();

        if let Some(time) = self.time {
            q = q.filter(sql("room.time && ").bind::<Tstzrange, _>(time));
        }

        q.filter(room::id.eq(self.id)).get_result(conn).optional()
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Insertable)]
#[table_name = "room"]
pub(crate) struct InsertQuery {
    audience: String,
    source_room_id: Option<Uuid>,
    time: Time,
    tags: Option<JsonValue>,
}

impl InsertQuery {
    pub(crate) fn new(audience: &str, time: Time) -> Self {
        Self {
            audience: audience.to_owned(),
            source_room_id: None,
            time,
            tags: None,
        }
    }

    pub(crate) fn source_room_id(self, source_room_id: Uuid) -> Self {
        Self {
            source_room_id: Some(source_room_id),
            ..self
        }
    }

    pub(crate) fn tags(self, tags: JsonValue) -> Self {
        Self {
            tags: Some(tags),
            ..self
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Object, Error> {
        use crate::diesel::RunQueryDsl;
        use crate::schema::room::dsl::room;

        diesel::insert_into(room).values(self).get_result(conn)
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Identifiable, AsChangeset, Deserialize)]
#[table_name = "room"]
pub(crate) struct UpdateQuery {
    id: Uuid,
    time: Option<Time>,
    tags: Option<JsonValue>,
}

impl UpdateQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self {
            id,
            time: None,
            tags: None,
        }
    }

    pub(crate) fn time(self, time: Time) -> Self {
        Self {
            time: Some(time),
            ..self
        }
    }

    pub(crate) fn tags(self, tags: JsonValue) -> Self {
        Self {
            tags: Some(tags),
            ..self
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Object, Error> {
        use diesel::prelude::*;

        diesel::update(self).set(self).get_result(conn)
    }
}
