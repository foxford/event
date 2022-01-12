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
                AND   removed = 'f'
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
                    SELECT DISTINCT ON(original_occurred_at, label) *
                    FROM event
                    WHERE deleted_at IS NULL
                    AND   room_id = $1
                    AND   set = $2
                    AND   original_occurred_at < $3
                    AND   occurred_at < COALESCE($4, 9223372036854775807)
                    ORDER BY original_occurred_at DESC, label ASC, occurred_at DESC
                ) AS subq
                WHERE removed = 'f'
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
        if let Some(attribute) = self.attribute {
            sqlx::query!(
                "
                SELECT COUNT(DISTINCT label) as total FROM (
                    SELECT
                        *,
                        bool_or(removed) OVER (
                            PARTITION BY room_id, set, label
                            ORDER BY occurred_at DESC
                        ) AS removed_windowed
                    FROM event
                    WHERE deleted_at IS NULL
                    AND   room_id = $1
                    AND   set = $2
                    AND   original_occurred_at < $3
                    AND   occurred_at < COALESCE($4, 9223372036854775807)
                    ORDER BY original_occurred_at DESC, label ASC, occurred_at DESC
                ) subq
                WHERE removed_windowed = 'f' AND attribute = $5::TEXT
                ",
                self.room_id,
                self.set,
                self.original_occurred_at,
                self.occurred_at,
                attribute,
            )
            .fetch_one(conn)
            .await
            .map(|r| r.total.unwrap_or(0))
        } else {
            sqlx::query!(
                "
                SELECT COUNT(DISTINCT label) as total FROM (
                    SELECT
                        *,
                        bool_or(removed) OVER (
                            PARTITION BY room_id, set, label
                            ORDER BY occurred_at DESC
                        ) AS removed_windowed
                    FROM event
                    WHERE deleted_at IS NULL
                    AND   room_id = $1
                    AND   set = $2
                    AND   original_occurred_at < $3
                    AND   occurred_at < COALESCE($4, 9223372036854775807)
                    ORDER BY original_occurred_at DESC, label ASC, occurred_at DESC
                ) subq
                WHERE removed_windowed = 'f'
                ",
                self.room_id,
                self.set,
                self.original_occurred_at,
                self.occurred_at,
            )
            .fetch_one(conn)
            .await
            .map(|r| r.total.unwrap_or(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use crate::test_helpers::prelude::*;

    use super::*;

    #[tokio::test]
    async fn query_with_removed_events() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let mut conn = db.get_conn().await;
        let room = shared_helpers::insert_room(&mut conn).await;

        let msg1_label = format!("message-{}", Uuid::new_v4());
        let msg2_label = format!("message-{}", Uuid::new_v4());

        message_builder(&room, &msg1_label, 1000, &agent, false)
            .insert(&mut conn)
            .await;

        message_builder(&room, &msg2_label, 5000, &agent, false)
            .insert(&mut conn)
            .await;

        let q = Query::new(room.id(), "messages".into(), 100_000, 100);
        let r = q.clone().execute(&mut conn).await.unwrap();

        // two messages were created
        assert_eq!(r.len(), 2);
        let r = q.total_count(&mut conn).await.unwrap();
        assert_eq!(r, 2);

        message_builder(&room, &msg1_label, 10000, &agent, true)
            .insert(&mut conn)
            .await;

        message_builder(&room, &msg2_label, 12000, &agent, true)
            .insert(&mut conn)
            .await;

        // must be zero since both messages were removed
        let r = q.clone().execute(&mut conn).await.unwrap();
        assert_eq!(r.len(), 0);

        let r = q.total_count(&mut conn).await.unwrap();
        assert_eq!(r, 0);
    }

    #[tokio::test]
    async fn query_with_removed_pins() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let mut conn = db.get_conn().await;
        let room = shared_helpers::insert_room(&mut conn).await;

        let msg1_label = format!("message-{}", Uuid::new_v4());
        let msg2_label = format!("message-{}", Uuid::new_v4());

        message_builder(&room, &msg1_label, 1000, &agent, false)
            .insert(&mut conn)
            .await;

        message_builder(&room, &msg1_label, 3000, &agent, false)
            .attribute("pinned")
            .insert(&mut conn)
            .await;

        message_builder(&room, &msg2_label, 7000, &agent, false)
            .insert(&mut conn)
            .await;

        message_builder(&room, &msg2_label, 8000, &agent, false)
            .attribute("pinned")
            .insert(&mut conn)
            .await;

        let q = Query::new(room.id(), "messages".into(), 100_000, 100).attribute("pinned");

        let r = q.clone().execute(&mut conn).await.unwrap();
        assert_eq!(r.len(), 2);

        let r = q.total_count(&mut conn).await.unwrap();
        assert_eq!(r, 2);

        message_builder(&room, &msg1_label, 12000, &agent, true)
            .insert(&mut conn)
            .await;

        // one pin left since msg1 was removed
        let r = q.clone().execute(&mut conn).await.unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].label(), Some(&msg2_label[..]));

        let r = q.total_count(&mut conn).await.unwrap();
        assert_eq!(r, 1);

        // reinsert msg1 (for instance its deletion was undone)
        message_builder(&room, &msg1_label, 15000, &agent, false)
            .insert(&mut conn)
            .await;

        // but msg1 pin will not reappear
        let r = q.clone().execute(&mut conn).await.unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].label(), Some(&msg2_label[..]));

        let r = q.total_count(&mut conn).await.unwrap();
        assert_eq!(r, 1);
    }

    fn message_builder(
        room: &crate::db::room::Object,
        label: &str,
        occurred_at: i64,
        agent: &TestAgent,
        removed: bool,
    ) -> factory::Event {
        factory::Event::new()
            .room_id(room.id())
            .kind("message")
            .set("messages")
            .label(label)
            .data(&json!({
                "text": "message",
            }))
            .occurred_at(occurred_at)
            .created_by(&agent.agent_id())
            .removed(removed)
    }
}
