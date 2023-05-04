use std::ops::Bound;

use chrono::{serde::ts_seconds, DateTime, Utc};

use serde_derive::{Deserialize, Serialize};
use sqlx::postgres::{types::PgRange, PgConnection};
use uuid::Uuid;

///////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Serialize)]
pub struct Object {
    room_id: Uuid,
    #[serde(with = "ts_seconds")]
    started_at: DateTime<Utc>,
    #[serde(with = "serde::segments")]
    segments: Segments,
    offset: i64,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct InsertQuery {
    room_id: Uuid,
    started_at: DateTime<Utc>,
    segments: Segments,
    offset: i64,
}

impl InsertQuery {
    pub fn new(room_id: Uuid, started_at: DateTime<Utc>, segments: Segments, offset: i64) -> Self {
        Self {
            room_id,
            started_at,
            segments,
            offset,
        }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        sqlx::query_as!(
            Object,
            r#"
            INSERT INTO adjustment (room_id, started_at, segments, "offset")
            VALUES ($1, $2, $3, $4)
            RETURNING
                room_id,
                started_at,
                segments AS "segments!: Segments",
                "offset",
                created_at
            "#,
            self.room_id,
            self.started_at,
            self.segments as Segments,
            self.offset,
        )
        .fetch_one(conn)
        .await
    }
}

////////////////////////////////////////////////////////////////////////////////

type BoundedOffsetTuples = Vec<(Bound<i64>, Bound<i64>)>;

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::Type)]
#[sqlx(transparent)]
#[serde(from = "BoundedOffsetTuples")]
#[serde(into = "BoundedOffsetTuples")]
pub struct Segments(Vec<PgRange<i64>>);

impl From<BoundedOffsetTuples> for Segments {
    fn from(segments: BoundedOffsetTuples) -> Self {
        Self(segments.into_iter().map(PgRange::from).collect())
    }
}

impl From<Segments> for BoundedOffsetTuples {
    fn from(val: Segments) -> Self {
        val.0.into_iter().map(|s| (s.start, s.end)).collect()
    }
}

impl From<Segments> for Vec<PgRange<i64>> {
    fn from(val: Segments) -> Self {
        val.0
    }
}

////////////////////////////////////////////////////////////////////////////////

pub mod serde {
    pub mod segments {
        use super::super::{BoundedOffsetTuples, Segments};
        use crate::serde::milliseconds_bound_tuples;
        use serde::{de, ser};

        pub fn serialize<S>(value: &Segments, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            let bounded_offset_tuples: BoundedOffsetTuples = value.to_owned().into();
            milliseconds_bound_tuples::serialize(&bounded_offset_tuples, serializer)
        }

        pub fn deserialize<'de, D>(d: D) -> Result<Segments, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            milliseconds_bound_tuples::deserialize(d).map(Segments::from)
        }
    }
}
