use chrono::serde::{ts_milliseconds, ts_milliseconds_option};
use chrono::{DateTime, Duration, Utc};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::postgres::PgConnection;
use svc_agent::AgentId;
use uuid::Uuid;

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub(crate) struct Object {
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
}

impl Object {
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }

    #[cfg(test)]
    pub(crate) fn room_id(&self) -> Uuid {
        self.room_id
    }

    #[cfg(test)]
    pub(crate) fn kind(&self) -> &str {
        &self.kind
    }

    #[cfg(test)]
    pub(crate) fn set(&self) -> &str {
        &self.set
    }

    #[cfg(test)]
    pub(crate) fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn attribute(&self) -> Option<&str> {
        self.attribute.as_deref()
    }

    pub(crate) fn data(&self) -> &JsonValue {
        &self.data
    }

    pub(crate) fn occurred_at(&self) -> i64 {
        self.occurred_at
    }

    pub(crate) fn created_by(&self) -> &AgentId {
        &self.created_by
    }

    #[cfg(test)]
    pub(crate) fn original_occurred_at(&self) -> i64 {
        self.original_occurred_at
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Default)]
pub(crate) struct Builder {
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
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn room_id(self, room_id: Uuid) -> Self {
        Self {
            room_id: Some(room_id),
            ..self
        }
    }

    pub(crate) fn kind(self, kind: &str) -> Self {
        Self {
            kind: Some(kind.to_owned()),
            ..self
        }
    }

    pub(crate) fn set(self, set: &str) -> Self {
        Self {
            set: Some(set.to_owned()),
            ..self
        }
    }

    pub(crate) fn label(self, label: &str) -> Self {
        Self {
            label: Some(label.to_owned()),
            ..self
        }
    }

    pub(crate) fn attribute(self, attribute: &str) -> Self {
        Self {
            attribute: Some(attribute.to_owned()),
            ..self
        }
    }

    pub(crate) fn data(self, data: &JsonValue) -> Self {
        Self {
            data: Some(data.to_owned()),
            ..self
        }
    }

    pub(crate) fn occurred_at(self, occurred_at: i64) -> Self {
        Self {
            occurred_at: Some(occurred_at),
            ..self
        }
    }

    pub(crate) fn created_by(self, created_by: &AgentId) -> Self {
        Self {
            created_by: Some(created_by.to_owned()),
            ..self
        }
    }

    pub(crate) fn build(self) -> Result<Object, &'static str> {
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
            created_by,
            created_at: Utc::now(),
            deleted_at: None,
            original_occurred_at: occurred_at,
        })
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Direction {
    Forward,
    Backward,
}

impl Default for Direction {
    fn default() -> Self {
        Self::Forward
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
enum KindFilter {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Default)]
pub(crate) struct ListQuery<'a> {
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
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn room_id(self, room_id: Uuid) -> Self {
        Self {
            room_id: Some(room_id),
            ..self
        }
    }

    pub(crate) fn kind(self, kind: String) -> Self {
        Self {
            kind: Some(KindFilter::Single(kind)),
            ..self
        }
    }

    pub(crate) fn kinds(self, kinds: Vec<String>) -> Self {
        Self {
            kind: Some(KindFilter::Multiple(kinds)),
            ..self
        }
    }

    pub(crate) fn set(self, set: &'a str) -> Self {
        Self {
            set: Some(set),
            ..self
        }
    }

    pub(crate) fn label(self, label: &'a str) -> Self {
        Self {
            label: Some(label),
            ..self
        }
    }

    pub(crate) fn attribute(self, attribute: &'a str) -> Self {
        Self {
            attribute: Some(attribute),
            ..self
        }
    }

    pub(crate) fn last_occurred_at(self, last_occurred_at: i64) -> Self {
        Self {
            last_occurred_at: Some(last_occurred_at),
            ..self
        }
    }

    pub(crate) fn direction(self, direction: Direction) -> Self {
        Self { direction, ..self }
    }

