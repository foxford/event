use std::convert::{TryFrom, TryInto};
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
    preserve_history: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    classroom_id: Option<Uuid>,
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

    pub(crate) fn time(&self) -> Result<RoomTime, String> {
        self.time.clone().try_into()
    }

    pub(crate) fn tags(&self) -> Option<&JsonValue> {
        self.tags.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn preserve_history(&self) -> bool {
        self.preserve_history
    }

    pub(crate) fn classroom_id(&self) -> Option<Uuid> {
        self.classroom_id
    }

    pub fn authz_object(&self) -> Vec<String> {
        match self.classroom_id {
            Some(cid) => vec!["classrooms".into(), cid.to_string()],
            None => vec!["rooms".into(), self.id.to_string()],
        }
    }

    pub(crate) fn is_closed(&self) -> bool {
        match self.time.0.end_bound() {
            Bound::Included(t) => *t < Utc::now(),
            Bound::Excluded(t) => *t <= Utc::now(),
            Bound::Unbounded => false,
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        let now = Utc::now();
        let t = (self.time.0.start_bound(), self.time.0.end_bound());
        match t {
            (Bound::Included(s), Bound::Excluded(e)) => *s < now && *e > now,
            (Bound::Included(s), Bound::Unbounded) => *s < now,
            _ => false,
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
    preserve_history: Option<bool>,
    classroom_id: Option<Uuid>,
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

    pub(crate) fn preserve_history(self, preserve_history: bool) -> Self {
        Self {
            preserve_history: Some(preserve_history),
            ..self
        }
    }

    pub(crate) fn classroom_id(self, classroom_id: Option<Uuid>) -> Self {
        Self {
            classroom_id,
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
            preserve_history: self
                .preserve_history
                .ok_or_else(|| anyhow!("missing preserve_history"))?,
            classroom_id: self.classroom_id,
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
            SELECT
                id,
                audience,
                source_room_id,
                time AS "time!: Time",
                tags,
                created_at,
                preserve_history,
                classroom_id
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
    preserve_history: bool,
    classroom_id: Option<Uuid>,
}

impl InsertQuery {
    pub(crate) fn new(audience: &str, time: Time) -> Self {
        Self {
            audience: audience.to_owned(),
            source_room_id: None,
            time,
            tags: None,
            preserve_history: true,
            classroom_id: None,
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

    pub(crate) fn preserve_history(self, preserve_history: bool) -> Self {
        Self {
            preserve_history,
            ..self
        }
    }

    pub(crate) fn classroom_id(self, classroom_id: Uuid) -> Self {
        Self {
            classroom_id: Some(classroom_id),
            ..self
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        let time: PgRange<DateTime<Utc>> = self.time.into();

        sqlx::query_as!(
            Object,
            r#"
            INSERT INTO room (audience, source_room_id, time, tags, preserve_history, classroom_id)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING
                id,
                audience,
                source_room_id,
                time AS "time!: Time",
                tags,
                created_at,
                preserve_history,
                classroom_id
            "#,
            self.audience,
            self.source_room_id,
            Some(time),
            self.tags,
            self.preserve_history,
            self.classroom_id,
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
    classroom_id: Option<Uuid>,
}

impl UpdateQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self {
            id,
            time: None,
            tags: None,
            classroom_id: None,
        }
    }

    pub(crate) fn time(self, time: Option<Time>) -> Self {
        Self { time, ..self }
    }

    pub(crate) fn tags(self, tags: Option<JsonValue>) -> Self {
        Self { tags, ..self }
    }

    pub(crate) fn classroom_id(self, classroom_id: Option<Uuid>) -> Self {
        Self {
            classroom_id,
            ..self
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        let time: Option<PgRange<DateTime<Utc>>> = self.time.map(|t| t.into());

        sqlx::query_as!(
            Object,
            r#"
            UPDATE room
            SET time = COALESCE($2, time),
                tags = COALESCE($3::JSON, tags),
                classroom_id = COALESCE($4, classroom_id)
            WHERE id = $1
            RETURNING
                id,
                audience,
                source_room_id,
                time AS "time!: Time",
                tags,
                created_at,
                preserve_history,
                classroom_id
            "#,
            self.id,
            time,
            self.tags,
            self.classroom_id
        )
        .fetch_one(conn)
        .await
    }
}

///////////////////////////////////////////////////////////////////////////////

use crate::db::room_time::BoundedDateTimeTuple;
use crate::db::room_time::RoomTime;

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::Type)]
#[sqlx(transparent)]
#[serde(from = "RoomTime")]
#[serde(into = "BoundedDateTimeTuple")]
pub(crate) struct Time(PgRange<DateTime<Utc>>);

impl From<RoomTime> for Time {
    fn from(time: RoomTime) -> Self {
        let time: BoundedDateTimeTuple = time.into();
        Self(PgRange::from(time))
    }
}

impl TryFrom<Time> for RoomTime {
    type Error = String;

    fn try_from(t: Time) -> Result<Self, Self::Error> {
        match RoomTime::new((t.0.start, t.0.end)) {
            Some(rt) => Ok(rt),
            None => Err(format!(
                "Invalid room time: ({:?}, {:?})",
                t.0.start, t.0.end
            )),
        }
    }
}

impl Into<PgRange<DateTime<Utc>>> for Time {
    fn into(self) -> PgRange<DateTime<Utc>> {
        self.0
    }
}

impl From<Time> for BoundedDateTimeTuple {
    fn from(time: Time) -> BoundedDateTimeTuple {
        (time.0.start, time.0.end)
    }
}

impl From<BoundedDateTimeTuple> for Time {
    fn from(tuple: BoundedDateTimeTuple) -> Time {
        Self(PgRange::from(tuple))
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
