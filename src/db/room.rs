use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
    fmt,
    ops::{Bound, RangeBounds},
    str::FromStr,
};

use chrono::{serde::ts_seconds, DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::postgres::{types::PgRange, PgConnection};
use svc_authn::AccountId;
use uuid::Uuid;

///////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Object {
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
    #[serde(skip)]
    kind: Option<ClassType>,
}

#[derive(Clone, Copy, Debug, sqlx::Type, PartialEq, Eq, Deserialize)]
#[sqlx(type_name = "class_type", rename_all = "lowercase")]
pub enum ClassType {
    Webinar,
    P2P,
    Minigroup,
}

impl fmt::Display for ClassType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Used to generate S3 bucket names to dump events
        let kind = match self {
            ClassType::Webinar => "webinar",
            ClassType::P2P => "p2p",
            ClassType::Minigroup => "minigroup",
        };
        write!(f, "{kind}")
    }
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
    kind: Option<ClassType>,
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
            kind,
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
            kind,
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
            kind,
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
            kind,
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
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn audience(&self) -> &str {
        &self.audience
    }

    pub fn source_room_id(&self) -> Option<Uuid> {
        self.source_room_id
    }

    pub fn time(&self) -> Result<RoomTime, String> {
        self.time.clone().try_into()
    }

    pub fn tags(&self) -> Option<&JsonValue> {
        self.tags.as_ref()
    }

    #[cfg(test)]
    pub fn preserve_history(&self) -> bool {
        self.preserve_history
    }

    pub fn kind(&self) -> Option<ClassType> {
        self.kind
    }

    pub fn classroom_id(&self) -> Uuid {
        self.classroom_id
    }

    pub fn locked_types(&self) -> &HashMap<String, bool> {
        &self.locked_types
    }

    pub fn validate_whiteboard_access(&self) -> bool {
        self.validate_whiteboard_access
    }

    pub fn whiteboard_access(&self) -> &HashMap<AccountId, bool> {
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

    pub fn is_closed(&self) -> bool {
        match self.time.0.end_bound() {
            Bound::Included(t) => *t < Utc::now(),
            Bound::Excluded(t) => *t <= Utc::now(),
            Bound::Unbounded => false,
        }
    }

    pub fn is_open(&self) -> bool {
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
pub struct Builder {
    id: Option<Uuid>,
    audience: Option<String>,
    source_room_id: Option<Uuid>,
    time: Option<Time>,
    tags: Option<JsonValue>,
    created_at: Option<DateTime<Utc>>,
    preserve_history: Option<bool>,
    classroom_id: Uuid,
    kind: Option<ClassType>,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn id(self, id: Uuid) -> Self {
        Self {
            id: Some(id),
            ..self
        }
    }

    pub fn audience(self, audience: String) -> Self {
        Self {
            audience: Some(audience),
            ..self
        }
    }

    pub fn source_room_id(self, source_room_id: Option<Uuid>) -> Self {
        Self {
            source_room_id,
            ..self
        }
    }

    pub fn time(self, time: Time) -> Self {
        Self {
            time: Some(time),
            ..self
        }
    }

    pub fn tags(self, tags: Option<JsonValue>) -> Self {
        Self { tags, ..self }
    }

    pub fn created_at(self, created_at: DateTime<Utc>) -> Self {
        Self {
            created_at: Some(created_at),
            ..self
        }
    }

    pub fn preserve_history(self, preserve_history: bool) -> Self {
        Self {
            preserve_history: Some(preserve_history),
            ..self
        }
    }

    pub fn classroom_id(self, classroom_id: Uuid) -> Self {
        Self {
            classroom_id,
            ..self
        }
    }

    pub fn kind(self, kind: Option<ClassType>) -> Self {
        Self { kind, ..self }
    }

    pub fn build(self) -> anyhow::Result<Object> {
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
            kind: self.kind,
        })
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Default)]
pub struct FindQuery {
    id: Option<Uuid>,
    classroom_id: Option<Uuid>,
}

impl FindQuery {
    pub fn by_id(id: Uuid) -> Self {
        Self {
            id: Some(id),
            ..Default::default()
        }
    }

