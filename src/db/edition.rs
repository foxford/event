use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};
use svc_agent::AgentId;
use uuid::Uuid;

use super::room::Object as Room;
use crate::schema::edition;

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, QueryableByName, Associations)]
#[belongs_to(Room, foreign_key = "source_room_id")]
#[table_name = "edition"]
pub(crate) struct Object {
    id: Uuid,
    source_room_id: Uuid,
    created_by: AgentId,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
}
