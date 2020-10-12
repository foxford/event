use std::ops::Bound;

use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::postgres::{PgConnection, PgPool as Db};

use crate::app::metrics::ProfilerKeys;
use crate::app::operations::adjust_room::{invert_segments, NANOSECONDS_IN_MILLISECOND};
use crate::db::adjustment::Segments;
use crate::db::change::{ListQuery as ChangeListQuery, Object as Change};
use crate::db::edition::Object as Edition;
use crate::db::event::{
    DeleteQuery as EventDeleteQuery, ListQuery as EventListQuery, Object as Event,
};
use crate::db::room::{InsertQuery as RoomInsertQuery, Object as Room};
use crate::profiler::Profiler;

////////////////////////////////////////////////////////////////////////////////

pub(crate) async fn call(
    db: &Db,
    profiler: &Profiler<(ProfilerKeys, Option<String>)>,
    edition: &Edition,
    source: &Room,
) -> Result<(Room, Segments)> {
    info!(
        crate::LOG,
        "Edition commit task started for edition_id = '{}', source room id = {}",
        edition.id(),
        source.id()
    );

    let start_timestamp = Utc::now();

    let mut txn = db
        .begin()
        .await
        .context("Failed to begin sqlx db transaction")?;

    let room_duration = match source.time() {
        (Bound::Included(start), Bound::Excluded(stop)) if stop > start => {
            stop.signed_duration_since(start)
        }
        _ => bail!("invalid duration for room = '{}'", source.id()),
    };

    let query = EventListQuery::new()
        .room_id(source.id())
        .kind("stream".to_string());

    let cut_events = profiler
        .measure(
            (ProfilerKeys::EventListQuery, Some("edition.commit".into())),
            query.execute(&mut txn),
        )
        .await
        .with_context(|| format!("failed to fetch cut events for room_id = '{}'", source.id()))?;

    let query = ChangeListQuery::new(edition.id()).kind("stream");

    let cut_changes = profiler
        .measure(
            (ProfilerKeys::ChangeListQuery, Some("edition.commit".into())),
            query.execute(&mut txn),
        )
        .await
        .with_context(|| {
            format!(
                "failed to fetch cut changes for room_id = '{}'",
                source.id(),
            )
        })?;

    let cut_gaps = collect_gaps(&cut_events, &cut_changes)?;
    let destination = clone_room(&mut txn, profiler, &source).await?;

    clone_events(
        &mut txn,
        profiler,
        &source,
        &destination,
        &edition,
        &cut_gaps,
    )
    .await?;

    let query = EventDeleteQuery::new(destination.id(), "stream");

    profiler
        .measure(
            (
                ProfilerKeys::EventDeleteQuery,
                Some("edition.commit".into()),
            ),
            query.execute(&mut txn),
        )
        .await
        .with_context(|| {
            format!(
                "failed to delete cut events for room_id = '{}'",
                destination.id()
            )
        })?;

    let modified_segments = invert_segments(&cut_gaps, room_duration)?
        .into_iter()
        .map(|(start, stop)| {
            (
                Bound::Included(start / NANOSECONDS_IN_MILLISECOND),
                Bound::Excluded(stop / NANOSECONDS_IN_MILLISECOND),
            )
        })
        .collect::<Vec<(Bound<i64>, Bound<i64>)>>();

    profiler
        .measure(
            (
                ProfilerKeys::EditionCommitTxnCommit,
                Some("edition.commit".into()),
            ),
            txn.commit(),
        )
        .await?;

    info!(
        crate::LOG,
        "Edition commit successfully finished for edition_id = '{}', duration = {} ms",
        edition.id(),
        (Utc::now() - start_timestamp).num_milliseconds()
    );

    Ok((destination, Segments::from(modified_segments))) as Result<(Room, Segments)>
}

async fn clone_room(
    conn: &mut PgConnection,
    profiler: &Profiler<(ProfilerKeys, Option<String>)>,
    source: &Room,
) -> Result<Room> {
    let mut query = RoomInsertQuery::new(&source.audience(), source.time().to_owned().into());
    query = query.source_room_id(source.id());

    if let Some(tags) = source.tags() {
        query = query.tags(tags.to_owned());
    }

    profiler
        .measure(
            (ProfilerKeys::RoomInsertQuery, Some("edition.commit".into())),
            query.execute(conn),
        )
        .await
        .context("Failed to insert room")
}

