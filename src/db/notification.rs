use serde::ser::{Serialize, SerializeStruct, Serializer};
use serde_json::Value as JsonValue;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::app::topic::Topic;

#[derive(Clone, Debug)]
pub struct Object {
    id: Uuid,
    label: String,
    topic: String,
    #[allow(dead_code)]
    namespace: String,
    payload: JsonValue,
}

impl Object {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    #[cfg(test)]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
}

impl Serialize for Object {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Object", 3)?;
        state.serialize_field("payload", &self.payload)?;
        state.serialize_field("label", &self.label)?;
        state.serialize_field("type", "event")?;
        state.end()
    }
}

pub struct FindWithLockQuery {
    id: Uuid,
}

impl FindWithLockQuery {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }

    pub async fn execute(&self, conn: &mut PgConnection) -> anyhow::Result<Option<Object>> {
        sqlx::query_as!(
            Object,
            r#"
            SELECT
                id,
                label,
                topic,
                namespace,
                payload
            FROM notification
            WHERE id = $1
            FOR UPDATE SKIP LOCKED
            "#,
            self.id
        )
        .fetch_optional(conn)
        .await
        .map_err(|e| anyhow!("Failed to find notification, err = {:?}", e))
    }
}

pub struct DeleteQuery {
    id: Uuid,
}

impl DeleteQuery {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }

    pub async fn execute(&self, conn: &mut PgConnection) -> anyhow::Result<()> {
        sqlx::query_as!(
            Object,
            r#"
            DELETE FROM notification WHERE id = $1
            "#,
            self.id
        )
        .execute(conn)
        .await
        .map(|_| ())
        .map_err(|e| anyhow!("Failed to delete notification, err = {:?}", e))
    }
}

pub struct InsertQuery {
    label: &'static str,
    topic: String,
    namespace: String,
    payload: JsonValue,
}

impl InsertQuery {
    pub fn new(
        label: &'static str,
        topic: Topic,
        namespace: &str,
        payload: impl serde::Serialize,
    ) -> Option<Self> {
        topic.nats_topic().map(|topic| Self {
            label,
            topic,
            namespace: namespace.to_owned(),
            payload: serde_json::to_value(payload).unwrap(),
        })
    }

    pub async fn execute(&self, conn: &mut PgConnection) -> anyhow::Result<Object> {
        sqlx::query_as!(
            Object,
            r#"
            INSERT INTO notification (label, topic, payload, namespace)
            VALUES ($1, $2, $3, $4)
            RETURNING
                id,
                label,
                topic,
                namespace,
                payload
            "#,
            self.label,
            self.topic,
            self.payload,
            self.namespace
        )
        .fetch_one(conn)
        .await
        .map_err(|e| anyhow!("Failed to insert notification, err = {:?}", e))
    }
}

pub struct DelayedListQuery {
    namespace: String,
}

impl DelayedListQuery {
    pub fn new(namespace: &str) -> Self {
        Self {
            namespace: namespace.to_owned(),
        }
    }

    pub async fn execute(&self, conn: &mut PgConnection) -> anyhow::Result<Vec<Object>> {
        sqlx::query_as!(
            Object,
            r#"
            SELECT id, label, topic, namespace, payload
            FROM notification
            WHERE namespace = $1 AND created_at < NOW() - INTERVAL '5 seconds'
            FOR UPDATE SKIP LOCKED
            LIMIT 3
            "#,
            self.namespace
        )
        .fetch_all(conn)
        .await
        .map_err(|e| anyhow!("Failed to find notifications, err = {:?}", e))
    }
}

#[cfg(test)]
pub struct ListQuery {
    topic: Option<String>,
}

#[cfg(test)]
impl ListQuery {
    pub fn new() -> Self {
        Self { topic: None }
    }

    pub fn topic(&mut self, topic: String) -> &mut Self {
        self.topic = Some(topic);
        self
    }

    pub async fn execute(&self, conn: &mut PgConnection) -> sqlx::Result<Vec<Object>> {
        if let Some(t) = &self.topic {
            sqlx::query_as!(
                Object,
                r#"
                SELECT id, label, topic, payload, namespace
                FROM notification
                WHERE topic = $1
                "#,
                t
            )
            .fetch_all(conn)
            .await
        } else {
            sqlx::query_as!(
                Object,
                r#"
                SELECT id, label, topic, namespace, payload
                FROM notification
                "#,
            )
            .fetch_all(conn)
            .await
        }
    }
}
