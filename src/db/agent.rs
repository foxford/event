use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use sqlx::postgres::PgConnection;
use svc_agent::AgentId;
use uuid::Uuid;

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "agent_status")]
pub enum Status {
    #[sqlx(rename = "in_progress")]
    InProgress,
    #[sqlx(rename = "ready")]
    Ready,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Object {
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    id: Uuid,
    agent_id: AgentId,
    room_id: Uuid,
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    status: Status,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentWithBan {
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    id: Uuid,
    agent_id: AgentId,
    room_id: Uuid,
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    status: Status,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    banned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

impl AgentWithBan {
    pub fn banned(&self) -> Option<bool> {
        self.banned
    }
}

////////////////////////////////////////////////////////////////////////////////

// это не мертвый код, он используется в методе ListQuery::execute
// но по какой то причине определяется анализатором как мертвый
#[allow(dead_code)]
const DEFAULT_LIST_LIMIT: usize = 1000;

#[cfg(test)]
#[derive(Debug)]
pub struct ListQuery {
    agent_id: Option<AgentId>,
    room_id: Option<Uuid>,
    status: Option<Status>,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[cfg(test)]
impl ListQuery {
    pub fn new() -> Self {
        Self {
            agent_id: None,
            room_id: None,
            status: None,
            offset: None,
            limit: None,
        }
    }

    pub fn agent_id(self, agent_id: AgentId) -> Self {
        Self {
            agent_id: Some(agent_id),
            ..self
        }
    }

    pub fn room_id(self, room_id: Uuid) -> Self {
        Self {
            room_id: Some(room_id),
            ..self
        }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Vec<Object>> {
        let limit = self.limit.unwrap_or(DEFAULT_LIST_LIMIT);
        let offset = self.offset.unwrap_or(0);
        sqlx::query_as!(
            Object,
            r#"
            SELECT
                id,
                agent_id            AS "agent_id!: AgentId",
                room_id,
                status              AS "status!: Status",
                created_at
            FROM agent
            WHERE ($1::agent_id IS NULL OR agent_id = $1)
                AND ($2::uuid IS NULL OR room_id = $2)
                AND ($3::agent_status IS NULL OR status > $3)
            ORDER BY created_at DESC LIMIT $4 OFFSET $5
            "#,
            self.agent_id as Option<AgentId>,
            self.room_id,
            self.status as Option<Status>,
            limit as i64,
            offset as i64,
        )
        .fetch_all(conn)
        .await
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct ListWithBansQuery {
    room_id: Uuid,
    status: Status,
    offset: usize,
    limit: usize,
}

impl ListWithBansQuery {
    pub fn new(room_id: Uuid, status: Status, offset: usize, limit: usize) -> Self {
        Self {
            room_id,
            status,
            offset,
            limit,
        }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Vec<AgentWithBan>> {
        sqlx::query_as!(
            AgentWithBan,
            r#"
            SELECT
                agent.id,
                agent_id AS "agent_id!: AgentId",
                agent.room_id,
                status AS "status!: Status",
                agent.created_at,
                (rban.created_at IS NOT NULL)::boolean AS banned,
                rban.reason
            FROM agent
            LEFT OUTER JOIN room_ban rban
            ON rban.room_id = agent.room_id AND rban.account_id = (agent.agent_id).account_id
            WHERE agent.room_id = $1 AND agent.status = $2
            ORDER BY created_at DESC
            LIMIT $3
            OFFSET $4
            "#,
            self.room_id,
            self.status as Status,
            self.limit as i64,
            self.offset as i64
        )
        .fetch_all(conn)
        .await
    }
}

#[derive(Debug)]
pub struct FindWithBanQuery {
    agent_id: AgentId,
    room_id: Uuid,
}

impl FindWithBanQuery {
    pub fn new(agent_id: AgentId, room_id: Uuid) -> Self {
        Self { agent_id, room_id }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Option<AgentWithBan>> {
        sqlx::query_as!(
            AgentWithBan,
            r#"
            SELECT
                agent.id,
                agent_id AS "agent_id!: AgentId",
                agent.room_id,
                status AS "status!: Status",
                agent.created_at,
                (rban.created_at IS NOT NULL)::boolean AS banned,
                rban.reason
            FROM agent
            LEFT OUTER JOIN room_ban rban
            ON rban.room_id = agent.room_id AND rban.account_id = (agent.agent_id).account_id
            WHERE agent_id = $1 AND agent.room_id = $2
            LIMIT 1
            "#,
            self.agent_id as AgentId,
            self.room_id
        )
        .fetch_optional(conn)
        .await
    }
}

#[derive(Debug)]
pub struct InsertQuery {
    agent_id: AgentId,
    room_id: Uuid,
    status: Status,
}

impl InsertQuery {
    pub fn new(agent_id: AgentId, room_id: Uuid) -> Self {
        Self {
            agent_id,
            room_id,
            status: Status::InProgress,
        }
    }

    #[cfg(test)]
    pub fn status(self, status: Status) -> Self {
        Self { status, ..self }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        sqlx::query_as!(
            Object,
            r#"
            INSERT INTO agent (agent_id, room_id, status)
            VALUES ($1, $2, $3)
            ON CONFLICT (agent_id, room_id) DO UPDATE SET status = $3
            RETURNING
                id,
                agent_id AS "agent_id!: AgentId",
                room_id,
                status AS "status!: Status",
                created_at
            "#,
            self.agent_id as AgentId,
            self.room_id,
            self.status as Status,
        )
        .fetch_one(conn)
        .await
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct UpdateQuery {
    agent_id: AgentId,
    room_id: Uuid,
    status: Option<Status>,
}

impl UpdateQuery {
    pub fn new(agent_id: AgentId, room_id: Uuid) -> Self {
        Self {
            agent_id,
            room_id,
            status: None,
        }
    }

    pub fn status(self, status: Status) -> Self {
        Self {
            status: Some(status),
            ..self
        }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Option<Object>> {
        sqlx::query_as!(
            Object,
            r#"
            UPDATE agent
            SET status = $3
            WHERE agent_id = $1
            AND   room_id = $2
            RETURNING
                id,
                agent_id AS "agent_id!: AgentId",
                room_id,
                status AS "status!: Status",
                created_at
            "#,
            self.agent_id.to_owned() as AgentId,
            self.room_id,
            self.status as Option<Status>,
        )
        .fetch_optional(conn)
        .await
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct DeleteQuery {
    agent_id: AgentId,
    room_id: Uuid,
}

impl DeleteQuery {
    pub fn new(agent_id: AgentId, room_id: Uuid) -> Self {
        Self { agent_id, room_id }
    }

    pub async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<usize> {
        sqlx::query_as!(
            Object,
            r#"
            DELETE FROM agent
            WHERE agent_id = $1
            AND   room_id  = $2
            "#,
            self.agent_id.to_owned() as AgentId,
            self.room_id,
        )
        .execute(conn)
        .await
        .map(|r| r.rows_affected() as usize)
    }
}
