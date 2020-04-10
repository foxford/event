use std::ops::Bound;

use chrono::Utc;
use diesel::connection::Connection;
use diesel::pg::PgConnection;
use failure::{bail, format_err, Error};
use log::info;
use svc_agent::AgentId;

use crate::app::operations::adjust_room::{
    cut_events_to_gaps, invert_segments, NANOSECONDS_IN_MILLISECOND,
};
use crate::db::adjustment::Segment;
use crate::db::edition::Object as Edition;
use crate::db::event::{DeleteQuery as EventDeleteQuery, ListQuery as EventListQuery};
use crate::db::room::{InsertQuery as RoomInsertQuery, Object as Room};
use crate::db::ConnectionPool as Db;

pub(crate) fn call(
    db: &Db,
    edition: &Edition,
    source: &Room,
    agent_id: &AgentId,
) -> Result<(Room, Vec<Segment>), Error> {
    info!(
        "Edition commit task started for edition_id = '{}', source room id = {}",
        edition.id(),
        source.id()
    );
    let start_timestamp = Utc::now();

    let conn = db.get()?;

    let result = conn.transaction::<(Room, Vec<Segment>), Error, _>(|| {
        let room_duration = match source.time() {
            (Bound::Included(start), Bound::Excluded(stop)) if stop > start => {
                stop.signed_duration_since(*start)
            }
            _ => bail!("invalid duration for room = '{}'", source.id()),
        };

        let cut_events = EventListQuery::new()
            .room_id(source.id())
            .kind("stream")
            .execute(&conn)
            .map_err(|err| {
                format_err!(
                    "failed to fetch cut events for room_id = '{}': {}",
                    source.id(),
                    err
                )
            })?;

        let cut_gaps = cut_events_to_gaps(&cut_events)?;

        let destination = clone_room(&conn, &source)?;
        clone_events(&conn, &source, &destination, &edition, agent_id, &cut_gaps)?;

        EventDeleteQuery::new(destination.id(), "stream")
            .execute(&conn)
            .map_err(|err| {
                format_err!(
                    "failed to delete cut events for room_id = '{}': {}",
                    destination.id(),
                    err
                )
            })?;

        let bounded_cut_gaps = cut_gaps
            .into_iter()
            .map(|(start, stop)| (Bound::Included(start), Bound::Excluded(stop)))
            .collect::<Vec<Segment>>();

        let modified_segments = invert_segments(&bounded_cut_gaps, room_duration)?
            .into_iter()
            .map(|(start, stop)| {
                (
                    Bound::Included(start / NANOSECONDS_IN_MILLISECOND),
                    Bound::Excluded(stop / NANOSECONDS_IN_MILLISECOND),
                )
            })
            .collect::<Vec<Segment>>();

        Ok((destination, modified_segments))
    })?;

    info!(
        "Edition commit successfully finished for edition_id = '{}', duration = {} ms",
        edition.id(),
        (Utc::now() - start_timestamp).num_milliseconds()
    );

    Ok(result)
}

fn clone_room(conn: &PgConnection, source: &Room) -> Result<Room, Error> {
    let mut query = RoomInsertQuery::new(&source.audience(), source.time().to_owned());

    query = query.source_room_id(source.id());

    if let Some(tags) = source.tags() {
        query = query.tags(tags.to_owned());
    }

    query
        .execute(conn)
        .map_err(|err| format_err!("Failed to insert room: {}", err))
}

const CLONE_EVENTS_SQL: &str = r#"
WITH
    gap_starts AS (
        SELECT start, ROW_NUMBER() OVER () AS row_number
        FROM UNNEST($5::BIGINT[]) AS start
    ),
    gap_stops AS (
        SELECT stop, ROW_NUMBER() OVER () AS row_number
        FROM UNNEST($6::BIGINT[]) AS stop
    ),
    gaps AS (
        SELECT start, stop
        FROM gap_starts, gap_stops
        WHERE gap_stops.row_number = gap_starts.row_number
    )