    pub fn by_classroom_id(classroom_id: Uuid) -> Self {
        Self {
            classroom_id: Some(classroom_id),
            ..Default::default()
        }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Option<Object>> {
        sqlx::query_as!(
            DbObject,
            r#"
            SELECT
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
                whiteboard_access,
                kind AS "kind?: ClassType"
            FROM room
            WHERE ($1::uuid IS NULL OR id = $1)
                AND ($2::uuid IS NULL OR classroom_id = $2)
            "#,
            self.id,
            self.classroom_id,
        )
        .fetch_optional(conn)
        .await?
        .map(|v| v.try_into())
        .transpose()
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct InsertQuery {
    audience: String,
    source_room_id: Option<Uuid>,
    time: Time,
    tags: Option<JsonValue>,
    preserve_history: bool,
    classroom_id: Uuid,
    locked_types: HashMap<String, bool>,
    validate_whiteboard_access: Option<bool>,
    whiteboard_access: HashMap<AccountId, bool>,
    kind: Option<ClassType>,
}

impl InsertQuery {
    pub fn new(audience: &str, time: Time, classroom_id: Uuid) -> Self {
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
            kind: None,
        }
    }

    pub fn source_room_id(self, source_room_id: Uuid) -> Self {
        Self {
            source_room_id: Some(source_room_id),
            ..self
        }
    }

    pub fn tags(self, tags: JsonValue) -> Self {
        Self {
            tags: Some(tags),
            ..self
        }
    }

    pub fn preserve_history(self, preserve_history: bool) -> Self {
        Self {
            preserve_history,
            ..self
        }
    }

    pub fn validate_whiteboard_access(self, flag: bool) -> Self {
        Self {
            validate_whiteboard_access: Some(flag),
            ..self
        }
    }

    pub fn kind(self, kind: ClassType) -> Self {
        Self {
            kind: Some(kind),
            ..self
        }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        let time: PgRange<DateTime<Utc>> = self.time.into();

        let locked_types = serde_json::to_value(&self.locked_types).unwrap();
        let whiteboard_access = serde_json::to_value(&self.whiteboard_access).unwrap();

        sqlx::query_as!(
            DbObject,
            r#"
            INSERT INTO room (
                audience, source_room_id, time, tags, preserve_history, classroom_id,
                    locked_types, validate_whiteboard_access, whiteboard_access, kind)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
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
                whiteboard_access,
                kind AS "kind?: ClassType"
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
            self.kind as Option<ClassType>,
        )
        .fetch_one(conn)
        .await?
        .try_into()
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Deserialize)]
pub struct UpdateQuery {
    id: Uuid,
    time: Option<Time>,
    tags: Option<JsonValue>,
    classroom_id: Option<Uuid>,
    locked_types: Option<HashMap<String, bool>>,
    whiteboard_access: Option<HashMap<AccountId, bool>>,
}

impl UpdateQuery {
    pub fn new(id: Uuid) -> Self {
        Self {
            id,
            time: None,
            tags: None,
            classroom_id: None,
            locked_types: None,
            whiteboard_access: None,
        }
    }

    pub fn time(self, time: Option<Time>) -> Self {
        Self { time, ..self }
    }

    pub fn tags(self, tags: Option<JsonValue>) -> Self {
        Self { tags, ..self }
    }

    pub fn classroom_id(self, classroom_id: Option<Uuid>) -> Self {
        Self {
            classroom_id,
            ..self
        }
    }

    pub fn locked_types(self, locked_types: HashMap<String, bool>) -> Self {
        Self {
            locked_types: Some(locked_types),
            ..self
        }
    }

    pub fn whiteboard_access(self, whiteboard_access: HashMap<AccountId, bool>) -> Self {
        Self {
            whiteboard_access: Some(whiteboard_access),
            ..self
        }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
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
                whiteboard_access,
                kind AS "kind?: ClassType"
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
pub struct Time(PgRange<DateTime<Utc>>);

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

pub mod serde {
    pub mod time {
        use super::super::Time;
        use crate::serde::ts_seconds_bound_tuple;
        use serde::{de, ser};

        pub fn serialize<S>(value: &Time, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            ts_seconds_bound_tuple::serialize(&value.to_owned().into(), serializer)
        }

        pub fn deserialize<'de, D>(d: D) -> Result<Time, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            let time = ts_seconds_bound_tuple::deserialize(d)?;
            Ok(Time::from(time))
        }
    }
}
