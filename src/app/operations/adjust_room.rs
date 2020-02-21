use std::ops::Bound;

use chrono::{DateTime, Duration, Utc};
use diesel::pg::PgConnection;
use failure::{bail, format_err, Error};
use log::info;

use crate::db::adjustment::{InsertQuery as AdjustmentInsertQuery, Segment};
use crate::db::event::{
    DeleteQuery as EventDeleteQuery, ListQuery as EventListQuery, Object as Event,
};
use crate::db::room::{InsertQuery as RoomInsertQuery, Object as Room};
use crate::db::ConnectionPool as Db;

pub(crate) fn call(
    db: &Db,
    real_time_room: &Room,
    started_at: DateTime<Utc>,
    segments: &[Segment],
    offset: i64,
) -> Result<(Room, Room, Vec<Segment>), Error> {
    info!(
        "Room adjustment task started for room_id = '{}'",
        real_time_room.id()
    );

    let start_timestamp = Utc::now();
    let conn = db.get()?;

    // Create adjustment.
    AdjustmentInsertQuery::new(real_time_room.id(), started_at, segments.to_vec(), offset)
        .execute(&conn)?;

    ///////////////////////////////////////////////////////////////////////////

    // Get room duration.
    let (room_opening, room_duration) = match real_time_room.time() {
        (Bound::Included(start), Bound::Excluded(stop)) if stop > start => {
            (*start, stop.signed_duration_since(*start))
        }
        _ => bail!("invalid duration for room = '{}'", real_time_room.id()),
    };

    // Invert segments to gaps.
    let segment_gaps = invert_segments(&segments, room_duration)?;

    // Calculate the difference between opening rooms in conference and event services.
    let room_opening_diff = (room_opening - started_at).num_milliseconds();

    // Create original room with events shifted according to segments.
    let original_room = create_room(&conn, &real_time_room)?;
    clone_events(
        &conn,
        &original_room,
        &segment_gaps,
        offset - room_opening_diff,
    )?;

    ///////////////////////////////////////////////////////////////////////////

    // Fetch shifted cut events and transform them to gaps.
    let cut_events = EventListQuery::new()
        .room_id(original_room.id())
        .kind("stream")
        .execute(&conn)
        .map_err(|err| {
            format_err!(
                "failed to fetch cut events for room_id = '{}': {}",
                original_room.id(),
                err
            )
        })?;

    let cut_gaps = cut_events_to_gaps(&cut_events)?;

    // Сreate modified room with events shifted again according to cut events this time.
    let modified_room = create_room(&conn, &original_room)?;
    clone_events(&conn, &modified_room, &cut_gaps, 0)?;

    // Delete cut events from the modified room.
    EventDeleteQuery::new(modified_room.id(), "stream")
        .execute(&conn)
        .map_err(|err| {
            format_err!(
                "failed to delete cut events for room_id = '{}': {}",
                modified_room.id(),
                err
            )
        })?;

    ///////////////////////////////////////////////////////////////////////////

    // Calculate modified segments by inverting cut gaps.
    let bounded_cut_gaps = cut_gaps
        .into_iter()
        .map(|(start, stop)| (Bound::Included(start), Bound::Excluded(stop)))
        .collect::<Vec<Segment>>();

    let mut modified_segments = invert_segments(&bounded_cut_gaps, room_duration)?
        .into_iter()
        .map(|(start, stop)| (Bound::Included(start), Bound::Excluded(stop)))
        .collect::<Vec<Segment>>();

    // Correct the last segment stop to be a sum of initial segments' lengths.
    if let Some((last_segment_start, _)) = modified_segments.pop() {
        let mut total_segments_length = 0;

        for segment in segments {
            if let (Bound::Included(start), Bound::Excluded(stop)) = segment {
                total_segments_length += stop - start
            }
        }

        modified_segments.push((last_segment_start, Bound::Excluded(total_segments_length)));
    }

    ///////////////////////////////////////////////////////////////////////////

    // Done.
    info!(
        "Room adjustment task successfully finished for room_id = '{}', duration = {} ms",
        real_time_room.id(),
        (Utc::now() - start_timestamp).num_milliseconds()
    );

    Ok((original_room, modified_room, modified_segments))
}