INSERT INTO event (id, room_id, kind, set, label, data, occurred_at, created_by, created_at)
SELECT
    id,
    room_id,
    kind,
    set,
    label,
    data,
    occurred_at + ROW_NUMBER() OVER (partition by occurred_at order by created_at) - 1,
    created_by,
    created_at
FROM (
    SELECT
        gen_random_uuid() AS id,
        $2 AS room_id,
        (CASE change.kind
                WHEN 'addition' THEN change.event_kind
                WHEN 'modification' THEN COALESCE(change.event_kind, event.kind)
                ELSE event.kind
            END
        ) AS kind,
        (CASE change.kind
            WHEN 'addition' THEN COALESCE(change.event_set, change.event_kind)
            WHEN 'modification' THEN COALESCE(change.event_set, event.set, change.event_kind, event.kind)
            ELSE event.set
            END
        ) AS set,
        (CASE change.kind
            WHEN 'addition' THEN change.event_label
            WHEN 'modification' THEN COALESCE(change.event_label, event.label)
            ELSE event.label
            END
        ) AS label,
        (CASE change.kind
            WHEN 'addition' THEN change.event_data
            WHEN 'modification' THEN COALESCE(change.event_data, event.data)
            ELSE event.data
            END
        ) AS data,
        (
            (CASE change.kind
                WHEN 'addition' THEN change.event_occurred_at
                WHEN 'modification' THEN COALESCE(change.event_occurred_at, event.occurred_at)
                ELSE event.occurred_at
                END
            ) - (
                SELECT COALESCE(SUM(LEAST(stop, occurred_at) - start), 0)
                FROM gaps
                WHERE start < occurred_at
            )
        ) AS occurred_at,
        $3 AS created_by,
        COALESCE(event.created_at, NOW()) as created_at
    FROM event
        FULL OUTER JOIN change ON change.event_id = event.id
    WHERE
        ((event.room_id = $1 AND deleted_at IS NULL) OR event.id IS NULL)
        AND
        ((change.edition_id = $4 AND change.kind <> 'removal') OR change.id IS NULL)
) AS subquery;
"#;

