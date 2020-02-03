use std::ops::Bound;

use chrono::{serde::ts_seconds, DateTime, Utc};
use diesel::{pg::PgConnection, result::Error};
use serde_derive::Serialize;
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::schema::room;

///////////////////////////////////////////////////////////////////////////////

pub(crate) type Time = (Bound<DateTime<Utc>>, Bound<DateTime<Utc>>);

#[derive(Clone, Debug, Serialize, Identifiable, Associations, Queryable)]
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
    id: Uuid,
    audience: String,
    source_room_id: Option<Uuid>,
    time: Time,
    tags: Option<JsonValue>,
}

impl InsertQuery {
    pub(crate) fn new(id: Uuid, audience: &str, time: Time) -> Self {
        Self {
            // Specify id explicitly to keep it in sync with the backend.
            id,
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
