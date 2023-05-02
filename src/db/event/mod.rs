use std::convert::TryFrom;

use chrono::serde::{ts_milliseconds, ts_milliseconds_option};
use chrono::{DateTime, Duration, Utc};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::postgres::PgConnection;
use svc_agent::{AccountId, AgentId};
use uuid::Uuid;

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Object {
    id: Uuid,
    room_id: Uuid,
    #[serde(rename = "type")]
    kind: String,
    set: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    attribute: Option<String>,
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
    original_created_by: AgentId,
    removed: bool,
}

impl Object {
    pub fn id(&self) -> Uuid {
        self.id
    }

    #[cfg(test)]
    pub fn room_id(&self) -> Uuid {
        self.room_id
    }

    #[cfg(test)]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    #[cfg(test)]
    pub fn set(&self) -> &str {
        &self.set
    }

    #[cfg(test)]
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    #[cfg(test)]
    pub fn attribute(&self) -> Option<&str> {
        self.attribute.as_deref()
    }

    pub fn data(&self) -> &JsonValue {
        &self.data
    }

    pub fn occurred_at(&self) -> i64 {
        self.occurred_at
    }

    pub fn created_by(&self) -> &AgentId {
        &self.created_by
    }

    #[cfg(test)]
    pub fn original_occurred_at(&self) -> i64 {
        self.original_occurred_at
    }

    #[cfg(test)]
    pub fn removed(&self) -> bool {
        self.removed
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct RawObject {
    id: Uuid,
    room_id: Uuid,
    #[serde(rename = "type")]
    kind: String,
    set: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    attribute: Option<String>,
    data: Option<JsonValue>,
    binary_data: Option<PostcardBin<CompactEvent>>,
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
    original_created_by: AgentId,
    removed: bool,
}

impl TryFrom<RawObject> for Object {
    type Error = sqlx::Error;

    fn try_from(raw: RawObject) -> Result<Object, Self::Error> {
        let data = match raw.binary_data {
            Some(binary) => binary
                .into_inner()
                .into_json()
                .map_err(|err| sqlx::Error::Decode(Box::new(err)))?,
            None => raw.data.ok_or_else(|| {
                sqlx::Error::Decode("data should be specified if binary_data is missing".into())
            })?,
        };

        Ok(Object {
            id: raw.id,
            room_id: raw.room_id,
            kind: raw.kind,
            set: raw.set,
            label: raw.label,
            attribute: raw.attribute,
            data,
            occurred_at: raw.occurred_at,
            created_by: raw.created_by,
            created_at: raw.created_at,
            deleted_at: raw.deleted_at,
            original_occurred_at: raw.original_occurred_at,
            original_created_by: raw.original_created_by,
            removed: raw.removed,
        })
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Default)]
pub struct Builder {
    room_id: Option<Uuid>,
    kind: Option<String>,
    set: Option<String>,
    label: Option<String>,
    data: Option<JsonValue>,
    occurred_at: Option<i64>,
    created_by: Option<AgentId>,
    attribute: Option<String>,
}

impl Builder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn room_id(self, room_id: Uuid) -> Self {
        Self {
            room_id: Some(room_id),
            ..self
        }
    }

    pub fn kind(self, kind: &str) -> Self {
        Self {
            kind: Some(kind.to_owned()),
            ..self
        }
    }

    pub fn set(self, set: &str) -> Self {
        Self {
            set: Some(set.to_owned()),
            ..self
        }
    }

    pub fn label(self, label: &str) -> Self {
        Self {
            label: Some(label.to_owned()),
            ..self
        }
    }

    pub fn attribute(self, attribute: &str) -> Self {
        Self {
            attribute: Some(attribute.to_owned()),
            ..self
        }
    }

    pub fn data(self, data: &JsonValue) -> Self {
        Self {
            data: Some(data.to_owned()),
            ..self
        }
    }

    pub fn occurred_at(self, occurred_at: i64) -> Self {
        Self {
            occurred_at: Some(occurred_at),
            ..self
        }
    }

    pub fn created_by(self, created_by: &AgentId) -> Self {
        Self {
            created_by: Some(created_by.to_owned()),
            ..self
        }
    }

    pub fn build(self) -> Result<Object, &'static str> {
        let room_id = self.room_id.ok_or("Missing `room_id`")?;
        let kind = self.kind.ok_or("Missing `kind`")?;
        let set = self.set.unwrap_or_else(|| kind.clone());
        let data = self.data.ok_or("Missing `data`")?;
        let occurred_at = self.occurred_at.ok_or("Missing `occurred_at`")?;
        let created_by = self.created_by.ok_or("Missing `created_by`")?;

        Ok(Object {
            id: Uuid::new_v4(),
            room_id,
            kind,
            set,
            label: self.label,
            attribute: self.attribute,
            data,
            occurred_at,
            created_by: created_by.clone(),
            created_at: Utc::now(),
            deleted_at: None,
            original_occurred_at: occurred_at,
            original_created_by: created_by,
            removed: false,
        })
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Forward,
    Backward,
}

impl Default for Direction {
    fn default() -> Self {
        Self::Forward
    }
}

///////////////////////////////////////////////////////////////////////////////
const DEFAULT_LIST_LIMIT: usize = 1000;

#[derive(Debug)]
enum KindFilter {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Default)]
pub struct ListQuery<'a> {
    room_id: Option<Uuid>,
    kind: Option<KindFilter>,
    set: Option<&'a str>,
    label: Option<&'a str>,
    attribute: Option<&'a str>,
    last_occurred_at: Option<i64>,
    direction: Direction,
    limit: Option<usize>,
}

