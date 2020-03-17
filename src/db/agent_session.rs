use std::ops::Bound;

use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use diesel::{pg::PgConnection, result::Error};
use serde_derive::{Deserialize, Serialize};
use svc_agent::AgentId;
use uuid::Uuid;

use super::room::Object as Room;
use crate::schema::agent_session;

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug, DbEnum, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[PgType = "agent_session_status"]
#[DieselType = "Agent_session_status"]
pub(crate) enum Status {
    Claimed,
    Started,
    Finished,
}

pub(crate) type Time = (Bound<DateTime<Utc>>, Bound<DateTime<Utc>>);

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, QueryableByName, Associations)]
#[belongs_to(Room, foreign_key = "room_id")]
#[table_name = "agent_session"]
pub(crate) struct Object {
    #[serde(skip_serializing)]
    id: Uuid,
    agent_id: AgentId,
    room_id: Uuid,
    #[serde(skip_serializing)]
    status: Status,
    #[serde(skip)]
    time: Option<Time>,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
}

impl Object {
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }

    #[cfg(test)]
    pub(crate) fn status(&self) -> Status {
        self.status
    }

    pub(crate) fn time(&self) -> Option<&Time> {
        self.time.as_ref()
    }
}

////////////////////////////////////////////////////////////////////////////////

pub(crate) struct ListQuery<'a> {
    agent_id: Option<&'a AgentId>,
    room_id: Option<Uuid>,
    status: Option<Status>,
    offset: Option<i64>,
    limit: Option<i64>,
}

impl<'a> ListQuery<'a> {
    pub(crate) fn new() -> Self {
        Self {
            agent_id: None,
            room_id: None,
            status: None,
            offset: None,
            limit: None,
        }
    }

    pub(crate) fn agent_id(self, agent_id: &'a AgentId) -> Self {
        Self {
            agent_id: Some(agent_id),
            ..self
        }
    }

    pub(crate) fn room_id(self, room_id: Uuid) -> Self {
        Self {
            room_id: Some(room_id),
            ..self
        }
    }

    pub(crate) fn status(self, status: Status) -> Self {
        Self {
            status: Some(status),
            ..self
        }
    }

    pub(crate) fn offset(self, offset: i64) -> Self {
        Self {
            offset: Some(offset),
            ..self
        }
    }

    pub(crate) fn limit(self, limit: i64) -> Self {
        Self {
            limit: Some(limit),
            ..self
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Vec<Object>, Error> {
        use diesel::prelude::*;

        let mut q = agent_session::table.into_boxed();

        if let Some(agent_id) = self.agent_id {
            q = q.filter(agent_session::agent_id.eq(agent_id));
        }

        if let Some(room_id) = self.room_id {
            q = q.filter(agent_session::room_id.eq(room_id));
        }

        if let Some(status) = self.status {
            q = q.filter(agent_session::status.eq(status));
        }

        if let Some(offset) = self.offset {
            q = q.offset(offset);
        }

        if let Some(limit) = self.limit {
            q = q.limit(limit);
        }

        q.order_by(agent_session::created_at.desc())
            .get_results(conn)
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) struct FindQuery<'a> {
    room_id: Uuid,
    agent_id: &'a AgentId,
}

impl<'a> FindQuery<'a> {
    pub(crate) fn new(room_id: Uuid, agent_id: &'a AgentId) -> Self {
        Self { agent_id, room_id }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Option<Object>, Error> {
        use diesel::prelude::*;

        agent_session::table
            .filter(agent_session::status.eq(Status::Started))
            .filter(agent_session::agent_id.eq(self.agent_id))
            .filter(agent_session::room_id.eq(self.room_id))
            .get_result(conn)
            .optional()
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Insertable)]
#[table_name = "agent_session"]
pub(crate) struct InsertQuery<'a> {
    id: Option<Uuid>,
    agent_id: &'a AgentId,
    room_id: Uuid,
    status: Status,
    time: Option<Time>,
}

impl<'a> InsertQuery<'a> {
    pub(crate) fn new(agent_id: &'a AgentId, room_id: Uuid) -> Self {
        Self {
            id: None,
            agent_id,
            room_id,
            status: Status::Claimed,
            time: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn status(self, status: Status) -> Self {
        Self { status, ..self }
    }

    #[cfg(test)]
    pub(crate) fn time(self, time: Time) -> Self {
        Self {
            time: Some(time),
            ..self
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Object, Error> {
        use crate::schema::agent_session::dsl::*;
        use diesel::{ExpressionMethods, RunQueryDsl};

        diesel::insert_into(agent_session)
            .values(self)
            .on_conflict((agent_id, room_id))
            .do_update()
            .set(status.eq(self.status))
            .get_result(conn)
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, AsChangeset)]
#[table_name = "agent_session"]
pub(crate) struct UpdateQuery<'a> {
    agent_id: &'a AgentId,
    room_id: Uuid,
    status: Option<Status>,
    time: Option<Time>,
}

impl<'a> UpdateQuery<'a> {
    pub(crate) fn new(agent_id: &'a AgentId, room_id: Uuid) -> Self {
        Self {
            agent_id,
            room_id,
            status: None,
            time: None,
        }
    }

    pub(crate) fn status(self, status: Status) -> Self {
        Self {
            status: Some(status),
            ..self
        }
    }

    pub(crate) fn time(self, time: Time) -> Self {
        Self {
            time: Some(time),
            ..self
        }
    }

    pub(crate) fn execute(&self, conn: &PgConnection) -> Result<Option<Object>, Error> {
        use diesel::prelude::*;

        let query = agent_session::table
            .filter(agent_session::status.ne(Status::Finished))
            .filter(agent_session::agent_id.eq(self.agent_id))
            .filter(agent_session::room_id.eq(self.room_id));

        diesel::update(query).set(self).get_result(conn).optional()
    }
}
