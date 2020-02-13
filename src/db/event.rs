use chrono::serde::{ts_seconds, ts_seconds_option};
use chrono::{DateTime, Utc};
use diesel::{pg::PgConnection, result::Error};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use svc_agent::AgentId;
use uuid::Uuid;

use super::room::Object as Room;
use crate::schema::event;

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[belongs_to(Room, foreign_key = "room_id")]
#[table_name = "event"]
pub(crate) struct Object {
    id: Uuid,
    room_id: Uuid,
    type_: String,
    data: JsonValue,
    offset: i64,
    created_by: AgentId,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
    #[serde(with = "ts_seconds_option")]
    deleted_at: Option<DateTime<Utc>>,
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Insertable)]
#[table_name = "event"]
pub(crate) struct InsertQuery<'a> {
    id: Option<Uuid>,
    room_id: Uuid,
    type_: String,
    data: JsonValue,
    created_by: &'a AgentId,
}

impl<'a> InsertQuery<'a> {
    pub(crate) fn new(
        room_id: Uuid,
        type_: &str,
        data: JsonValue,
        created_by: &'a AgentId,
    ) -> Self {
        Self {
            id: None,
            room_id,
            type_: type_.to_owned(),
            data,
            created_by,
        }
    }

    pub(crate) fn id(self, id: Uuid) -> Self {
        Self {
            id: Some(id),
            ..self
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Object, Error> {
        use crate::diesel::RunQueryDsl;
        use crate::schema::event::dsl::event;

        diesel::insert_into(event).values(self).get_result(conn)
    }
}