/// Creates a derived room from the source room.
fn create_room(conn: &PgConnection, source_room: &Room) -> Result<Room, Error> {
    let mut query = RoomInsertQuery::new(&source_room.audience(), source_room.time().to_owned());
    query = query.source_room_id(source_room.id());

    if let Some(tags) = source_room.tags() {
        query = query.tags(tags.to_owned());
    }

    query
        .execute(conn)
        .map_err(|err| format_err!("failed to insert room: {}", err))
}

const SHIFT_CLONE_EVENTS_SQL: &'static str = r#"
    with
        gap_starts as (
            select start, row_number() over() as row_number
            from unnest($1::bigint[]) as start
        ),
        gap_stops as (
            select stop, row_number() over() as row_number
            from unnest($2::bigint[]) as stop
        ),
        gaps as (
            select start, stop
            from gap_starts, gap_stops
            where gap_stops.row_number = gap_starts.row_number
        )
    insert into event (id, room_id, kind, set, label, data, occurred_at, created_by, created_at)
    select
        gen_random_uuid() as id,
        $3 as room_id,
        kind,
        set,
        label, 
        data,
        occurred_at - (
            select coalesce(sum(least(stop, occurred_at) - start), 0)
            from gaps
            where start < occurred_at
        ) + $4 as occurred_at,
        created_by,
        created_at
    from event
    where room_id = $5
    and deleted_at is null
"#;

/// Clones events from the source room of the `room` with shifting them according to `gaps` and
/// adding `offset`.
fn clone_events(
    conn: &PgConnection,
    room: &Room,
    gaps: &[(i64, i64)],
    offset: i64,
) -> Result<(), Error> {
    use diesel::prelude::*;
    use diesel::sql_types::{Array, Int8, Uuid};

    let source_room_id = match room.source_room_id() {
        Some(id) => id,
        None => bail!("room = '{}' is not derived from another room", room.id()),
    };

    let mut starts = Vec::with_capacity(gaps.len());
    let mut stops = Vec::with_capacity(gaps.len());

    for (start, stop) in gaps {
        starts.push(start);
        stops.push(stop);
    }

    diesel::sql_query(SHIFT_CLONE_EVENTS_SQL)
        .bind::<Array<Int8>, _>(&starts)
        .bind::<Array<Int8>, _>(&stops)
        .bind::<Uuid, _>(room.id())
        .bind::<Int8, _>(offset)
        .bind::<Uuid, _>(source_room_id)
        .execute(conn)
        .map(|_| ())
        .map_err(|err| {
            format_err!(
                "failed to shift clone events from to room = '{}': {}",
                room.id(),
                err
            )
        })
}

/// Turns `segments` into gaps.
fn invert_segments(
    segments: &[Segment],
    room_duration: Duration,
) -> Result<Vec<(i64, i64)>, Error> {
    let mut gaps = Vec::with_capacity(segments.len() + 2);

    // A possible gap before the first segment.
    if let Some(first_segment) = segments.first() {
        match first_segment {
            (Bound::Included(first_segment_start), _) => {
                if *first_segment_start > 0 {
                    gaps.push((0, *first_segment_start));
                }
            }
            _ => bail!("invalid first segment"),
        }
    }

    // Gaps between segments.
    for item in segments.iter().zip(&segments[1..]) {
        match item {
            ((_, Bound::Excluded(segment_stop)), (Bound::Included(next_segment_start), _)) => {
                gaps.push((*segment_stop, *next_segment_start));
            }
            _ => bail!("invalid segments"),
        }
    }

    // A possible gap after the last segment.
    if let Some(last_segment) = segments.last() {
        match last_segment {
            (_, Bound::Excluded(last_segment_stop)) => {
                let room_duration_millis = room_duration.num_milliseconds();

                if *last_segment_stop < room_duration_millis {
                    gaps.push((*last_segment_stop, room_duration_millis));
                }
            }
            _ => bail!("invalid last segment"),
        }
    }

    Ok(gaps)
}

#[derive(Clone, Copy, Debug)]
enum CutEventsToGapsState {
    Started(i64),
    Stopped,
}

