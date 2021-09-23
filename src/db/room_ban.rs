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
    reason: Option<String>,
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

    #[cfg(test)]
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

#[derive(Debug)]
pub(crate) struct InsertQuery {
    account_id: AccountId,
    room_id: Uuid,
    reason: Option<String>,
}

impl InsertQuery {
    pub(crate) fn new(account_id: AccountId, room_id: Uuid) -> Self {
        Self {
            account_id,
            room_id,
            reason: None,
        }
    }

    pub(crate) fn reason(&mut self, reason: &str) {
        self.reason = Some(reason.to_owned())
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Object> {
        sqlx::query_as!(
            Object,
            r#"
            INSERT INTO room_ban (account_id, room_id, reason)
            VALUES ($1, $2, $3) ON CONFLICT (account_id, room_id) DO UPDATE
            SET created_at=room_ban.created_at
            RETURNING
                id,
                account_id AS "account_id!: AccountId",
                room_id,
                reason,
                created_at
            "#,
            self.account_id as AccountId,
            self.room_id,
            self.reason,
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
                room_id, reason, created_at
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
pub(crate) struct ClassroomFindQuery {
    account_id: AccountId,
    classroom_id: Uuid,
}

impl ClassroomFindQuery {
    pub(crate) fn new(account_id: AccountId, classroom_id: Uuid) -> Self {
        Self {
            account_id,
            classroom_id,
        }
    }

    pub(crate) async fn execute(self, conn: &mut PgConnection) -> sqlx::Result<Option<Object>> {
        sqlx::query_as!(
            Object,
            r#"
            SELECT
                id, account_id AS "account_id!: AccountId",
                room_id, reason, created_at
            FROM room_ban
            WHERE account_id = $1 AND room_id = (
                SELECT id FROM room
                WHERE classroom_id = $2 AND UPPER(time) IS NULL
                ORDER BY created_at DESC LIMIT 1
            )
            "#,
            self.account_id as AccountId,
            self.classroom_id,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::prelude::*;
    use std::ops::Bound;

    #[async_std::test]
    async fn find_ban_multirooms() {
        let db = TestDb::new().await;
        let mut conn = db.get_conn().await;

        let banned_agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let classroom_id = Uuid::new_v4();

        let room = {
            factory::Room::new()
                .audience(USR_AUDIENCE)
                .classroom_id(classroom_id)
                .time((Bound::Included(Utc::now()), Bound::Unbounded))
                .insert(&mut conn)
                .await;
            let room = factory::Room::new()
                .audience(USR_AUDIENCE)
                .time((Bound::Included(Utc::now()), Bound::Unbounded))
                .classroom_id(classroom_id)
                .insert(&mut conn)
                .await;
            factory::RoomBan::new(banned_agent.account_id(), room.id())
                .insert(&mut conn)
                .await;
            room
        };

        let ban = ClassroomFindQuery::new(banned_agent.account_id().to_owned(), classroom_id)
            .execute(&mut conn)
            .await
            .expect("Ban query failed");
        assert!(ban.is_some());
    }
}
