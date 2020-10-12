use std::ops::{Bound, RangeBounds};

use chrono::{serde::ts_seconds, DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::postgres::{types::PgRange, PgConnection};
use uuid::Uuid;

///////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Object {
    id: Uuid,
    audience: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_room_id: Option<Uuid>,
    #[serde(with = "serde::time")]
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

    pub(crate) fn time(&self) -> (Bound<DateTime<Utc>>, Bound<DateTime<Utc>>) {
        (self.time.0.start, self.time.0.end)
    }

    pub(crate) fn tags(&self) -> Option<&JsonValue> {
        self.tags.as_ref()
    }

    pub(crate) fn is_closed(&self) -> bool {
        match self.time.0.end_bound() {
            Bound::Included(t) => *t < Utc::now(),
            Bound::Excluded(t) => *t <= Utc::now(),
            Bound::Unbounded => false,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Builder {
    id: Option<Uuid>,
    audience: Option<String>,
    source_room_id: Option<Uuid>,
    time: Option<Time>,
    tags: Option<JsonValue>,
    created_at: Option<DateTime<Utc>>,
}

impl Builder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn id(self, id: Uuid) -> Self {
        Self {
            id: Some(id),
            ..self
        }
    }

    pub(crate) fn audience(self, audience: String) -> Self {
        Self {
            audience: Some(audience),
            ..self
        }
    }

    pub(crate) fn source_room_id(self, source_room_id: Option<Uuid>) -> Self {
        Self {
            source_room_id,
            ..self
        }
    }

    pub(crate) fn time(self, time: Time) -> Self {
        Self {
            time: Some(time),
            ..self
        }
    }

    pub(crate) fn tags(self, tags: Option<JsonValue>) -> Self {
        Self { tags, ..self }
    }

    pub(crate) fn created_at(self, created_at: DateTime<Utc>) -> Self {
        Self {
            created_at: Some(created_at),
            ..self
        }
    }

    pub(crate) fn build(self) -> anyhow::Result<Object> {
        Ok(Object {
            id: self.id.ok_or_else(|| anyhow!("missing id"))?,
            audience: self.audience.ok_or_else(|| anyhow!("missing audience"))?,
            source_room_id: self.source_room_id,
            time: self.time.ok_or_else(|| anyhow!("missing time"))?,
            tags: self.tags,
            created_at: self
                .created_at
                .ok_or_else(|| anyhow!("missing created_at"))?,
        })
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub(crate) struct FindQuery {
    id: Uuid,
    time: Option<Time>,
}

impl FindQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self { id, time: None }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Option<Object>> {
        let time: Option<PgRange<DateTime<Utc>>> = self.time.map(|t| t.into());

        sqlx::query_as!(
            Object,
            r#"
            SELECT id, audience, source_room_id, time AS "time!: Time", tags, created_at
            FROM room
            WHERE id = $1
            AND   ($2::TSTZRANGE IS NULL OR time && $2::TSTZRANGE)
            "#,
            self.id,
            time,
        )
        .fetch_optional(conn)
        .await
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
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

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        let time: PgRange<DateTime<Utc>> = self.time.into();

        sqlx::query_as!(
            Object,
            r#"
            INSERT INTO room (audience, source_room_id, time, tags)
            VALUES ($1, $2, $3, $4)
            RETURNING id, audience, source_room_id, time AS "time!: Time", tags, created_at
            "#,
            self.audience,
            self.source_room_id,
            Some(time),
            self.tags,
        )
        .fetch_one(conn)
        .await
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
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

    pub(crate) fn time(self, time: Option<Time>) -> Self {
        Self { time, ..self }
    }

    pub(crate) fn tags(self, tags: Option<JsonValue>) -> Self {
        Self { tags, ..self }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        let time: Option<PgRange<DateTime<Utc>>> = self.time.map(|t| t.into());

        sqlx::query_as!(
            Object,
            r#"
            UPDATE room
            SET time = COALESCE($2, time),
                tags = COALESCE($3::JSON, tags)
            WHERE id = $1
            RETURNING id, audience, source_room_id, time AS "time!: Time", tags, created_at
            "#,
            self.id,
            time,
            self.tags,
        )
        .fetch_one(conn)
        .await
    }
}

///////////////////////////////////////////////////////////////////////////////

type BoundedDateTimeTuple = (Bound<DateTime<Utc>>, Bound<DateTime<Utc>>);

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::Type)]
#[sqlx(transparent)]
#[serde(from = "BoundedDateTimeTuple")]
#[serde(into = "BoundedDateTimeTuple")]
pub(crate) struct Time(PgRange<DateTime<Utc>>);

impl From<BoundedDateTimeTuple> for Time {
    fn from(time: BoundedDateTimeTuple) -> Self {
        Self(PgRange::from(time))
    }
}

impl Into<BoundedDateTimeTuple> for Time {
    fn into(self) -> BoundedDateTimeTuple {
        (self.0.start, self.0.end)
    }
}

impl Into<PgRange<DateTime<Utc>>> for Time {
    fn into(self) -> PgRange<DateTime<Utc>> {
        self.0
    }
}

////////////////////////////////////////////////////////////////////////////////

pub(crate) mod serde {
    pub(crate) mod time {
        use super::super::Time;
        use crate::serde::ts_seconds_bound_tuple;
        use serde::{de, ser};

        pub(crate) fn serialize<S>(value: &Time, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            ts_seconds_bound_tuple::serialize(&value.to_owned().into(), serializer)
        }

        pub(crate) fn deserialize<'de, D>(d: D) -> Result<Time, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            let time = ts_seconds_bound_tuple::deserialize(d)?;
            Ok(Time::from(time))
        }
    }
}