impl<'a> ListQuery<'a> {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn room_id(self, room_id: Uuid) -> Self {
        Self {
            room_id: Some(room_id),
            ..self
        }
    }

    pub fn kind(self, kind: String) -> Self {
        Self {
            kind: Some(KindFilter::Single(kind)),
            ..self
        }
    }

    pub fn kinds(self, kinds: Vec<String>) -> Self {
        Self {
            kind: Some(KindFilter::Multiple(kinds)),
            ..self
        }
    }

    pub fn set(self, set: &'a str) -> Self {
        Self {
            set: Some(set),
            ..self
        }
    }

    pub fn label(self, label: &'a str) -> Self {
        Self {
            label: Some(label),
            ..self
        }
    }

    pub fn attribute(self, attribute: &'a str) -> Self {
        Self {
            attribute: Some(attribute),
            ..self
        }
    }

    pub fn last_occurred_at(self, last_occurred_at: i64) -> Self {
        Self {
            last_occurred_at: Some(last_occurred_at),
            ..self
        }
    }

    pub fn direction(self, direction: Direction) -> Self {
        Self { direction, ..self }
    }

    pub fn limit(self, limit: usize) -> Self {
        Self {
            limit: Some(limit),
            ..self
        }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Vec<Object>> {
        use serde_json::Value;

        let limit = self.limit.unwrap_or(DEFAULT_LIST_LIMIT);
        let kinds = match self.kind {
            Some(KindFilter::Single(ref kind)) => vec![kind.clone()],
            Some(KindFilter::Multiple(ref kinds)) => kinds.clone(),
            None => vec![],
        };

        match self.direction {
            Direction::Forward => {
                sqlx::query_as!(
                    Object,
                    r#"
                    SELECT
                        id,
                        room_id,
                        kind,
                        set,
                        label,
                        data                AS "data!: Value",
                        occurred_at,
                        created_at,
                        deleted_at,
                        created_by          AS "created_by!: AgentId",
                        original_created_by AS "original_created_by!: AgentId",
                        original_occurred_at,
                        removed,
                        attribute
                    FROM event
                    WHERE deleted_at IS NULL
                        AND ($1::uuid IS NULL OR (room_id = $1::uuid))
                        AND ($2::text IS NULL OR (set = $2::text))
                        AND ($3::text IS NULL OR (label = $3::text))
                        AND ($4::text IS NULL OR (attribute = $4::text))
                        AND (array_length($5::text[], 1) = 0 OR (kind = ANY($5)))
                        AND ($6::bigint IS NULL OR (occurred_at > $6::bigint))
                    ORDER BY occurred_at ASC, created_at ASC LIMIT $7
                    "#,
                    self.room_id,
                    self.set,
                    self.label,
                    self.attribute,
                    kinds.as_slice(),
                    self.last_occurred_at,
                    limit as i64,
                )
                .fetch_all(conn)
                .await
            }
            Direction::Backward => {
                sqlx::query_as!(
                    Object,
                    r#"
                    SELECT
                        id,
                        room_id,
                        kind,
                        set,
                        label,
                        data                AS "data!: Value",
                        occurred_at,
                        created_at,
                        deleted_at,
                        created_by          AS "created_by!: AgentId",
                        original_created_by AS "original_created_by!: AgentId",
                        original_occurred_at,
                        removed,
                        attribute
                    FROM event
                    WHERE deleted_at IS NULL
                        AND ($1::uuid IS NULL OR (room_id = $1::uuid))
                        AND ($2::text IS NULL OR (set = $2::text))
                        AND ($3::text IS NULL OR (label = $3::text))
                        AND ($4::text IS NULL OR (attribute = $4::text))
                        AND (array_length($5::text[], 1) = 0 OR (kind = ANY($5)))
                        AND ($6::bigint IS NULL OR (occurred_at <= $6::bigint))
                    ORDER BY occurred_at DESC, created_at DESC LIMIT $7
                    "#,
                    self.room_id,
                    self.set,
                    self.label,
                    self.attribute,
                    kinds.as_slice(),
                    self.last_occurred_at,
                    limit as i64,
                )
                .fetch_all(conn)
                .await
            }
        }
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct InsertQuery {
    room_id: Uuid,
    kind: String,
    set: String,
    label: Option<String>,
    data: Option<JsonValue>,
    binary_data: Option<PostcardBin<CompactEvent>>,
    attribute: Option<String>,
    occurred_at: i64,
    created_by: AgentId,
    created_at: Option<DateTime<Utc>>,
    removed: bool,
}

impl InsertQuery {
    pub fn new(
        room_id: Uuid,
        kind: String,
        data: JsonValue,
        occurred_at: i64,
        created_by: AgentId,
    ) -> Result<Self, anyhow::Error> {
        let (data, binary_data) = match kind.as_str() {
            "draw" => (None, Some(PostcardBin::new(CompactEvent::from_json(data)?))),
            _ => (Some(data), None),
        };

        Ok(Self {
            room_id,
            set: kind.clone(),
            kind,
            label: None,
            attribute: None,
            data,
            binary_data,
            occurred_at,
            created_by,
            created_at: None,
            removed: false,
        })
    }

    pub fn set(self, set: String) -> Self {
        Self { set, ..self }
    }

    pub fn label(self, label: String) -> Self {
        Self {
            label: Some(label),
            ..self
        }
    }

    pub fn attribute(self, attribute: String) -> Self {
        Self {
            attribute: Some(attribute),
            ..self
        }
    }

    pub fn removed(self, removed: bool) -> Self {
        Self { removed, ..self }
    }

    #[cfg(test)]
    pub fn created_at(self, created_at: DateTime<Utc>) -> Self {
        Self {
            created_at: Some(created_at),
            ..self
        }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        let raw = match self.created_at {
            Some(created_at) => {
                sqlx::query_as!(
                    RawObject,
                    r#"
                    INSERT INTO event (
                        room_id,
                        set,
                        kind,
                        label,
                        attribute,
                        data,
                        occurred_at,
                        created_by,
                        created_at,
                        removed,
                        binary_data
                    )
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                    RETURNING
                        id,
                        room_id,
                        kind,
                        set,
                        label,
                        attribute,
                        data,
                        binary_data AS "binary_data: PostcardBin<CompactEvent>",
                        occurred_at,
                        created_by AS "created_by!: AgentId",
                        created_at,
                        deleted_at,
                        original_occurred_at,
                        original_created_by as "original_created_by: AgentId",
                        removed
                    "#,
                    self.room_id,
                    self.set,
                    self.kind,
                    self.label,
                    self.attribute,
                    self.data,
                    self.occurred_at,
                    self.created_by as AgentId,
                    created_at,
                    self.removed,
                    self.binary_data as Option<PostcardBin<CompactEvent>>,
                )
                .fetch_one(conn)
                .await?
            }
            None => {
                sqlx::query_as!(
                    RawObject,
                    r#"
                INSERT INTO event (
                    room_id,
                    set,
                    kind,
                    label,
                    attribute,
                    data,
                    occurred_at,
                    created_by,
                    removed,
                    binary_data
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                RETURNING
                    id,
                    room_id,
                    kind,
                    set,
                    label,
                    attribute,
                    data,
                    binary_data AS "binary_data: PostcardBin<CompactEvent>",
                    occurred_at,
                    created_by AS "created_by!: AgentId",
                    created_at,
                    deleted_at,
                    original_occurred_at,
                    original_created_by as "original_created_by: AgentId",
                    removed
                "#,
                    self.room_id,
                    self.set,
                    self.kind,
                    self.label,
                    self.attribute,
                    self.data,
                    self.occurred_at,
                    self.created_by as AgentId,
                    self.removed,
                    self.binary_data as Option<PostcardBin<CompactEvent>>,
                )
                .fetch_one(conn)
                .await?
            }
        };

        Object::try_from(raw)
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct DeleteQuery<'a> {
    room_id: Uuid,
    kind: &'a str,
}

impl<'a> DeleteQuery<'a> {
    pub fn new(room_id: Uuid, kind: &'a str) -> Self {
        Self { room_id, kind }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<()> {
        sqlx::query!(
            "
            DELETE FROM event
            WHERE deleted_at IS NULL
            AND   room_id = $1
            AND   kind = $2
            ",
            self.room_id,
            self.kind,
        )
        .execute(conn)
        .await
        .map(|_| ())
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct OriginalEventQuery {
    room_id: Uuid,
    set: String,
    label: String,
}

impl OriginalEventQuery {
    pub fn new(room_id: Uuid, set: String, label: String) -> Self {
        Self {
            room_id,
            set,
            label,
        }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Option<Object>> {
        let raw = sqlx::query_as!(
            RawObject,
            r#"
            SELECT
                id,
                room_id,
                kind,
                set,
                label,
                attribute,
                data,
                binary_data as "binary_data: PostcardBin<CompactEvent>",
                occurred_at,
                created_by as "created_by!: AgentId",
                created_at,
                deleted_at,
                original_occurred_at,
                original_created_by as "original_created_by: AgentId",
                removed
            FROM event
            WHERE deleted_at IS NULL
            AND   room_id = $1
            AND   set = $2
            AND   label = $3
            ORDER BY occurred_at
            LIMIT 1
            "#,
            self.room_id,
            self.set,
            self.label,
        )
        .fetch_optional(conn)
        .await?;

        match raw {
            Some(raw) => Ok(Some(Object::try_from(raw)?)),
            None => Ok(None),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct VacuumQuery {
    max_history_size: usize,
    max_history_lifetime: Duration,
    max_deleted_lifetime: Duration,
}

impl VacuumQuery {
    pub fn new(
        max_history_size: usize,
        max_history_lifetime: Duration,
        max_deleted_lifetime: Duration,
    ) -> Self {
        Self {
            max_history_size,
            max_history_lifetime,
            max_deleted_lifetime,
        }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<()> {
        sqlx::query!(
            r#"
            DELETE FROM event
            WHERE id IN (
                -- Exclude preserved rooms and calculate reverse ordinal (history depth).
                WITH sub AS (
                    SELECT
                        e.*,
                        ROW_NUMBER() OVER (
                            PARTITION BY e.room_id, e.set, e.label
                            ORDER BY e.occurred_at DESC
                        ) AS reverse_ordinal
                    FROM event AS e
                    INNER JOIN room AS r
                    ON r.id = e.room_id
                    WHERE r.preserve_history = 'f'
                )

                -- Too deep history.
                SELECT id
                FROM sub
                WHERE reverse_ordinal > $1

                UNION ALL

                -- Too old history.
                SELECT id
                FROM sub
                WHERE reverse_ordinal > 1
                AND created_at < NOW() - INTERVAL '1 second' * $2

                UNION ALL

                -- Too old deleted labels.
                SELECT e.id
                FROM sub
                INNER JOIN event AS e
                ON  e.room_id = sub.room_id
                AND e.set = sub.set
                AND e.label = sub.label
                WHERE e.deleted_at IS NULL
                AND   sub.attribute = 'deleted'
                AND   sub.reverse_ordinal = 1
                AND   sub.created_at < NOW() - INTERVAL '1 second' * $3
            )
            "#,
            self.max_history_size as i64,
            self.max_history_lifetime.num_seconds() as i64,
            self.max_deleted_lifetime.num_seconds() as i64,
        )
        .execute(conn)
        .await
        .map(|_| ())
    }
}

pub enum AgentAction {
    Left,
    Enter,
}

impl AgentAction {
    fn as_str(&self) -> &'static str {
        match self {
            AgentAction::Left => "agent_left",
            AgentAction::Enter => "agent_enter",
        }
    }
}

pub async fn insert_agent_action(
    room: &super::room::Object,
    action: AgentAction,
    agent_id: &AgentId,
    conn: &mut PgConnection,
) -> std::result::Result<(), anyhow::Error> {
    let occurred_at = match room.time().as_ref().map(|t| t.start()) {
        Ok(&opened_at) => (Utc::now() - opened_at)
            .num_nanoseconds()
            .unwrap_or(std::i64::MAX),
        _ => {
            return Err(anyhow!("Invalid room time"));
        }
    };

    let action = action.as_str();
    InsertQuery::new(
        room.id(),
        action.to_owned(),
        JsonValue::Null,
        occurred_at,
        agent_id.to_owned(),
    )?
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn insert_account_ban_event(
    room: &super::room::Object,
    banned_user: &AccountId,
    value: bool,
    reason: Option<String>,
    agent_id: &AgentId,
    conn: &mut PgConnection,
) -> anyhow::Result<()> {
    let occurred_at = match room.time().as_ref().map(|t| t.start()) {
        Ok(&opened_at) => (Utc::now() - opened_at)
            .num_nanoseconds()
            .unwrap_or(std::i64::MAX),
        _ => {
            return Err(anyhow!("Invalid room time"));
        }
    };

    InsertQuery::new(
        room.id(),
        "account_ban".to_string(),
        serde_json::json!({ "account_id": banned_user.to_owned(), "value": value, "reason": reason }),
        occurred_at,
        agent_id.to_owned(),
    )?
    .execute(conn)
    .await?;
    Ok(())
}

mod binary_encoding;
mod schema;
mod set_state;

pub use self::binary_encoding::PostcardBin;
pub use schema::CompactEvent;
pub use set_state::Query as SetStateQuery;