    pub(crate) fn limit(self, limit: usize) -> Self {
        Self {
            limit: Some(limit),
            ..self
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Vec<Object>> {
        use quaint::ast::{Comparable, Orderable, ParameterizedValue, Select};
        use quaint::visitor::{Postgres, Visitor};

        let mut q = Select::from_table("event").so_that("deleted_at".is_null());

        if let Some(room_id) = self.room_id {
            q = q.and_where("room_id".equals(room_id));
        }

        q = match self.kind {
            Some(KindFilter::Single(ref kind)) => q.and_where("kind".equals(kind.as_str())),
            Some(KindFilter::Multiple(ref kinds)) => {
                let kinds = kinds.iter().map(|k| k.as_str()).collect::<Vec<&str>>();
                q.and_where("kind".in_selection(kinds))
            }
            None => q,
        };

        if let Some(set) = self.set {
            q = q.and_where("set".equals(set));
        }

        if let Some(label) = self.label {
            q = q.and_where("label".equals(label));
        }

        if let Some(attribute) = self.attribute {
            q = q.and_where("attribute".equals(attribute));
        }

        if let Some(limit) = self.limit {
            q = q.limit(limit);
        }

        q = match self.direction {
            Direction::Forward => {
                if let Some(last_occurred_at) = self.last_occurred_at {
                    q = q.and_where("occurred_at".greater_than(last_occurred_at));
                }

                q.order_by("occurred_at").order_by("created_at")
            }
            Direction::Backward => {
                if let Some(last_occurred_at) = self.last_occurred_at {
                    q = q.and_where("occurred_at".less_than(last_occurred_at));
                }

                q.order_by("occurred_at".descend())
                    .order_by("created_at".descend())
            }
        };

        let (sql, bindings) = Postgres::build(q);
        let mut query = sqlx::query_as(&sql);

        for binding in bindings {
            query = match binding {
                ParameterizedValue::Integer(value) => query.bind(value),
                ParameterizedValue::Text(value) => query.bind(value.to_string()),
                ParameterizedValue::Uuid(value) => query.bind(value),
                _ => query,
            }
        }

        query.fetch_all(conn).await
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub(crate) struct InsertQuery {
    room_id: Uuid,
    kind: String,
    set: String,
    label: Option<String>,
    data: JsonValue,
    attribute: Option<String>,
    occurred_at: i64,
    created_by: AgentId,
    created_at: Option<DateTime<Utc>>,
}

impl InsertQuery {
    pub(crate) fn new(
        room_id: Uuid,
        kind: String,
        data: JsonValue,
        occurred_at: i64,
        created_by: AgentId,
    ) -> Self {
        Self {
            room_id,
            set: kind.clone(),
            kind,
            label: None,
            attribute: None,
            data,
            occurred_at,
            created_by,
            created_at: None,
        }
    }

    pub(crate) fn set(self, set: String) -> Self {
        Self { set, ..self }
    }

    pub(crate) fn label(self, label: String) -> Self {
        Self {
            label: Some(label),
            ..self
        }
    }

    pub(crate) fn attribute(self, attribute: String) -> Self {
        Self {
            attribute: Some(attribute),
            ..self
        }
    }

    #[cfg(test)]
    pub(crate) fn created_at(self, created_at: DateTime<Utc>) -> Self {
        Self {
            created_at: Some(created_at),
            ..self
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        sqlx::query_as!(
            Object,
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
                created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING
                id,
                room_id,
                kind,
                set,
                label,
                attribute,
                data,
                occurred_at,
                created_by AS "created_by!: AgentId",
                created_at,
                deleted_at,
                original_occurred_at
            "#,
            self.room_id,
            self.set,
            self.kind,
            self.label,
            self.attribute,
            self.data,
            self.occurred_at,
            self.created_by as AgentId,
            self.created_at.unwrap_or_else(|| Utc::now()),
        )
        .fetch_one(conn)
        .await
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub(crate) struct DeleteQuery<'a> {
    room_id: Uuid,
    kind: &'a str,
}

impl<'a> DeleteQuery<'a> {
    pub(crate) fn new(room_id: Uuid, kind: &'a str) -> Self {
        Self { room_id, kind }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<()> {
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

#[derive(Clone)]
pub(crate) struct SetStateQuery<'a> {
    room_id: Uuid,
    set: String,
    attribute: Option<&'a str>,
    occurred_at: Option<i64>,
    original_occurred_at: i64,
    limit: i64,
}

impl<'a> SetStateQuery<'a> {
    pub(crate) fn new(room_id: Uuid, set: String, original_occurred_at: i64, limit: i64) -> Self {
        Self {
            room_id,
            set,
            attribute: None,
            occurred_at: None,
            original_occurred_at,
            limit,
        }
    }

    pub(crate) fn occurred_at(self, occurred_at: i64) -> Self {
        Self {
            occurred_at: Some(occurred_at),
            ..self
        }
    }

    pub(crate) fn attribute(self, attribute: &'a str) -> Self {
        Self {
            attribute: Some(attribute),
            ..self
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Vec<Object>> {
        if let Some(attribute) = self.attribute {
            sqlx::query_as!(
                Object,
                r#"
                SELECT
                    id,
                    room_id,
                    kind,
                    set,
                    label,
                    attribute,
                    data,
                    occurred_at,
                    created_by as "created_by!: AgentId",
                    created_at,
                    deleted_at,
                    original_occurred_at
                FROM (
                    SELECT DISTINCT ON(original_occurred_at, label)
                        *,
                        ROW_NUMBER() OVER (
                            PARTITION BY room_id, set, label
                            ORDER BY occurred_at DESC
                        ) AS reverse_ordinal
                    FROM event
                    WHERE deleted_at IS NULL
                    AND   room_id = $1
                    AND   set = $2
                    AND   original_occurred_at < $4
                    AND   occurred_at < COALESCE($5, 9223372036854775807)
                    ORDER BY original_occurred_at DESC, label ASC, occurred_at DESC
                ) AS q
                WHERE reverse_ordinal = 1
                AND   attribute = $3
                LIMIT $6
                "#,
                self.room_id,
                self.set,
                attribute,
                self.original_occurred_at,
                self.occurred_at,
                self.limit,
            )
            .fetch_all(conn)
            .await
        } else {
            sqlx::query_as!(
                Object,
                r#"
                SELECT DISTINCT ON(original_occurred_at, label)
                    id,
                    room_id,
                    kind,
                    set,
                    label,
                    attribute,
                    data,
                    occurred_at,
                    created_by as "created_by!: AgentId",
                    created_at,
                    deleted_at,
                    original_occurred_at
                FROM event
                WHERE deleted_at IS NULL
                AND   room_id = $1
                AND   set = $2
                AND   original_occurred_at < $3
                AND   occurred_at < COALESCE($4, 9223372036854775807)
                ORDER BY original_occurred_at DESC, label ASC, occurred_at DESC
                LIMIT $5
                "#,
                self.room_id,
                self.set,
                self.original_occurred_at,
                self.occurred_at,
                self.limit,
            )
            .fetch_all(conn)
            .await
        }
    }

    pub(crate) async fn total_count(&self, conn: &mut PgConnection) -> sqlx::Result<i64> {
        sqlx::query!(
            "
            SELECT COUNT(DISTINCT label) AS total
            FROM event
            WHERE deleted_at IS NULL
            AND   room_id = $1
            AND   set = $2
            AND   ($3::TEXT IS NULL OR attribute = $3::TEXT)
            AND   original_occurred_at < $4
            AND   occurred_at < COALESCE($5, 9223372036854775807)
            ",
            self.room_id,
            self.set,
            self.attribute,
            self.original_occurred_at,
            self.occurred_at,
        )
        .fetch_one(conn)
        .await
        .map(|r| r.total.unwrap_or(0))
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub(crate) struct OriginalEventQuery {
    room_id: Uuid,
    set: String,
    label: String,
}

impl OriginalEventQuery {
    pub(crate) fn new(room_id: Uuid, set: String, label: String) -> Self {
        Self {
            room_id,
            set,
            label,
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Option<Object>> {
        sqlx::query_as!(
            Object,
            r#"
            SELECT
                id,
                room_id,
                kind,
                set,
                label,
                attribute,
                data,
                occurred_at,
                created_by as "created_by!: AgentId",
                created_at,
                deleted_at,
                original_occurred_at
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
        .await
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub(crate) struct VacuumQuery {
    max_history_size: usize,
    max_history_lifetime: Duration,
}

impl VacuumQuery {
    pub(crate) fn new(max_history_size: usize, max_history_lifetime: Duration) -> Self {
        Self {
            max_history_size,
            max_history_lifetime,
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<()> {
        sqlx::query!(
            r#"
            DELETE FROM event
            WHERE id IN (
                SELECT id
                FROM (
                    SELECT
                        e.id,
                        e.created_at,
                        ROW_NUMBER() OVER (
                            PARTITION BY e.room_id, e.set, e.label
                            ORDER BY e.occurred_at DESC
                        ) AS history_depth
                    FROM event AS e
                    INNER JOIN room AS r
                    ON r.id = e.room_id
                    WHERE r.preserve_history = 'f'
                ) AS q
                WHERE history_depth > $1
                OR created_at < NOW() - INTERVAL '1 second' * $2
            )
            "#,
            self.max_history_size as i64,
            self.max_history_lifetime.num_seconds() as i64,
        )
        .execute(conn)
        .await
        .map(|_| ())
    }
}
