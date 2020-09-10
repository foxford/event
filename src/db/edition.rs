use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use diesel::{pg::PgConnection, result::Error};

use serde_derive::{Deserialize, Serialize};
use svc_agent::AgentId;
use uuid06::Uuid;

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

impl Object {
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }

    pub(crate) fn source_room_id(&self) -> Uuid {
        self.source_room_id
    }
}

pub(crate) struct FindQuery {
    id: Uuid,
}

impl FindQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self { id }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Option<Object>, Error> {
        use diesel::prelude::*;

        edition::table
            .filter(edition::id.eq(self.id))
            .get_result(conn)
            .optional()
    }
}

#[derive(Debug, Insertable)]
#[table_name = "edition"]
pub(crate) struct InsertQuery {
    source_room_id: Uuid,
    created_by: AgentId,
}

impl InsertQuery {
    pub(crate) fn new(source_room_id: Uuid, created_by: AgentId) -> Self {
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

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Vec<Object>, Error> {
        use diesel::prelude::*;

        let mut q = edition::table
            .filter(edition::source_room_id.eq(self.source_room_id))
            .into_boxed();

        if let Some(last_created_at) = self.last_created_at {
            q = q.filter(edition::created_at.ge(last_created_at));
        }

        q = q.order_by(edition::created_at.desc()).limit(self.limit);
        q.get_results(conn)
    }
}

pub(crate) struct DeleteQuery {
    id: Uuid,
}

impl DeleteQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self { id }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<usize, Error> {
        use diesel::prelude::*;

        let q = edition::table.filter(edition::id.eq(self.id));
        diesel::delete(q).execute(conn)
    }
}