/// Transforms cut-start/stop events ordered list to gaps list with a simple FSM.
fn cut_events_to_gaps(cut_events: &[Event]) -> Result<Vec<(i64, i64)>, Error> {
    let mut gaps = Vec::with_capacity(cut_events.len());
    let mut state: CutEventsToGapsState = CutEventsToGapsState::Stopped;

    for event in cut_events {
        let command = event.data().get("cut").and_then(|v| v.as_str());

        match (command, state) {
            (Some("start"), CutEventsToGapsState::Stopped) => {
                state = CutEventsToGapsState::Started(event.occurred_at());
            }
            (Some("stop"), CutEventsToGapsState::Started(start)) => {
                gaps.push((start, event.occurred_at()));
                state = CutEventsToGapsState::Stopped;
            }
            _ => bail!(
                "invalid cut event, id = '{}', command = {:?}, state = {:?}",
                event.id(),
                command,
                state
            ),
        }
    }

    Ok(gaps)
}

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use chrono::{DateTime, Duration, NaiveDateTime, Utc};
    use diesel::pg::PgConnection;
    use serde_json::{json, Value as JsonValue};
    use svc_agent::{AccountId, AgentId};

    use crate::db::event::{
        InsertQuery as EventInsertQuery, ListQuery as EventListQuery, Object as Event,
    };
    use crate::db::room::{InsertQuery as RoomInsertQuery, Object as Room};
    use crate::test_helpers::db::TestDb;

    const AUDIENCE: &'static str = "dev.svc.example.org";

    #[test]
    fn adjust_room() {
        futures::executor::block_on(async {
            let db = TestDb::new();

            let conn = db
                .connection_pool()
                .get()
                .expect("Failed to get db connection");

            // Create room.
            let opened_at = DateTime::from_utc(NaiveDateTime::from_timestamp(1582002673, 0), Utc);
            let closed_at = opened_at + Duration::seconds(15);
            let time = (Bound::Included(opened_at), Bound::Excluded(closed_at));

            let room = RoomInsertQuery::new(AUDIENCE, time)
                .execute(&conn)
                .expect("Failed to insert room");

            // Create events.
            create_event(&conn, &room, 1000, "message", json!({"message": "m1"}));
            create_event(&conn, &room, 2000, "message", json!({"message": "m2"}));
            create_event(&conn, &room, 3000, "stream", json!({"cut": "start"}));
            create_event(&conn, &room, 4000, "message", json!({"message": "m4"}));
            create_event(&conn, &room, 5000, "stream", json!({"cut": "stop"}));
            create_event(&conn, &room, 6000, "message", json!({"message": "m6"}));
            create_event(&conn, &room, 7000, "message", json!({"message": "m7"}));
            create_event(&conn, &room, 8000, "stream", json!({"cut": "start"}));
            create_event(&conn, &room, 9000, "message", json!({"message": "m9"}));
            create_event(&conn, &room, 10000, "stream", json!({"cut": "stop"}));
            create_event(&conn, &room, 11000, "message", json!({"message": "m11"}));
            create_event(&conn, &room, 12000, "message", json!({"message": "m12"}));
            drop(conn);

            // Video segments.
            let segments = vec![
                (Bound::Included(1500), Bound::Excluded(6500)),
                (Bound::Included(8500), Bound::Excluded(11500)),
            ];

            // Conference room was opened 50 ms earlier and preroll is 3 s long.
            let started_at = opened_at - Duration::milliseconds(50);
            let offset = 3000;

            // Call room adjustment.
            let (original_room, modified_room, modified_segments) =
                super::call(db.connection_pool(), &room, started_at, &segments, offset)
                    .expect("Room adjustment failed");

            // Assert original room.
            assert_eq!(original_room.source_room_id(), Some(room.id()));
            assert_eq!(original_room.audience(), room.audience());
            assert_eq!(original_room.time(), room.time());
            assert_eq!(original_room.tags(), room.tags());

            // Assert original room events.
            let conn = db
                .connection_pool()
                .get()
                .expect("Failed to get db connection");

            let events = EventListQuery::new()
                .room_id(original_room.id())
                .execute(&conn)
                .expect("Failed to fetch original room events");

            assert_eq!(events.len(), 12);

            // 0 + 3000 (offset) - 50 (diff)
            assert_event(&events[0], 2950, "message", &json!({"message": "m1"}));
            // 2000 - 1500 (gap 1) + 3000 (offset) - 50 (diff)
            assert_event(&events[1], 3450, "message", &json!({"message": "m2"}));
            // 3000 - 1500 (gap 1) + 3000 (offset) - 50 (diff)
            assert_event(&events[2], 4450, "stream", &json!({"cut": "start"}));
            // 4000 - 1500 (gap 1) + 3000 (offset) - 50 (diff)
            assert_event(&events[3], 5450, "message", &json!({"message": "m4"}));
            // 5000 - 1500 (gap 1) + 3000 (offset) - 50 (diff)
            assert_event(&events[4], 6450, "stream", &json!({"cut": "stop"}));
            // 6000 - 1500 (gap 1) + 3000 (offset) - 50 (diff)
            assert_event(&events[5], 7450, "message", &json!({"message": "m6"}));
            // 7000 - 1500 (gap 1) - (7000 - 6500) (gap 2 part) + 3000 (offset) - 50 (diff)
            assert_event(&events[6], 7950, "message", &json!({"message": "m7"}));
            // 8000 - 1500 (gap 1) - (8000 - 6500) (gap 2 part) + 3000 (offset) - 50 (diff)
            assert_event(&events[7], 7950, "stream", &json!({"cut": "start"}));
            // 7000 - 1500 (gap 1) - 2000 (gap 2) + 3000 (offset) - 50 (diff)
            assert_event(&events[8], 8450, "message", &json!({"message": "m9"}));
            // 9000 - 1500 (gap 1) - 2000 (gap 2) + 3000 (offset) - 50 (diff)
            assert_event(&events[9], 9450, "stream", &json!({"cut": "stop"}));
            // 10000 - 1500 (gap 1) - 2000 (gap 2) + 3000 (offset) - 50 (diff)
            assert_event(&events[10], 10450, "message", &json!({"message": "m11"}));
            // 11500 (last segment stop) - 1500 (gap 1) - 2000 (gap 2) + 3000 (offset) - 50 (diff)
            assert_event(&events[11], 10950, "message", &json!({"message": "m12"}));

            // Assert modified room.
            assert_eq!(modified_room.source_room_id(), Some(original_room.id()));
            assert_eq!(modified_room.audience(), original_room.audience());
            assert_eq!(modified_room.time(), original_room.time());
            assert_eq!(modified_room.tags(), original_room.tags());

            // Assert modified room events.
            let events = EventListQuery::new()
                .room_id(modified_room.id())
                .execute(&conn)
                .expect("Failed to fetch modified room events");

            assert_eq!(events.len(), 8);

            // 2950
            assert_event(&events[0], 2950, "message", &json!({"message": "m1"}));
            // 3450
            assert_event(&events[1], 3450, "message", &json!({"message": "m2"}));
            // 5450 - (2500 - 1500) (cut 1 part)
            assert_event(&events[2], 4450, "message", &json!({"message": "m4"}));
            // 7450 - 2000 (cut 1)
            assert_event(&events[3], 5450, "message", &json!({"message": "m6"}));
            // 7950 - 2000 (cut 1)
            assert_event(&events[4], 5950, "message", &json!({"message": "m7"}));
            // 8450 - 2000 (cut 1) - (3500 - 3000) (cut 2 part)
            assert_event(&events[5], 5950, "message", &json!({"message": "m9"}));
            // 10450 - 2000 (cut 1) - 1500 (cut 2)
            assert_event(&events[6], 6950, "message", &json!({"message": "m11"}));
            // 10950 - 2000 (cut 1) - 1500 (cut 2)
            assert_event(&events[7], 7450, "message", &json!({"message": "m12"}));

            // Assert modified segments.
            assert_eq!(
                modified_segments,
                vec![
                    (Bound::Included(0), Bound::Excluded(4450)),
                    (Bound::Included(6450), Bound::Excluded(7950)),
                    // 8000 = (6500 - 1500) + (11500 - 8500) (segments' lengths sum) + 3000 (offset)
                    (Bound::Included(9450), Bound::Excluded(8000)),
                ]
            )
        });
    }

    fn create_event(
        conn: &PgConnection,
        room: &Room,
        occurred_at: i64,
        kind: &str,
        data: JsonValue,
    ) {
        let created_by = AgentId::new("test", AccountId::new("test", AUDIENCE));

        let opened_at = match room.time() {
            (Bound::Included(opened_at), _) => *opened_at,
            _ => panic!("Invalid room time"),
        };

        EventInsertQuery::new(room.id(), kind, data, occurred_at, &created_by)
            .created_at(opened_at + Duration::milliseconds(occurred_at))
            .execute(conn)
            .expect("Failed to insert event");
    }

    fn assert_event(event: &Event, occurred_at: i64, kind: &str, data: &JsonValue) {
        assert_eq!(event.kind(), kind);
        assert_eq!(event.data(), data);
        assert_eq!(event.occurred_at(), occurred_at);
    }
}
