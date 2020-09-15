use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::postgres::PgConnection;
use svc_agent::AgentId;
use uuid::Uuid;

use crate::db::room::{Builder as RoomBuilder, Object as Room, Time as RoomTime};

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename = "change_type")]
pub(crate) enum ChangeType {
    #[sqlx(rename = "addition")]
    Addition,
    #[sqlx(rename = "modification")]
    Modification,
    #[sqlx(rename = "removal")]
    Removal,
}

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub(crate) struct Object {
    id: Uuid,
    edition_id: Uuid,
    #[serde(rename = "type")]
    kind: ChangeType,
    event_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "event_type")]
    event_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_set: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_data: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_occurred_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_created_by: Option<AgentId>,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
}

impl Object {
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }

    #[cfg(test)]
    pub(crate) fn edition_id(&self) -> Uuid {
        self.edition_id
    }

    #[cfg(test)]
    pub(crate) fn kind(&self) -> ChangeType {
        self.kind
    }

    #[cfg(test)]
    pub(crate) fn event_id(&self) -> Option<Uuid> {
        self.event_id
    }

    pub(crate) fn event_data(&self) -> &Option<JsonValue> {
        &self.event_data
    }

    pub(crate) fn event_occurred_at(&self) -> Option<i64> {
        self.event_occurred_at
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub(crate) struct FindWithRoomQuery {
    id: Uuid,
}

impl FindWithRoomQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self { id }
    }

    pub(crate) async fn execute(
        self,
        conn: &mut PgConnection,
    ) -> sqlx::Result<Option<(Object, Room)>> {
        let maybe_row = sqlx::query!(
            r#"
                SELECT
                    c.id                 AS change_id,
                    c.edition_id         AS change_edition_id,
                    c.kind               AS "change_kind!: ChangeType",
                    c.event_id           AS change_event_id,
                    c.event_kind         AS change_event_kind,
                    c.event_set          AS change_event_set,
                    c.event_label        AS change_event_label,
                    c.event_data         AS change_event_data,
                    c.event_occurred_at  AS change_event_occurred_at,
                    c.event_created_by   AS "change_event_created_by?: AgentId",
                    c.created_at         AS change_created_at,
                    r.id                 AS room_id,
                    r.audience           AS room_audience,
                    r.source_room_id     AS room_source_room_id,
                    r.time               AS "room_time!: RoomTime",
                    r.tags               AS room_tags,
                    r.created_at         AS room_created_at
                FROM change AS c
                INNER JOIN edition AS e
                ON e.id = c.edition_id
                INNER JOIN room AS r
                ON r.id = e.source_room_id
                WHERE c.id = $1
                "#,
            self.id
        )
        .fetch_optional(conn)
        .await?;

        match maybe_row {
            None => Ok(None),
            Some(row) => {
                let change = Object {
                    id: row.change_id,
                    edition_id: row.change_edition_id,
                    kind: row.change_kind,
                    event_id: row.change_event_id,
                    event_kind: row.change_event_kind,
                    event_set: row.change_event_set,
                    event_label: row.change_event_label,
                    event_data: row.change_event_data,
                    event_occurred_at: row.change_event_occurred_at,
                    event_created_by: row.change_event_created_by,
                    created_at: row.change_created_at,
                };

                let room = RoomBuilder::new()
                    .id(row.room_id)
                    .audience(row.room_audience)
                    .source_room_id(row.room_source_room_id)
                    .time(row.room_time)
                    .tags(row.room_tags)
                    .created_at(row.room_created_at)
                    .build()
                    .map_err(|err| sqlx::Error::Decode(err.into()))?;

                Ok(Some((change, room)))
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub(crate) struct InsertQuery {
    edition_id: Uuid,
    kind: ChangeType,
    event_id: Option<Uuid>,
    event_kind: Option<String>,
    event_set: Option<String>,
    event_label: Option<String>,
    event_data: Option<JsonValue>,
    event_occurred_at: Option<i64>,
    event_created_by: Option<AgentId>,
}

impl InsertQuery {
    pub(crate) fn new(edition_id: Uuid, kind: ChangeType) -> Self {
        Self {
            event_id: None,
            event_kind: None,
            event_set: None,
            event_label: None,
            event_data: None,
            event_occurred_at: None,
            event_created_by: None,
            edition_id,
            kind,
        }
    }

    pub(crate) fn event_id(self, event_id: Uuid) -> Self {
        Self {
            event_id: Some(event_id),
            ..self
        }
    }

    pub(crate) fn event_kind(self, kind: String) -> Self {
        Self {
            event_kind: Some(kind),
            ..self
        }
    }

    pub(crate) fn event_set(self, set: Option<String>) -> Self {
        Self {
            event_set: set,
            ..self
        }
    }

    pub(crate) fn event_label(self, label: Option<String>) -> Self {
        Self {
            event_label: label,
            ..self
        }
    }

    pub(crate) fn event_data(self, data: JsonValue) -> Self {
        Self {
            event_data: Some(data),
            ..self
        }
    }

    pub(crate) fn event_occurred_at(self, occurred_at: i64) -> Self {
        Self {
            event_occurred_at: Some(occurred_at),
            ..self
        }
    }

    pub(crate) fn event_created_by(self, created_by: AgentId) -> Self {
        Self {
            event_created_by: Some(created_by),
            ..self
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        sqlx::query_as!(
            Object,
            r#"
            INSERT INTO change (
                event_id,
                event_kind,
                event_set,
                event_label,
                event_data,
                event_occurred_at,
                event_created_by,
                edition_id,
                kind
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING
                id,
                edition_id,
                kind               AS "kind!: ChangeType",
                event_id,
                event_kind,
                event_set,
                event_label,
                event_data,
                event_occurred_at,
                event_created_by   AS "event_created_by?: AgentId",
                created_at
            "#,
            self.event_id,
            self.event_kind,
            self.event_set,
            self.event_label,
            self.event_data,
            self.event_occurred_at,
            self.event_created_by as Option<AgentId>,
            self.edition_id,
            self.kind as ChangeType,
        )
        .fetch_one(conn)
        .await
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub(crate) struct ListQuery {
    id: Uuid,
    last_created_at: Option<DateTime<Utc>>,
    kind: Option<String>,
    limit: usize,
}

impl ListQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self {
            limit: 25,
            last_created_at: None,
            id,
            kind: None,
        }
    }

    pub(crate) fn limit(self, limit: usize) -> Self {
        Self { limit, ..self }
    }

    pub(crate) fn kind(self, kind: &str) -> Self {
        Self {
            kind: Some(kind.to_owned()),
            ..self
        }
    }

    pub(crate) fn last_created_at(self, last_created_at: DateTime<Utc>) -> Self {
        Self {
            last_created_at: Some(last_created_at),
            ..self
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Vec<Object>> {
        use quaint::ast::{Comparable, Orderable, ParameterizedValue, Select};
        use quaint::visitor::{Postgres, Visitor};

        let mut q = Select::from_table("change").so_that("edition_id".equals(self.id));

        if let Some(kind) = self.kind {
            q = q.and_where("event_kind".equals(kind));
        }

        if let Some(last_created_at) = self.last_created_at {
            q = q.and_where("created_at".greater_than(last_created_at));
        }

        q = q.order_by("created_at".descend()).limit(self.limit);

        let (sql, bindings) = Postgres::build(q);
        let mut query = sqlx::query_as(&sql);

        for binding in bindings {
            query = match binding {
                ParameterizedValue::Uuid(value) => query.bind(value),
                ParameterizedValue::Integer(value) => query.bind(value),
                ParameterizedValue::Text(value) => query.bind(value.to_string()),
                ParameterizedValue::DateTime(value) => query.bind(value),
                _ => query,
            }
        }

        query.fetch_all(conn).await
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub(crate) struct DeleteQuery {
    id: Uuid,
}

impl DeleteQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self { id }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<()> {
        sqlx::query!("DELETE FROM change WHERE id = $1", self.id)
            .execute(conn)
            .await
            .map(|_| ())
    }
}
