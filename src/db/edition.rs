use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use diesel::{pg::PgConnection, result::Error};

use serde_derive::{Deserialize, Serialize};
use svc_agent::AgentId;
use uuid::Uuid;

use super::room::Object as Room;
use crate::schema::edition;

#[derive(
    Clone, Debug, Serialize, Deserialize, Identifiable, Queryable, QueryableByName, Associations,
)]
#[belongs_to(Room, foreign_key = "source_room_id")]
#[table_name = "edition"]
pub(crate) struct Object {
    id: Uuid,
    source_room_id: Uuid,
    created_by: AgentId,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[table_name = "edition"]
pub(crate) struct InsertQuery<'a> {
    source_room_id: &'a Uuid,
    created_by: &'a AgentId,
}

impl<'a> InsertQuery<'a> {
    pub(crate) fn new(source_room_id: &'a Uuid, created_by: &'a AgentId) -> Self {
        Self {
            source_room_id,
            created_by,
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Object, Error> {
        use crate::diesel::RunQueryDsl;
        use crate::schema::edition::dsl::edition;

        diesel::insert_into(edition).values(self).get_result(conn)
    }
}

pub(crate) struct ListQuery<'a> {
    source_room_id: &'a Uuid,
    last_created_at: Option<&'a DateTime<Utc>>,
    limit: i64,
}

impl<'a> ListQuery<'a> {
    pub(crate) fn new(source_room_id: &'a Uuid) -> Self {
        Self {
            source_room_id: source_room_id,
            limit: 25,
            last_created_at: None,
        }
    }

    pub(crate) fn limit(self, limit: i64) -> Self {
        Self {
            limit: limit,
            ..self
        }
    }

    pub(crate) fn last_created_at(self, last_created_at: &'a DateTime<Utc>) -> Self {
        Self {
            last_created_at: Some(last_created_at),
            ..self
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Vec<Object>, Error> {
        use diesel::prelude::*;

        let mut q = edition::table
            .filter(edition::source_room_id.eq(self.source_room_id))
            .into_boxed();

        if let Some(last_created_at) = self.last_created_at {
            q = q.filter(edition::created_at.ge(last_created_at));
        }

        q = q.limit(self.limit);
        q.get_results(conn)
    }
}

impl Object {
    #[cfg(test)]
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }

    #[cfg(test)]
    pub(crate) fn source_room_id(&self) -> Uuid {
        self.source_room_id
    }
}
