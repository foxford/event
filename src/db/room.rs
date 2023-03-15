use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::ops::{Bound, RangeBounds};
use std::str::FromStr;

use chrono::{serde::ts_seconds, DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::postgres::{types::PgRange, PgConnection};
use svc_authn::AccountId;
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
    classroom_id: Uuid,
    #[serde(default)]
    locked_types: HashMap<String, bool>,
    #[serde(default)]
    validate_whiteboard_access: bool,
    #[serde(default)]
    whiteboard_access: HashMap<AccountId, bool>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct DbObject {
    id: Uuid,
    audience: String,
    source_room_id: Option<Uuid>,
    time: Time,
    tags: Option<JsonValue>,
    created_at: DateTime<Utc>,
    preserve_history: bool,
    classroom_id: Uuid,
    locked_types: JsonValue,
    validate_whiteboard_access: bool,
    whiteboard_access: JsonValue,
}

impl TryFrom<DbObject> for Object {
    type Error = sqlx::Error;

    fn try_from(v: DbObject) -> Result<Self, Self::Error> {
        let DbObject {
            id,
            audience,
            source_room_id,
            time,
            tags,
            created_at,
            preserve_history,
            classroom_id,
            locked_types,
            validate_whiteboard_access,
            whiteboard_access,
        } = v;

        let locked_types = locked_types
            .as_object()
            .ok_or_else(|| sqlx::Error::ColumnDecode {
                index: "locked_types".into(),
                source: Box::new(JsonbConversionFail::new(
                    JsonbConversionFailKind::LockedTypes,
                    id,
                )) as Box<dyn std::error::Error + Sync + Send>,
            })?
            .into_iter()
            .map(|(k, v)| (k.into(), v.as_bool().unwrap_or(false)))
            .collect();

        let whiteboard_access = whiteboard_access
            .as_object()
            .ok_or_else(|| sqlx::Error::ColumnDecode {
                index: "whiteboard_access".into(),
                source: Box::new(JsonbConversionFail::new(
                    JsonbConversionFailKind::WhiteboardAccess,
                    id,
                )) as Box<dyn std::error::Error + Sync + Send>,
            })?
            .into_iter()
            .filter_map(|(k, v)| {
                AccountId::from_str(k)
                    .ok()
                    .map(|k| (k, v.as_bool().unwrap_or(false)))
            })
            .collect();

        Ok(Self {
            id,
            audience,
            source_room_id,
            time,
            tags,
            created_at,
            preserve_history,
            classroom_id,
            locked_types,
            validate_whiteboard_access,
            whiteboard_access,
        })
    }
}

impl From<Object> for DbObject {
    fn from(v: Object) -> Self {
        let Object {
            id,
            audience,
            source_room_id,
            time,
            tags,
            created_at,
            preserve_history,
            classroom_id,
            locked_types,
            validate_whiteboard_access,
            whiteboard_access,
        } = v;

        let locked_types = serde_json::to_value(locked_types).unwrap();
        let whiteboard_access = serde_json::to_value(whiteboard_access).unwrap();

        Self {
            id,
            audience,
            source_room_id,
            time,
            tags,
            created_at,
            preserve_history,
            classroom_id,
            locked_types,
            validate_whiteboard_access,
            whiteboard_access,
        }
    }
}

#[derive(Debug)]
struct JsonbConversionFail {
    kind: JsonbConversionFailKind,
    room_id: Uuid,
}

#[derive(Debug)]
enum JsonbConversionFailKind {
    LockedTypes,
    WhiteboardAccess,
}

impl JsonbConversionFail {
    pub fn new(kind: JsonbConversionFailKind, room_id: Uuid) -> Self {
        Self { kind, room_id }
    }
}

impl std::fmt::Display for JsonbConversionFail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Failed to convert {:?} jsonb to map, room id = {}",
            self.kind, self.room_id
        )
    }
}

impl std::error::Error for JsonbConversionFail {}

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

    pub(crate) fn classroom_id(&self) -> Uuid {
        self.classroom_id
    }

    pub(crate) fn locked_types(&self) -> &HashMap<String, bool> {
        &self.locked_types
    }

    pub(crate) fn validate_whiteboard_access(&self) -> bool {
        self.validate_whiteboard_access
    }

    pub(crate) fn whiteboard_access(&self) -> &HashMap<AccountId, bool> {
        &self.whiteboard_access
    }

    pub fn authz_object(&self) -> Vec<String> {
        vec!["classrooms".into(), self.classroom_id.to_string()]
    }

    fn account_has_whiteboard_access(&self, account: &AccountId) -> bool {
        if self.validate_whiteboard_access {
            self.whiteboard_access.get(account) == Some(&true)
        } else {
            true
        }
    }

    fn has_locked_type(&self, kind: &str) -> bool {
        self.locked_types.get(kind) == Some(&true)
    }

    pub fn event_should_authz_room_update(&self, kind: &str, account: &AccountId) -> bool {
        self.has_locked_type(kind)
            || ((kind == "draw" || kind == "draw_lock")
                && !self.account_has_whiteboard_access(account))
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
    classroom_id: Uuid,
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

    pub(crate) fn classroom_id(self, classroom_id: Uuid) -> Self {
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
            locked_types: Default::default(),
            validate_whiteboard_access: Default::default(),
            whiteboard_access: Default::default(),
        })
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Default)]
pub(crate) struct FindQuery {
    id: Option<Uuid>,
    classroom_id: Option<Uuid>,
}