async fn clone_events(
    conn: &mut PgConnection,
    profiler: &Profiler<(ProfilerKeys, Option<String>)>,
    source: &Room,
    destination: &Room,
    edition: &Edition,
    gaps: &[(i64, i64)],
) -> Result<()> {
    let mut starts = Vec::with_capacity(gaps.len());
    let mut stops = Vec::with_capacity(gaps.len());

    for (start, stop) in gaps {
        starts.push(*start);
        stops.push(*stop);
    }

    let query = sqlx::query!(
        "
        WITH
            gap_starts AS (
                SELECT start, ROW_NUMBER() OVER () AS row_number
                FROM UNNEST($4::BIGINT[]) AS start
            ),
            gap_stops AS (
                SELECT stop, ROW_NUMBER() OVER () AS row_number
                FROM UNNEST($5::BIGINT[]) AS stop
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
                $2::UUID AS room_id,
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
                (CASE change.kind
                    WHEN 'addition' THEN change.event_created_by
                    ELSE event.created_by
                    END
                ) AS created_by,
                COALESCE(event.created_at, NOW()) as created_at
            FROM
                (SELECT * FROM event WHERE event.room_id = $1 AND deleted_at IS NULL)
                AS event
                FULL OUTER JOIN
                (SELECT * FROM change WHERE change.edition_id = $3)
                AS change
                ON change.event_id = event.id
            WHERE
                ((event.room_id = $1 AND deleted_at IS NULL) OR event.id IS NULL)
                AND
                ((change.edition_id = $3 AND change.kind <> 'removal') OR change.id IS NULL)
        ) AS subquery
        ",
        source.id(),
        destination.id(),
        edition.id(),
        starts.as_slice(),
        stops.as_slice(),
    );

    profiler
        .measure(
            (
                ProfilerKeys::EditionCloneEventsQuery,
                Some("edition.commit".into()),
            ),
            query.execute(conn),
        )
        .await
        .map(|_| ())
        .with_context(|| {
            format!(
                "Failed cloning events from room = '{}' to room = {}",
                source.id(),
                destination.id(),
            )
        })
}

#[derive(Clone, Copy, Debug)]
enum CutEventsToGapsState {
    Started(i64, u64),
    Stopped,
}