fn clone_events(
    conn: &PgConnection,
    source: &Room,
    destination: &Room,
    edition: &Edition,
    agent_id: &AgentId,
    gaps: &[(i64, i64)],
) -> Result<(), Error> {
    use diesel::prelude::*;
    use diesel::sql_types::{Array, Int8, Uuid};
    use svc_agent::sql::Agent_id;

    let mut starts = Vec::with_capacity(gaps.len());
    let mut stops = Vec::with_capacity(gaps.len());

    for (start, stop) in gaps {
        starts.push(start);
        stops.push(stop);
    }

    diesel::sql_query(CLONE_EVENTS_SQL)
        .bind::<Uuid, _>(source.id())
        .bind::<Uuid, _>(destination.id())
        .bind::<Agent_id, _>(agent_id)
        .bind::<Uuid, _>(edition.id())
        .bind::<Array<Int8>, _>(&starts)
        .bind::<Array<Int8>, _>(&stops)
        .execute(conn)
        .map(|_| ())
        .map_err(|err| {
            format_err!(
                "Failed cloning events from room = '{}' to room = {}, reason: {}",
                source.id(),
                destination.id(),
                err
            )
        })
}

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use chrono::Duration;
    use diesel::pg::PgConnection;
    use serde_json::{json, Value as JsonValue};
    use svc_agent::{AccountId, AgentId};

    use crate::db::event::{
        InsertQuery as EventInsertQuery, ListQuery as EventListQuery, Object as Event,
    };

    use crate::db::change::ChangeType;
    use crate::db::room::Object as Room;
    use crate::test_helpers::db::TestDb;
    use crate::test_helpers::prelude::*;

    const AUDIENCE: &str = "dev.svc.example.org";

    #[test]
    fn commit_edition() {
        futures::executor::block_on(async {
            let db = TestDb::new();
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

            let conn = db
                .connection_pool()
                .get()
                .expect("Failed to get db connection");

            let room = shared_helpers::insert_room(&conn);

            // Seed events.
            let e1 = create_event(
                &conn,
                &room,
                1_000_000_000,
                "message",
                json!({"message": "m1"}),
            );

            let e2 = create_event(
                &conn,
                &room,
                2_000_000_000,
                "message",
                json!({"message": "m2"}),
            );

            create_event(
                &conn,
                &room,
                2_500_000_000,
                "message",
                json!({"message": "passthrough"}),
            );

            create_event(
                &conn,
                &room,
                3_000_000_000,
                "stream",
                json!({"cut": "start"}),
            );

            create_event(
                &conn,
                &room,
                4_000_000_000,
                "message",
                json!({"message": "m4"}),
            );

            create_event(
                &conn,
                &room,
                5_000_000_000,
                "stream",
                json!({"cut": "stop"}),
            );

            create_event(
                &conn,
                &room,
                6_000_000_000,
                "message",
                json!({"message": "m5"}),
            );

            let edition = factory::Edition::new(room.id(), agent.agent_id()).insert(&conn);

            factory::Change::new(edition.id(), ChangeType::Addition)
                .event_data(json!({"message": "newmessage"}))
                .event_kind("something")
                .event_set("type")
                .event_label("mylabel")
                .event_occurred_at(3_000_000_000)
                .event_created_by(agent.agent_id())
                .insert(&conn);

            factory::Change::new(edition.id(), ChangeType::Modification)
                .event_data(json![{"key": "value"}])
                .event_label("randomlabel")
                .event_id(e1.id())
                .insert(&conn);

            factory::Change::new(edition.id(), ChangeType::Removal)
                .event_id(e2.id())
                .insert(&conn);

            drop(conn);
            let (destination, segments) =
                super::call(db.connection_pool(), &edition, &room, agent.agent_id())
                    .expect("edition commit failed");

            // Assert original room.
            assert_eq!(destination.source_room_id().unwrap(), room.id());
            assert_eq!(room.audience(), destination.audience());
            assert_eq!(room.tags(), destination.tags());
            assert_eq!(segments.len(), 2);

            let conn = db
                .connection_pool()
                .get()
                .expect("Failed to get db connection");

            let events = EventListQuery::new()
                .room_id(destination.id())
                .execute(&conn)
                .expect("Failed to fetch events");

            assert_eq!(events.len(), 5);

            assert_eq!(events[0].occurred_at(), 1_000_000_000);
            assert_eq!(events[0].data()["key"], "value");

            assert_eq!(events[1].occurred_at(), 2_500_000_000);
            assert_eq!(events[1].data()["message"], "passthrough");

            assert_eq!(events[2].occurred_at(), 3_000_000_000);
            assert_eq!(events[2].data()["message"], "newmessage");

            assert_eq!(events[3].occurred_at(), 3_000_000_002);
            assert_eq!(events[3].data()["message"], "m4");

            assert_eq!(events[4].occurred_at(), 4_000_000_000);
            assert_eq!(events[4].data()["message"], "m5");
        });
    }

    fn create_event(
        conn: &PgConnection,
        room: &Room,
        occurred_at: i64,
        kind: &str,
        data: JsonValue,
    ) -> Event {
        let created_by = AgentId::new("test", AccountId::new("test", AUDIENCE));

        let opened_at = match room.time() {
            (Bound::Included(opened_at), _) => *opened_at,
            _ => panic!("Invalid room time"),
        };

        EventInsertQuery::new(room.id(), kind, &data, occurred_at, &created_by)
            .created_at(opened_at + Duration::nanoseconds(occurred_at))
            .execute(conn)
            .expect("Failed to insert event")
    }
}