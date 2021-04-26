use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use sqlx::{postgres::PgConnection, Done};
use svc_agent::AgentId;
use uuid::Uuid;

use crate::db::room::{Builder as RoomBuilder, Object as Room, Time as RoomTime};

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Object {
    id: Uuid,
    source_room_id: Uuid,
    created_by: AgentId,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
}

impl Object {
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }

    pub(crate) fn source_room_id(&self) -> Uuid {
        self.source_room_id
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
                e.id               AS edition_id,
                e.source_room_id   AS edition_source_room_id,
                e.created_by       AS "edition_created_by!: AgentId",
                e.created_at       AS edition_created_at,
                r.id               AS room_id,
                r.audience         AS room_audience,
                r.source_room_id   AS room_source_room_id,
                r.time             AS "room_time!: RoomTime",
                r.tags             AS room_tags,
                r.created_at       AS room_created_at,
                r.preserve_history AS room_preserve_history,
                r.classroom_id     AS room_classroom_id
            FROM edition AS e
            INNER JOIN room AS r
            ON r.id = e.source_room_id
            WHERE e.id = $1
            "#,
            self.id,
        )
        .fetch_optional(conn)
        .await?;

        match maybe_row {
            None => Ok(None),
            Some(row) => {
                let edition = Object {
                    id: row.edition_id,
                    source_room_id: row.edition_source_room_id,
                    created_by: row.edition_created_by,
                    created_at: row.edition_created_at,
                };

                let room = RoomBuilder::new()
                    .id(row.room_id)
                    .audience(row.room_audience)
                    .source_room_id(row.room_source_room_id)
                    .time(row.room_time)
                    .tags(row.room_tags)
                    .created_at(row.room_created_at)
                    .preserve_history(row.room_preserve_history)
                    .classroom_id(row.room_classroom_id)
                    .build()
                    .map_err(|err| sqlx::Error::Decode(err.into()))?;

                Ok(Some((edition, room)))
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub(crate) struct InsertQuery<'a> {
    source_room_id: Uuid,
    created_by: &'a AgentId,
}

impl<'a> InsertQuery<'a> {
    pub(crate) fn new(source_room_id: Uuid, created_by: &'a AgentId) -> Self {
        Self {
            source_room_id,
            created_by,
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        sqlx::query_as!(
            Object,
            r#"
            INSERT INTO edition (source_room_id, created_by)
            VALUES ($1, $2)
            RETURNING id, source_room_id, created_by AS "created_by!: AgentId", created_at
            "#,
            self.source_room_id,
            self.created_by.to_owned() as AgentId,
        )
        .fetch_one(conn)
        .await
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub(crate) struct ListQuery {
    source_room_id: Uuid,
    last_created_at: Option<DateTime<Utc>>,
    limit: i64,
}

impl ListQuery {
    pub(crate) fn new(source_room_id: Uuid) -> Self {
        Self {
            limit: 25,
            last_created_at: None,
            source_room_id,
        }
    }

    pub(crate) fn limit(self, limit: i64) -> Self {
        Self { limit, ..self }
    }

    pub(crate) fn last_created_at(self, last_created_at: DateTime<Utc>) -> Self {
        Self {
            last_created_at: Some(last_created_at),
            ..self
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Vec<Object>> {
        sqlx::query_as!(
            Object,
            r#"
            SELECT id, source_room_id, created_by AS "created_by!: AgentId", created_at
            FROM edition
            WHERE source_room_id = $1
            AND   created_at > COALESCE($2, TO_TIMESTAMP(0))
            ORDER BY created_at DESC
            LIMIT $3
            "#,
            self.source_room_id,
            self.last_created_at,
            self.limit,
        )
        .fetch_all(conn)
        .await
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

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<usize> {
        sqlx::query!("DELETE FROM edition WHERE id = $1", self.id)
            .execute(conn)
            .await
            .map(|r| r.rows_affected() as usize)
    }
}
