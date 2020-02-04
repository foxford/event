use std::ops::Bound;

use chrono::{serde::ts_seconds, DateTime, Utc};
use diesel::{pg::PgConnection, result::Error};
use serde_derive::Serialize;
use uuid::Uuid;

use crate::db::room::Object as Room;
use crate::schema::adjustment;

///////////////////////////////////////////////////////////////////////////////

pub(crate) type Segment = (Bound<i64>, Bound<i64>);

#[derive(Clone, Debug, Serialize, Identifiable, Associations, Queryable)]
#[table_name = "adjustment"]
#[primary_key(room_id)]
#[belongs_to(Room, foreign_key = "room_id")]
pub(crate) struct Object {
    room_id: Uuid,
    #[serde(with = "ts_seconds")]
    started_at: DateTime<Utc>,
    #[serde(with = "crate::serde::milliseconds_bound_tuples")]
    segments: Vec<Segment>,
    offset: i64,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Insertable)]
#[table_name = "adjustment"]
pub(crate) struct InsertQuery {
    room_id: Uuid,
    started_at: DateTime<Utc>,
    segments: Vec<Segment>,
    offset: i64,
}

impl InsertQuery {
    pub(crate) fn new(
        room_id: Uuid,
        started_at: DateTime<Utc>,
        segments: Vec<Segment>,
        offset: i64,
    ) -> Self {
        Self {
            room_id,
            started_at,
            segments,
            offset,
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Object, Error> {
        use crate::diesel::RunQueryDsl;
        use crate::schema::adjustment::dsl::adjustment;

        diesel::insert_into(adjustment)
            .values(self)
            .get_result(conn)
    }
}