enum EventOrChangeAtDur<'a> {
    Event(&'a Event, i64),
    Change(&'a Change, i64),
}

// Transforms cut start-stop events and changes into a vec of (start, end) tuples.
fn collect_gaps(cut_events: &[Event], cut_changes: &[Change]) -> Result<Vec<(i64, i64)>> {
    let mut cut_vec = vec![];
    cut_events
        .iter()
        .for_each(|ev| cut_vec.push(EventOrChangeAtDur::Event(&ev, ev.occurred_at())));

    cut_changes.iter().for_each(|ch| {
        cut_vec.push(EventOrChangeAtDur::Change(
            &ch,
            ch.event_occurred_at().expect("must have occurred_at"),
        ))
    });

    cut_vec.sort_by_key(|v| match v {
        EventOrChangeAtDur::Event(_, ref k) => *k,
        EventOrChangeAtDur::Change(_, ref k) => *k,
    });

    let mut gaps = Vec::with_capacity(cut_events.len());
    let mut state: CutEventsToGapsState = CutEventsToGapsState::Stopped;

    for cut in cut_vec {
        let (command, occurred_at) = match cut {
            EventOrChangeAtDur::Event(ref event, _) => (
                event.data().get("cut").and_then(|v| v.as_str()),
                event.occurred_at(),
            ),
            EventOrChangeAtDur::Change(ref change, _) => (
                change
                    .event_data()
                    .as_ref()
                    .expect("must have event_data")
                    .get("cut")
                    .and_then(|v| v.as_str()),
                change.event_occurred_at().expect("must have occurred_at"),
            ),
        };

        match (command, &mut state) {
            (Some("start"), CutEventsToGapsState::Stopped) => {
                state = CutEventsToGapsState::Started(occurred_at, 0);
            }
            (Some("start"), CutEventsToGapsState::Started(_start, ref mut nest_lvl)) => {
                *nest_lvl += 1;
            }
            (Some("stop"), CutEventsToGapsState::Started(start, 0)) => {
                gaps.push((*start, occurred_at));
                state = CutEventsToGapsState::Stopped;
            }
            (Some("stop"), CutEventsToGapsState::Started(_start, ref mut nest_lvl)) => {
                *nest_lvl -= 1;
            }
            _ => match cut {
                EventOrChangeAtDur::Event(ref event, _) => bail!(
                    "invalid cut event, id = '{}', command = {:?}, state = {:?}",
                    event.id(),
                    command,
                    state
                ),
                EventOrChangeAtDur::Change(ref change, _) => bail!(
                    "invalid cut change, id = '{}', command = {:?}, state = {:?}",
                    change.id(),
                    command,
                    state
                ),
            },
        }
    }

    Ok(gaps)
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use chrono::Duration;
    use serde_json::{json, Value as JsonValue};
    use sqlx::postgres::PgConnection;
    use svc_agent::{AccountId, AgentId};
    use svc_authn::Authenticable;

    use crate::db::event::{
        InsertQuery as EventInsertQuery, ListQuery as EventListQuery, Object as Event,
    };

    use crate::app::metrics::ProfilerKeys;
    use crate::db::change::ChangeType;
    use crate::db::room::Object as Room;
    use crate::profiler::Profiler;
    use crate::test_helpers::db::TestDb;
    use crate::test_helpers::prelude::*;

    const AUDIENCE: &str = "dev.svc.example.org";

    #[test]
    fn commit_edition() {
        async_std::task::block_on(async {
            let profiler = Profiler::<(ProfilerKeys, Option<String>)>::start();
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_room(&mut conn).await;

            // Seed events.
            let e1 = create_event(
                &mut conn,
                &room,
                1_000_000_000,
                "message",
                json!({"message": "m1"}),
            )
            .await;

            let e2 = create_event(
                &mut conn,
                &room,
                2_000_000_000,
                "message",
                json!({"message": "m2"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                2_500_000_000,
                "message",
                json!({"message": "passthrough"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                3_000_000_000,
                "stream",
                json!({"cut": "start"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                4_000_000_000,
                "message",
                json!({"message": "m4"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                5_000_000_000,
                "stream",
                json!({"cut": "stop"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                6_000_000_000,
                "message",
                json!({"message": "m5"}),
            )
            .await;

            let edition = factory::Edition::new(room.id(), agent.agent_id())
                .insert(&mut conn)
                .await;

            factory::Change::new(edition.id(), ChangeType::Addition)
                .event_data(json!({"message": "newmessage"}))
                .event_kind("something")
                .event_set("type")
                .event_label("mylabel")
                .event_occurred_at(3_000_000_000)
                .event_created_by(&AgentId::new("barbaz", AccountId::new("foo", USR_AUDIENCE)))
                .insert(&mut conn)
                .await;

            factory::Change::new(edition.id(), ChangeType::Modification)
                .event_data(json![{"key": "value"}])
                .event_label("randomlabel")
                .event_id(e1.id())
                .insert(&mut conn)
                .await;

            factory::Change::new(edition.id(), ChangeType::Removal)
                .event_id(e2.id())
                .insert(&mut conn)
                .await;

            drop(conn);

            let (destination, segments) =
                super::call(&db.connection_pool(), &profiler, &edition, &room)
                    .await
                    .expect("edition commit failed");

            // Assert original room.
            assert_eq!(destination.source_room_id().unwrap(), room.id());
            assert_eq!(room.audience(), destination.audience());
            assert_eq!(room.tags(), destination.tags());
            let segments: Vec<(Bound<i64>, Bound<i64>)> = segments.into();
            assert_eq!(segments.len(), 2);

            let mut conn = db.get_conn().await;

            let events = EventListQuery::new()
                .room_id(destination.id())
                .execute(&mut conn)
                .await
                .expect("Failed to fetch events");

            assert_eq!(events.len(), 5);

            assert_eq!(events[0].occurred_at(), 1_000_000_000);
            assert_eq!(events[0].data()["key"], "value");

            assert_eq!(events[1].occurred_at(), 2_500_000_000);
            assert_eq!(events[1].data()["message"], "passthrough");

            assert_eq!(events[2].occurred_at(), 3_000_000_001);
            assert_eq!(events[2].data()["message"], "m4");

            assert_eq!(events[3].occurred_at(), 3_000_000_003);
            assert_eq!(events[3].data()["message"], "newmessage");
            let aid = events[3].created_by();
            assert_eq!(aid.label(), "barbaz");
            assert_eq!(aid.as_account_id().label(), "foo");

            assert_eq!(events[4].occurred_at(), 4_000_000_000);
            assert_eq!(events[4].data()["message"], "m5");
        });
    }

    #[test]
    fn commit_edition_with_cut_changes() {
        async_std::task::block_on(async {
            let profiler = Profiler::<(ProfilerKeys, Option<String>)>::start();
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_room(&mut conn).await;

            create_event(
                &mut conn,
                &room,
                2_500_000_000,
                "message",
                json!({"message": "passthrough"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                3_500_000_000,
                "message",
                json!({"message": "cutted out"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                4_200_000_000,
                "stream",
                json!({"cut": "start"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                4_500_000_000,
                "message",
                json!({"message": "some message"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                4_800_000_000,
                "stream",
                json!({"cut": "stop"}),
            )
            .await;

            let edition = factory::Edition::new(room.id(), agent.agent_id())
                .insert(&mut conn)
                .await;

            factory::Change::new(edition.id(), ChangeType::Addition)
                .event_data(json!({"cut": "start"}))
                .event_kind("stream")
                .event_set("stream")
                .event_occurred_at(3_000_000_000)
                .event_created_by(agent.agent_id())
                .insert(&mut conn)
                .await;

            factory::Change::new(edition.id(), ChangeType::Addition)
                .event_data(json!({"cut": "stop"}))
                .event_kind("stream")
                .event_set("stream")
                .event_occurred_at(4_000_000_000)
                .event_created_by(agent.agent_id())
                .insert(&mut conn)
                .await;

            drop(conn);

            let (destination, segments) =
                super::call(&db.connection_pool(), &profiler, &edition, &room)
                    .await
                    .expect("edition commit failed");

            // Assert original room.
            assert_eq!(destination.source_room_id().unwrap(), room.id());
            assert_eq!(room.audience(), destination.audience());
            assert_eq!(room.tags(), destination.tags());
            let segments: Vec<(Bound<i64>, Bound<i64>)> = segments.into();
            assert_eq!(segments.len(), 3);

            let mut conn = db.get_conn().await;

            let events = EventListQuery::new()
                .room_id(destination.id())
                .execute(&mut conn)
                .await
                .expect("Failed to fetch events");

            assert_eq!(events.len(), 3);
            assert_eq!(events[0].data()["message"], "passthrough");
            assert_eq!(events[0].occurred_at(), 2_500_000_000);
            assert_eq!(events[1].data()["message"], "cutted out");
            assert_eq!(events[1].occurred_at(), 3_000_000_000);
            assert_eq!(events[2].data()["message"], "some message");
            assert_eq!(events[2].occurred_at(), 3_200_000_001);
        });
    }

    #[test]
    fn commit_edition_with_intersecting_gaps() {
        async_std::task::block_on(async {
            let profiler = Profiler::<(ProfilerKeys, Option<String>)>::start();
            let db = TestDb::new().await;
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut conn = db.get_conn().await;
            let room = shared_helpers::insert_room(&mut conn).await;

            create_event(
                &mut conn,
                &room,
                2_500_000_000,
                "message",
                json!({"message": "passthrough"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                3_000_000_000,
                "stream",
                json!({"cut": "start"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                3_500_000_000,
                "message",
                json!({"message": "cutted out"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                4_000_000_000,
                "stream",
                json!({"cut": "stop"}),
            )
            .await;

            create_event(
                &mut conn,
                &room,
                5_000_000_000,
                "message",
                json!({"message": "passthrough2"}),
            )
            .await;

            let edition = factory::Edition::new(room.id(), agent.agent_id())
                .insert(&mut conn)
                .await;

            factory::Change::new(edition.id(), ChangeType::Addition)
                .event_data(json!({"cut": "start"}))
                .event_kind("stream")
                .event_set("stream")
                .event_occurred_at(3_200_000_000)
                .event_created_by(agent.agent_id())
                .insert(&mut conn)
                .await;

            factory::Change::new(edition.id(), ChangeType::Addition)
                .event_data(json!({"cut": "stop"}))
                .event_kind("stream")
                .event_set("stream")
                .event_occurred_at(4_500_000_000)
                .event_created_by(agent.agent_id())
                .insert(&mut conn)
                .await;

            drop(conn);

            let (destination, segments) =
                super::call(&db.connection_pool(), &profiler, &edition, &room)
                    .await
                    .expect("edition commit failed");

            // Assert original room.
            assert_eq!(destination.source_room_id().unwrap(), room.id());
            assert_eq!(room.audience(), destination.audience());
            assert_eq!(room.tags(), destination.tags());
            let segments: Vec<(Bound<i64>, Bound<i64>)> = segments.into();
            assert_eq!(segments.len(), 2);

            let mut conn = db.get_conn().await;

            let events = EventListQuery::new()
                .room_id(destination.id())
                .execute(&mut conn)
                .await
                .expect("Failed to fetch events");

            assert_eq!(events.len(), 3);
            assert_eq!(events[0].data()["message"], "passthrough");
            assert_eq!(events[0].occurred_at(), 2_500_000_000);
            assert_eq!(events[1].data()["message"], "cutted out");
            assert_eq!(events[1].occurred_at(), 3_000_000_001);
            assert_eq!(events[2].data()["message"], "passthrough2");
            assert_eq!(events[2].occurred_at(), 3_500_000_000);
        });
    }

    async fn create_event(
        conn: &mut PgConnection,
        room: &Room,
        occurred_at: i64,
        kind: &str,
        data: JsonValue,
    ) -> Event {
        let created_by = AgentId::new("test", AccountId::new("test", AUDIENCE));

        let opened_at = match room.time() {
            (Bound::Included(opened_at), _) => opened_at,
            _ => panic!("Invalid room time"),
        };

        EventInsertQuery::new(room.id(), kind.to_owned(), data, occurred_at, created_by)
            .created_at(opened_at + Duration::nanoseconds(occurred_at))
            .execute(conn)
            .await
            .expect("Failed to insert event")
    }
}
