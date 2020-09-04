use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use diesel::{pg::PgConnection, result::Error};
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use svc_agent::AgentId;
use uuid06::Uuid;

use super::edition::Object as Edition;
use super::room::Object as Room;
use crate::schema::change;

#[derive(DbEnum, Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[PgType = "change_type"]
#[DieselType = "Change_type"]
pub(crate) enum ChangeType {
    Addition,
    Modification,
    Removal,
}

#[derive(
    Clone, Debug, Serialize, Deserialize, Identifiable, Queryable, QueryableByName, Associations,
)]
#[belongs_to(Edition, foreign_key = "edition_id")]
#[table_name = "change"]
pub(crate) struct Object {
    id: Uuid,
    edition_id: Uuid,
    #[serde(rename = "type")]
    kind: ChangeType,
    event_id: Option<Uuid>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "event_type")]
    event_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_set: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_data: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_occurred_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_created_by: Option<AgentId>,

    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
}

pub(crate) struct FindWithEditionAndRoomQuery {
    id: Uuid,
}

type ChangeWithEditionAndRoom = (Object, (Edition, Room));

impl FindWithEditionAndRoomQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self { id }
    }

    pub(crate) fn execute(
        &self,
        conn: &PgConnection,
    ) -> Result<Option<ChangeWithEditionAndRoom>, Error> {
        use crate::schema::{edition, room};
        use diesel::prelude::*;

        let q = change::table
            .inner_join(edition::table.inner_join(room::table))
            .filter(change::id.eq(self.id));
        q.get_result(conn).optional()
    }
}

#[derive(Debug, Insertable)]
#[table_name = "change"]
pub(crate) struct InsertQuery {
    edition_id: Uuid,
    kind: ChangeType,
    event_id: Option<Uuid>,
    event_kind: Option<String>,
    event_set: Option<String>,
    event_label: Option<String>,
    event_data: Option<JsonValue>,
    event_occurred_at: Option<i64>,
    event_created_by: Option<AgentId>,
}

impl InsertQuery {
    pub(crate) fn new(edition_id: Uuid, kind: ChangeType) -> Self {
        Self {
            event_id: None,
            event_kind: None,
            event_set: None,
            event_label: None,
            event_data: None,
            event_occurred_at: None,
            event_created_by: None,
            edition_id,
            kind,
        }
    }

    pub(crate) fn event_id(self, event_id: Uuid) -> Self {
        Self {
            event_id: Some(event_id),
            ..self
        }
    }

    pub(crate) fn event_kind(self, kind: String) -> Self {
        Self {
            event_kind: Some(kind),
            ..self
        }
    }

    pub(crate) fn event_set(self, set: Option<String>) -> Self {
        Self {
            event_set: set,
            ..self
        }
    }

    pub(crate) fn event_label(self, label: Option<String>) -> Self {
        Self {
            event_label: label,
            ..self
        }
    }

    pub(crate) fn event_data(self, data: JsonValue) -> Self {
        Self {
            event_data: Some(data),
            ..self
        }
    }

    pub(crate) fn event_occurred_at(self, occurred_at: i64) -> Self {
        Self {
            event_occurred_at: Some(occurred_at),
            ..self
        }
    }

    pub(crate) fn event_created_by(self, created_by: AgentId) -> Self {
        Self {
            event_created_by: Some(created_by),
            ..self
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Object, Error> {
        use crate::diesel::RunQueryDsl;
        use crate::schema::change::dsl::change;

        diesel::insert_into(change).values(self).get_result(conn)
    }
}

pub(crate) struct ListQuery {
    id: Uuid,
    last_created_at: Option<DateTime<Utc>>,
    kind: Option<String>,
    limit: i64,
}

impl ListQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self {
            limit: 25,
            last_created_at: None,
            id,
            kind: None,
        }
    }

    pub(crate) fn limit(self, limit: i64) -> Self {
        Self { limit, ..self }
    }

    pub(crate) fn kind(self, kind: &str) -> Self {
        Self {
            kind: Some(kind.to_owned()),
            ..self
        }
    }

    pub(crate) fn last_created_at(self, last_created_at: DateTime<Utc>) -> Self {
        Self {
            last_created_at: Some(last_created_at),
            ..self
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Vec<Object>, Error> {
        use diesel::prelude::*;

        let mut q = change::table
            .filter(change::edition_id.eq(self.id))
            .into_boxed();

        if let Some(ref kind) = self.kind {
            q = q.filter(change::event_kind.eq(kind));
        }

        if let Some(last_created_at) = self.last_created_at {
            q = q.filter(change::created_at.ge(last_created_at));
        }

        q.order_by(change::created_at.desc())
            .limit(self.limit)
            .get_results(conn)
    }
}

pub(crate) struct DeleteQuery {
    id: Uuid,
}

impl DeleteQuery {
    pub(crate) fn new(id: Uuid) -> Self {
        Self { id }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<(), Error> {
        use diesel::prelude::*;

        let q = change::table.filter(change::id.eq(self.id));

        diesel::delete(q).execute(conn)?;
        Ok(())
    }
}

impl Object {
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }

    #[cfg(test)]
    pub(crate) fn edition_id(&self) -> Uuid {
        self.edition_id
    }

    #[cfg(test)]
    pub(crate) fn kind(&self) -> ChangeType {
        self.kind
    }

    #[cfg(test)]
    pub(crate) fn event_id(&self) -> Option<Uuid> {
        self.event_id
    }

    pub(crate) fn event_data(&self) -> &Option<JsonValue> {
        &self.event_data
    }

    pub(crate) fn event_occurred_at(&self) -> Option<i64> {
        self.event_occurred_at
    }
}
