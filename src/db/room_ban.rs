use chrono::{DateTime, Utc};
use sqlx::{postgres::PgConnection, Done};
use svc_agent::AccountId;
use uuid::Uuid;

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct Object {
    id: Uuid,
    account_id: AccountId,
    room_id: Uuid,
    created_at: DateTime<Utc>,
}

impl Object {
    #[cfg(test)]
    pub fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    #[cfg(test)]
    pub fn room_id(&self) -> &Uuid {
        &self.room_id
    }
}

#[derive(Debug)]
pub(crate) struct InsertQuery {
    account_id: AccountId,
    room_id: Uuid,
}

impl InsertQuery {
    pub(crate) fn new(account_id: AccountId, room_id: Uuid) -> Self {
        Self {
            account_id,
            room_id,
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        sqlx::query_as!(
            Object,
            r#"
            INSERT INTO room_ban (account_id, room_id)
            VALUES ($1, $2) ON CONFLICT (account_id, room_id) DO UPDATE
            SET created_at=room_ban.created_at
            RETURNING
                id,
                account_id AS "account_id!: AccountId",
                room_id,
                created_at
            "#,
            self.account_id as AccountId,
            self.room_id,
        )
        .fetch_one(conn)
        .await
    }
}

#[derive(Debug)]
pub(crate) struct FindQuery {
    account_id: AccountId,
    room_id: Uuid,
}

impl FindQuery {
    pub(crate) fn new(account_id: AccountId, room_id: Uuid) -> Self {
        Self {
            account_id,
            room_id,
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Option<Object>> {
        sqlx::query_as!(
            Object,
            r#"
            SELECT
                id, account_id AS "account_id!: AccountId",
                room_id, created_at
            FROM room_ban
            WHERE account_id = $1 AND room_id = $2
            "#,
            self.account_id as AccountId,
            self.room_id,
        )
        .fetch_optional(conn)
        .await
    }
}

#[derive(Debug)]
pub(crate) struct DeleteQuery {
    account_id: AccountId,
    room_id: Uuid,
}

impl DeleteQuery {
    pub(crate) fn new(account_id: AccountId, room_id: Uuid) -> Self {
        Self {
            account_id,
            room_id,
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<usize> {
        sqlx::query_as!(
            Object,
            r#"
            DELETE FROM room_ban
            WHERE account_id = $1
            AND   room_id  = $2
            "#,
            self.account_id as AccountId,
            self.room_id,
        )
        .execute(conn)
        .await
        .map(|r| r.rows_affected() as usize)
    }
}