impl FindQuery {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn by_id(self, id: Uuid) -> Self {
        Self {
            id: Some(id),
            ..self
        }
    }

    pub(crate) fn by_classroom_id(self, classroom_id: Uuid) -> Self {
        Self {
            classroom_id: Some(classroom_id),
            ..self
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Option<Object>> {
        use quaint::ast::{Comparable, Select};
        use quaint::visitor::{Postgres, Visitor};

        let mut q = Select::from_table("room");

        if self.id.is_some() {
            q = q.and_where("id".equals("_placeholder_"));
        }

        if self.classroom_id.is_some() {
            q = q.and_where("classroom_id".equals("_placeholder_"));
        }

        let (sql, _bindings) = Postgres::build(q);
        let mut query = sqlx::query_as::<_, DbObject>(&sql);

        if let Some(id) = self.id {
            query = query.bind(id);
        }

        if let Some(classroom_id) = self.classroom_id {
            query = query.bind(classroom_id);
        }

        query
            .fetch_optional(conn)
            .await?
            .map(|v| v.try_into())
            .transpose()
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
    classroom_id: Uuid,
    locked_types: HashMap<String, bool>,
    validate_whiteboard_access: Option<bool>,
    whiteboard_access: HashMap<AccountId, bool>,
}

impl InsertQuery {
    pub(crate) fn new(audience: &str, time: Time, classroom_id: Uuid) -> Self {
        Self {
            audience: audience.to_owned(),
            source_room_id: None,
            time,
            tags: None,
            preserve_history: true,
            classroom_id,
            locked_types: Default::default(),
            validate_whiteboard_access: Default::default(),
            whiteboard_access: Default::default(),
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

    pub(crate) fn validate_whiteboard_access(self, flag: bool) -> Self {
        Self {
            validate_whiteboard_access: Some(flag),
            ..self
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        let time: PgRange<DateTime<Utc>> = self.time.into();

        let locked_types = serde_json::to_value(&self.locked_types).unwrap();
        let whiteboard_access = serde_json::to_value(&self.whiteboard_access).unwrap();

        sqlx::query_as!(
            DbObject,
            r#"
            INSERT INTO room (
                audience, source_room_id, time, tags, preserve_history, classroom_id,
                    locked_types, validate_whiteboard_access, whiteboard_access)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING
                id,
                audience,
                source_room_id,
                time AS "time!: Time",
                tags,
                created_at,
                preserve_history,
                classroom_id,
                locked_types,
                validate_whiteboard_access,
                whiteboard_access
            "#,
            self.audience,
            self.source_room_id,
            Some(time),
            self.tags,
            self.preserve_history,
            self.classroom_id,
            locked_types,
            self.validate_whiteboard_access.unwrap_or(false),
            whiteboard_access,
        )
        .fetch_one(conn)
        .await?
        .try_into()
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateQuery {
    id: Uuid,
    time: Option<Time>,
    tags: Option<JsonValue>,
    classroom_id: Option<Uuid>,
    locked_types: Option<HashMap<String, bool>>,
    whiteboard_access: Option<HashMap<AccountId, bool>>,
}

impl UpdateQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self {
            id,
            time: None,
            tags: None,
            classroom_id: None,
            locked_types: None,
            whiteboard_access: None,
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

    pub(crate) fn locked_types(self, locked_types: HashMap<String, bool>) -> Self {
        Self {
            locked_types: Some(locked_types),
            ..self
        }
    }

    pub(crate) fn whiteboard_access(self, whiteboard_access: HashMap<AccountId, bool>) -> Self {
        Self {
            whiteboard_access: Some(whiteboard_access),
            ..self
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        let time: Option<PgRange<DateTime<Utc>>> = self.time.map(|t| t.into());

        // Delete false values from map not to accumulate them
        let locked_types = self.locked_types.map(|mut m| {
            m.retain(|_k, v| *v);
            serde_json::to_value(&m).unwrap()
        });
        let whiteboard_access = self.whiteboard_access.map(|mut m| {
            m.retain(|_k, v| *v);
            serde_json::to_value(&m).unwrap()
        });

        sqlx::query_as!(
            DbObject,
            r#"
            UPDATE room
            SET time = COALESCE($2, time),
                tags = COALESCE($3::JSON, tags),
                classroom_id = COALESCE($4, classroom_id),
                locked_types = COALESCE($5, locked_types),
                whiteboard_access = COALESCE($6, whiteboard_access)
            WHERE id = $1
            RETURNING
                id,
                audience,
                source_room_id,
                time AS "time!: Time",
                tags,
                created_at,
                preserve_history,
                classroom_id,
                locked_types,
                validate_whiteboard_access,
                whiteboard_access
            "#,
            self.id,
            time,
            self.tags,
            self.classroom_id,
            locked_types,
            whiteboard_access
        )
        .fetch_one(conn)
        .await?
        .try_into()
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

impl From<Time> for PgRange<DateTime<Utc>> {
    fn from(val: Time) -> Self {
        val.0
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
