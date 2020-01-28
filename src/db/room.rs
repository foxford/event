use std::ops::Bound;

use chrono::{serde::ts_seconds, DateTime, Utc};
use diesel::{pg::PgConnection, result::Error};
use serde_derive::Serialize;
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::schema::room;

///////////////////////////////////////////////////////////////////////////////

pub(crate) type Time = (Bound<DateTime<Utc>>, Bound<DateTime<Utc>>);
pub(crate) type Fragment = (Bound<i64>, Bound<i64>);

#[derive(Clone, Debug, Serialize, Identifiable, Queryable, QueryableByName)]
#[table_name = "room"]
pub(crate) struct Object {
    id: Uuid,
    #[serde(with = "crate::serde::ts_seconds_bound_tuple")]
    time: Time,
    audience: String,
    backend_room_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fragments: Option<Vec<Fragment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preroll_duration: Option<i64>,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
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
        use diesel::prelude::*;
        room::table.find(self.id).get_result(conn).optional()
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Insertable)]
#[table_name = "room"]
pub(crate) struct InsertQuery {
    audience: String,
    backend_room_id: Uuid,
    time: Time,
    tags: Option<JsonValue>,
}

impl InsertQuery {
    pub(crate) fn new(audience: &str, backend_room_id: Uuid, time: Time) -> Self {
        Self {
            audience: audience.to_owned(),
            backend_room_id,
            time,
            tags: None,
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
