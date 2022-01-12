use sqlx::postgres::PgConnection;
use svc_agent::AgentId;
use uuid::Uuid;

use super::Object;

#[derive(Clone)]
pub struct Query<'a> {
    room_id: Uuid,
    set: String,
    attribute: Option<&'a str>,
    occurred_at: Option<i64>,
    original_occurred_at: i64,
    limit: i64,
}

impl<'a> Query<'a> {
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
                    original_occurred_at,
                    original_created_by as "original_created_by: AgentId"
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
                    original_occurred_at,
                    original_created_by as "original_created_by: AgentId"
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
