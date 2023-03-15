use std::cmp;
use std::ops::Bound;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sqlx::{
    postgres::{PgConnection, PgPool as Db},
    Acquire,
};
use tracing::{info, instrument};

use crate::{
    config::AdjustConfig,
    db::{
        adjustment::{InsertQuery as AdjustmentInsertQuery, Segments},
        event::{
            DeleteQuery as EventDeleteQuery, Direction, InsertQuery as EventInsertQuery,
            ListQuery as EventListQuery, Object as Event,
        },
        room::{InsertQuery as RoomInsertQuery, Object as Room},
        room_time::RoomTimeBound,
    },
    metrics::{Metrics, QueryKey},
};

pub(crate) const NANOSECONDS_IN_MILLISECOND: i64 = 1_000_000;

////////////////////////////////////////////////////////////////////////////////

pub struct AdjustOutput {
    // Original room - with events shifted into video segments
    pub(crate) original_room: Room,
    // Modified room - same as original but has cut-start & cut-stop events applied
    pub(crate) modified_room: Room,
    // Modified segments with applied cut-starts and cut-stops - used for webinars
    pub(crate) modified_segments: Segments,
    // Initial video segments but with applied cut-starts and cut-stops - used for minigroups
    pub(crate) cut_original_segments: Segments,
}

#[instrument(
    skip_all,
    fields(
        source_room_id = %real_time_room.id(),
        started_at = ?started_at,
        segments = ?segments,
        offset = ?offset,
    )
)]
pub(crate) async fn call(
    db: &Db,
    metrics: &Metrics,
    real_time_room: &Room,
    started_at: DateTime<Utc>,
    segments: &Segments,
    offset: i64,
    cfg: AdjustConfig,
) -> Result<AdjustOutput> {
    info!("Room adjustment task started",);
    let start_timestamp = Utc::now();

    // Parse segments.
    let bounded_offset_tuples: Vec<(Bound<i64>, Bound<i64>)> = segments.to_owned().into();
    let mut parsed_segments = Vec::with_capacity(bounded_offset_tuples.len());

    for segment in bounded_offset_tuples {
        match segment {
            (Bound::Included(start), Bound::Excluded(stop)) => parsed_segments.push((start, stop)),
            segment => bail!("Invalid segment: {:?}", segment),
        }
    }

    // Create adjustment.
    let mut conn = db
        .acquire()
        .await
        .context("Failed to acquire db connection")?;

    let time = real_time_room
        .time()
        .map_err(|e| anyhow!(e))
        .context("Invalid room time")?;
    let real_time_room_new_time = if *time.end() == RoomTimeBound::Unbounded {
        let new_time = match time.update((
            Bound::Included(*time.start()),
            Bound::Excluded(start_timestamp),
        )) {
            Some(new_time) => new_time,
            None => {
                bail!(format!(
                    "Failed to update room time bound, invalid room time, room_id = '{}'",
                    real_time_room.id(),
                ))
            }
        };

        let query = crate::db::room::UpdateQuery::new(real_time_room.id())
            .time(Some(new_time.clone().into()));

        metrics
            .measure_query(QueryKey::RoomUpdateQuery, query.execute(&mut conn))
            .await
            .with_context(|| {
                format!(
                    "Failed to update room time bound, room_id = '{}'",
                    real_time_room.id(),
                )
            })?;
        new_time
    } else {
        time
    };

    let query =
        AdjustmentInsertQuery::new(real_time_room.id(), started_at, segments.to_owned(), offset);

    metrics
        .measure_query(QueryKey::AdjustmentInsertQuery, query.execute(&mut conn))
        .await
        .with_context(|| {
            format!(
                "Failed to insert adjustment, room_id = '{}'",
                real_time_room.id(),
            )
        })?;

    ///////////////////////////////////////////////////////////////////////////

    // Get room opening time and duration.
    let (room_opening, room_duration) = match real_time_room_new_time.end() {
        RoomTimeBound::Excluded(stop) => {
            let start = real_time_room_new_time.start();
            (*start, stop.signed_duration_since(*start))
        }
        _ => bail!("invalid duration for room = '{}'", real_time_room.id()),
    };

    // Calculate RTC offset as the difference between event room opening and RTC start.
    let rtc_offset = (started_at - room_opening).num_milliseconds();

    // Convert segments to nanoseconds.
    let nano_segments = parsed_segments
        .iter()
        .map(|(start, stop)| {
            let nano_start = (start + rtc_offset) * NANOSECONDS_IN_MILLISECOND;
            let nano_stop = (stop + rtc_offset) * NANOSECONDS_IN_MILLISECOND;
            (nano_start, nano_stop)
        })
        .collect::<Vec<(i64, i64)>>();

    // Invert segments to gaps.
    let min_segment_length = cfg.min_segment_length;
    let segment_gaps = invert_segments(&nano_segments, room_duration, min_segment_length)?;

    let parsed_segments_finish = parsed_segments.last().unwrap().1;

    // Calculate total duration of initial segments.
    let total_segments_millis = parsed_segments
        .into_iter()
        .fold(0, |acc, (start, stop)| acc + (stop - start));

    let total_segments_duration = Duration::milliseconds(total_segments_millis);

    // Create original room with events shifted according to segments.
    let original_room = create_room(
        &mut conn,
        metrics,
        real_time_room,
        started_at,
        total_segments_duration,
    )
    .await?;

    clone_events(&mut conn, metrics, &original_room, &segment_gaps, 0).await?;

    ///////////////////////////////////////////////////////////////////////////

    // Finds events and creates the stream events for them:
    // host                         -> stream { cut: stop }
    // break(value: true)           -> stream { cut: start }
    // break(value: false)          -> stream { cut: stop }
    // group(group: created)        -> stream { cut: start }
    // group(group: deleted)        -> stream { cut: stop }

    // Finds the first host event
    let query = EventListQuery::new()
        .room_id(original_room.id())
        .kind("host".to_string())
        .direction(Direction::Forward)
        .limit(1);

    let host_events = metrics
        .measure_query(QueryKey::EventListQuery, query.execute(&mut conn))
        .await
        .with_context(|| {
            format!(
                "failed to fetch the host event for room_id = '{}'",
                original_room.id()
            )
        })?;

    tracing::warn!(?host_events, "adjust: host events");

    let mut insert_queries = Vec::new();
    if let Some(host_event) = host_events.first() {
        let q = EventInsertQuery::new(
            original_room.id(),
            "stream".to_string(),
            json!({"cut": "stop"}),
            host_event.occurred_at(),
            host_event.created_by().to_owned(),
        )?;

        insert_queries.push(q);
    }

    // Finds break and group events
    let query = EventListQuery::new()
        .room_id(original_room.id())
        .kinds(vec!["break".to_string(), "video_group".to_string()]);

    let break_group_events = metrics
        .measure_query(QueryKey::EventListQuery, query.execute(&mut conn))
        .await
        .with_context(|| {
            format!(
                "failed to fetch break and video group events for room_id = '{}'",
                original_room.id()
            )
        })?;

    tracing::warn!(?break_group_events, "adjust: break and video_group events");

    for event in break_group_events {
        let data = if event.kind() == "break" {
            let value = event.data().get("value").and_then(|v| v.as_bool());
            match value {
                Some(true) => {
                    json!({"cut": "start"})
                }
                Some(false) => {
                    json!({"cut": "stop"})
                }
                None => continue,
            }
        } else {
            let value = event.data().get("video_group").and_then(|v| v.as_str());
            match value {
                Some("created") => {
                    json!({"cut": "start"})
                }
                Some("deleted") => {
                    json!({"cut": "stop"})
                }
                _ => continue,
            }
        };

        let q = EventInsertQuery::new(
            original_room.id(),
            "stream".to_string(),
            data,
            event.occurred_at(),
            event.created_by().to_owned(),
        )?;

        insert_queries.push(q);
    }

    if !insert_queries.is_empty() {
        let mut txn = conn
            .begin()
            .await
            .context("Failed to acquire transaction")?;

        for q in insert_queries {
            metrics
                .measure_query(QueryKey::EventInsertQuery, q.execute(&mut txn))
                .await
                .with_context(|| {
                    format!(
                        "failed to create stream event for room_id = '{}'",
                        original_room.id()
                    )
                })?;
        }

        txn.commit().await.context("Failed to commit transaction")?;
    }

    ///////////////////////////////////////////////////////////////////////////

    // Fetch shifted cut events and transform them to gaps.
    let query = EventListQuery::new()
        .room_id(original_room.id())
        .kind("stream".to_string());

    let cut_events = metrics
        .measure_query(QueryKey::EventListQuery, query.execute(&mut conn))
        .await
        .with_context(|| {
            format!(
                "failed to fetch cut events for room_id = '{}'",
                original_room.id()
            )
        })?;

    tracing::warn!(?cut_events, "adjust: cut events");

    let cut_gaps = cut_events_to_gaps(&cut_events)?;

    tracing::warn!(?cut_gaps, "adjust: cut gaps");

    let cut_original_segments = {
        let query = EventListQuery::new()
            .room_id(original_room.id())
            .kind("stream".to_string());

        let cut_events = metrics
            .measure_query(QueryKey::EventListQuery, query.execute(&mut conn))
            .await
            .with_context(|| {
                format!(
                    "failed to fetch cut events for room_id = '{}'",
                    original_room.id()
                )
            })?;

        tracing::warn!(?cut_events, "adjust: cut_events");

        let mut cut_g1 = cut_events_to_gaps(&cut_events)?;
        cut_g1.iter_mut().for_each(|(a, b)| {
            *a -= rtc_offset * NANOSECONDS_IN_MILLISECOND;
            *b -= rtc_offset * NANOSECONDS_IN_MILLISECOND;
        });

        tracing::warn!(?cut_g1, "adjust: cut_g1");

        let g1 = invert_segments(
            &cut_g1,
            Duration::milliseconds(parsed_segments_finish),
            min_segment_length,
        )?;

        tracing::warn!(?g1, "adjust: g1");

        let segments = nano_segments
            .iter()
            .map(|(a, b)| {
                let a = *a - rtc_offset * NANOSECONDS_IN_MILLISECOND;
                let b = *b - rtc_offset * NANOSECONDS_IN_MILLISECOND;
                (a, b)
            })
            .collect::<Vec<_>>();

        tracing::warn!(?segments, "adjust: segments");

        intersect::intersect(&g1, &segments)
            .into_iter()
            .map(|(start, stop)| {
                (
                    Bound::Included(start / NANOSECONDS_IN_MILLISECOND),
                    Bound::Excluded(stop / NANOSECONDS_IN_MILLISECOND),
                )
            })
            .collect::<Vec<(Bound<i64>, Bound<i64>)>>()
    };

    tracing::warn!(?cut_original_segments, "adjust: cut original segments");

    // Create modified room with events shifted again according to cut events this time.
    let modified_room = create_room(
        &mut conn,
        metrics,
        &original_room,
        started_at,
        total_segments_duration,
    )
    .await?;
    clone_events(
        &mut conn,
        metrics,
        &modified_room,
        &cut_gaps,
        offset * NANOSECONDS_IN_MILLISECOND,
    )
    .await?;

    // Delete cut events from the modified room.
    let query = EventDeleteQuery::new(modified_room.id(), "stream");

    metrics
        .measure_query(QueryKey::EventDeleteQuery, query.execute(&mut conn))
        .await
        .with_context(|| {
            format!(
                "failed to delete cut events for room_id = '{}'",
                modified_room.id()
            )
        })?;

    ///////////////////////////////////////////////////////////////////////////

    // Calculate modified segments by inverting cut gaps limited by total initial segments duration.
    let modified_segments =
        invert_segments(&cut_gaps, total_segments_duration, min_segment_length)?
            .into_iter()
            .map(|(start, stop)| {
                (
                    Bound::Included(cmp::max(start / NANOSECONDS_IN_MILLISECOND, 0)),
                    Bound::Excluded(stop / NANOSECONDS_IN_MILLISECOND),
                )
            })
            .collect::<Vec<(Bound<i64>, Bound<i64>)>>();

    tracing::warn!(?modified_segments, "adjust: modified segments");

    ///////////////////////////////////////////////////////////////////////////

    // Done.
    info!(
        duration_ms = (Utc::now() - start_timestamp).num_milliseconds(),
        "Room adjustment task successfully finished",
    );

    Ok(AdjustOutput {
        original_room,
        modified_room,
        modified_segments: Segments::from(modified_segments),
        cut_original_segments: Segments::from(cut_original_segments),
    })
}

/// Creates a derived room from the source room.
async fn create_room(
    conn: &mut PgConnection,
    metrics: &Metrics,
    source_room: &Room,
    started_at: DateTime<Utc>,
    room_duration: Duration,
) -> Result<Room> {
    let time = (
        Bound::Included(started_at),
        Bound::Excluded(started_at + room_duration),
    );
    let mut query = RoomInsertQuery::new(
        source_room.audience(),
        time.into(),
        source_room.classroom_id(),
    );
    query = query.source_room_id(source_room.id());

    if let Some(tags) = source_room.tags() {
        query = query.tags(tags.to_owned());
    }

    metrics
        .measure_query(QueryKey::RoomInsertQuery, query.execute(conn))
        .await
        .context("failed to insert room")
}

/// Clones events from the source room of the `room` with shifting them according to `gaps` and
/// adding `offset` (both in nanoseconds).
async fn clone_events(
    conn: &mut PgConnection,
    metrics: &Metrics,
    room: &Room,
    gaps: &[(i64, i64)],
    offset: i64,
) -> Result<()> {
    let source_room_id = match room.source_room_id() {
        Some(id) => id,
        None => bail!("room = '{}' is not derived from another room", room.id()),
    };

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
                FROM UNNEST($1::BIGINT[]) AS start
            ),
            gap_stops AS (
                SELECT stop, ROW_NUMBER() OVER () AS row_number
                FROM UNNEST($2::BIGINT[]) AS stop
            ),
            gaps AS (
                SELECT start, stop
                FROM gap_starts, gap_stops
                WHERE gap_stops.row_number = gap_starts.row_number
            )
        INSERT INTO event (id, room_id, kind, set, label, data, binary_data, attribute, removed, occurred_at, created_by, created_at)
        SELECT
            id,
            room_id,
            kind,
            set,
            label,
            data,
            binary_data,
            attribute,
            removed,
            -- Monotonization
            -- cutstarts and cutstops are left as is to avoid skew
            (
                CASE kind
                WHEN 'stream' THEN occurred_at
                ELSE occurred_at + ROW_NUMBER() OVER (PARTITION BY occurred_at, kind = 'stream' ORDER BY created_at) - 1
                END
            ),
            created_by,
            created_at
        FROM (
            SELECT
                gen_random_uuid() AS id,
                $3::UUID AS room_id,
                kind,
                set,
                label,
                data,
                binary_data,
                attribute,
                removed,
                (
                    CASE occurred_at <= (SELECT stop FROM gaps WHERE start = 0)
                    WHEN TRUE THEN 0
                    ELSE occurred_at - (
                        SELECT COALESCE(SUM(LEAST(stop, occurred_at) - start), 0)
                        FROM gaps
                        WHERE start < occurred_at
                        AND   start >= 0
                    )
                    END
                ) + $4 AS occurred_at,
                created_by,
                created_at
            FROM event
            WHERE room_id = $5
            AND   deleted_at IS NULL
        ) AS sub
        ",
        starts.as_slice(),
        stops.as_slice(),
        room.id(),
        sqlx::types::BigDecimal::from(offset),
        source_room_id,
    );

    metrics
        .measure_query(QueryKey::RoomAdjustCloneEventsQuery, query.execute(conn))
        .await
        .map(|_| ())
        .with_context(|| {
            format!(
                "failed to shift clone events from to room = '{}'",
                room.id()
            )
        })
}

/// Turns `segments` into gaps.
pub(crate) fn invert_segments(
    segments: &[(i64, i64)],
    room_duration: Duration,
    min_segment_length: StdDuration,
) -> Result<Vec<(i64, i64)>> {
    if segments.is_empty() {
        let total_nanos = room_duration.num_nanoseconds().unwrap_or(std::i64::MAX);
        return Ok(vec![(0, total_nanos)]);
    }

    let mut gaps = Vec::with_capacity(segments.len() + 2);

    // A possible gap before the first segment.
    if let Some((first_segment_start, _)) = segments.first() {
        if *first_segment_start > 0 {
            gaps.push((0, *first_segment_start));
        }
    }

    // Gaps between segments.
    for ((_, segment_stop), (next_segment_start, _)) in segments.iter().zip(&segments[1..]) {
        gaps.push((*segment_stop, *next_segment_start));
    }

    // A possible gap after the last segment.
    if let Some((_, last_segment_stop)) = segments.last() {
        let room_duration_nanos = room_duration.num_nanoseconds().unwrap_or(std::i64::MAX);

        // Don't create segments less than `min_segment_length`
        if *last_segment_stop < room_duration_nanos
            && StdDuration::from_nanos((room_duration_nanos - last_segment_stop) as u64)
                .gt(&min_segment_length)
        {
            gaps.push((*last_segment_stop, room_duration_nanos));
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
pub(crate) fn cut_events_to_gaps(cut_events: &[Event]) -> Result<Vec<(i64, i64)>> {
    let mut gaps = Vec::with_capacity(cut_events.len());
    let mut state: CutEventsToGapsState = CutEventsToGapsState::Started(0);

    for event in cut_events {
        let command = event.data().get("cut").and_then(|v| v.as_str());

        match (command, state) {
            (Some("start"), CutEventsToGapsState::Started(_)) => {
                state = CutEventsToGapsState::Started(event.occurred_at());
            }
            (Some("start"), CutEventsToGapsState::Stopped) => {
                state = CutEventsToGapsState::Started(event.occurred_at());
            }
            (Some("stop"), CutEventsToGapsState::Started(start)) => {
                gaps.push((start, event.occurred_at()));
                state = CutEventsToGapsState::Stopped;
            }
            // if command is stop but we've already stopped - do nothing instead of failing
            (Some("stop"), CutEventsToGapsState::Stopped) => {}
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

mod intersect;

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::ops::Bound;
    use std::time::Duration as StdDuration;

    use super::{call, AdjustOutput};

    use crate::config::AdjustConfig;
    use chrono::{DateTime, Duration, NaiveDateTime, Utc};
    use humantime::parse_duration as pd;
    use prometheus::Registry;
    use serde_json::{json, Value as JsonValue};
    use sqlx::postgres::PgConnection;
    use svc_agent::{AccountId, AgentId};

    use crate::db::adjustment::Segments;
    use crate::db::event::{
        InsertQuery as EventInsertQuery, ListQuery as EventListQuery, Object as Event,
    };
    use crate::db::room::{InsertQuery as RoomInsertQuery, Object as Room, Time as RoomTime};
    use crate::db::room_time::RoomTimeBound;
    use crate::metrics::Metrics;
    use crate::test_helpers::db::TestDb;

    const AUDIENCE: &str = "dev.svc.example.org";

    enum TestCtxState {
        Initialized,
        SegmentsSet {
            rtc_started_at: DateTime<Utc>,
            segments: Segments,
            duration: Duration,
            offset: Duration,
        },
        Ran {
            rtc_started_at: DateTime<Utc>,
            #[allow(dead_code)]
            segments: Segments,
            duration: Duration,
            #[allow(dead_code)]
            offset: Duration,
            original_room: Room,
            modified_room: Room,
            modified_segments: Segments,
            cut_original_segments: Segments,
        },
    }

    struct TestCtx {
        db: TestDb,
        room: Room,
        opened_at: DateTime<Utc>,
        state: TestCtxState,
        metrics: Metrics,
        adjust_cfg: AdjustConfig,
    }

    impl TestCtx {
        async fn get_conn(&self) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
            self.db.get_conn().await
        }

        fn get_modified_room(&self) -> &Room {
            match &self.state {
                TestCtxState::Ran { modified_room, .. } => modified_room,
                _ => panic!("Invalid state"),
            }
        }

        async fn create_event(
            &self,
            conn: &mut PgConnection,
            occurred_at: i64,
            kind: &str,
            data: JsonValue,
        ) {
            self.create_event_f(
                conn,
                occurred_at,
                kind,
                data,
                None::<fn(EventInsertQuery) -> EventInsertQuery>,
            )
            .await;
        }

        async fn create_event_f(
            &self,
            conn: &mut PgConnection,
            occurred_at: i64,
            kind: &str,
            data: JsonValue,
            f: Option<impl FnOnce(EventInsertQuery) -> EventInsertQuery>,
        ) {
            let created_by = AgentId::new("test", AccountId::new("test", AUDIENCE));

            let opened_at = match self.room.time().map(|t| t.into()) {
                Ok((Bound::Included(opened_at), _)) => opened_at,
                _ => panic!("Invalid room time"),
            };

            let mut q = EventInsertQuery::new(
                self.room.id(),
                kind.to_owned(),
                data.clone(),
                occurred_at,
                created_by,
            )
            .expect("Failed to create insert query")
            .created_at(opened_at + Duration::nanoseconds(occurred_at));

            if let Some(f) = f {
                q = f(q);
            }

            q.execute(conn).await.expect("Failed to insert event");
        }

        async fn create_event_with_attribute(
            &self,
            conn: &mut PgConnection,
            occurred_at: i64,
            kind: &str,
            data: JsonValue,
            attribute: Option<&str>,
        ) {
            self.create_event_f(
                conn,
                occurred_at,
                kind,
                data,
                Some(|q: EventInsertQuery| {
                    if let Some(p) = attribute {
                        q.attribute(p.to_owned())
                    } else {
                        q
                    }
                }),
            )
            .await;
        }

        async fn new(events: &[(i64, &str, JsonValue)]) -> Self {
            let db = TestDb::new().await;
            let metrics = Metrics::new(&Registry::new()).unwrap();
            let mut conn = db.get_conn().await;
            let opened_at = DateTime::from_utc(NaiveDateTime::from_timestamp(1582002673, 0), Utc);
            let time = RoomTime::from((Bound::Included(opened_at), Bound::Unbounded));

            let room = RoomInsertQuery::new(AUDIENCE, time, uuid::Uuid::new_v4())
                .execute(&mut conn)
                .await
                .expect("Failed to insert room");

            let ctx = Self {
                db,
                room,
                opened_at,
                metrics,
                state: TestCtxState::Initialized,
                adjust_cfg: AdjustConfig {
                    min_segment_length: StdDuration::from_secs(1),
                },
            };

            for (occurred_at, kind, data) in events {
                ctx.create_event(&mut conn, *occurred_at, kind, data.to_owned())
                    .await;
            }

            ctx
        }

        fn set_segments(
            &mut self,
            segments: Vec<(i64, i64)>,
            rtc_started_at: DateTime<Utc>,
            offset: &str,
        ) {
            assert!(matches!(self.state, TestCtxState::Initialized));

            let duration = segments.iter().fold(0, |acc, (a, b)| acc + b - a);
            let duration = Duration::milliseconds(duration);

            let segments = segments
                .into_iter()
                .map(|(a, b)| (Bound::Included(a), Bound::Excluded(b)))
                .collect::<Vec<_>>();

            let offset = Duration::from_std(pd(offset).expect("Failed to parse duration")).unwrap();

            self.state = TestCtxState::SegmentsSet {
                duration,
                rtc_started_at,
                offset,
                segments: segments.into(),
            }
        }

        fn set_ran(
            &mut self,
            original_room: Room,
            modified_room: Room,
            modified_segments: Segments,
            cut_original_segments: Segments,
        ) {
            match &mut self.state {
                TestCtxState::SegmentsSet {
                    duration,
                    segments,
                    rtc_started_at,
                    offset,
                } => {
                    let new_segments = std::mem::replace(segments, vec![].into());
                    self.state = TestCtxState::Ran {
                        duration: *duration,
                        segments: new_segments,
                        rtc_started_at: *rtc_started_at,
                        offset: *offset,
                        original_room,
                        modified_room,
                        modified_segments,
                        cut_original_segments,
                    }
                }
                _ => panic!("Wrong state"),
            }
        }

        fn modified_room(&self) -> &Room {
            match &self.state {
                TestCtxState::Ran { modified_room, .. } => modified_room,
                _ => panic!("Wrong state"),
            }
        }

        fn modified_segments(&self) -> &Segments {
            match &self.state {
                TestCtxState::Ran {
                    modified_segments, ..
                } => modified_segments,
                _ => panic!("Wrong state"),
            }
        }

        fn cut_original_segments(&self) -> &Segments {
            match &self.state {
                TestCtxState::Ran {
                    cut_original_segments,
                    ..
                } => cut_original_segments,
                _ => panic!("Wrong state"),
            }
        }

        async fn run(&mut self) {
            let (segments, rtc_started_at, offset) = match &self.state {
                TestCtxState::SegmentsSet {
                    segments,
                    rtc_started_at,
                    offset,
                    ..
                } => (segments, *rtc_started_at, *offset),
                _ => panic!("Wrong state"),
            };

            let AdjustOutput {
                original_room,
                modified_room,
                modified_segments,
                cut_original_segments,
            } = call(
                &self.db.connection_pool(),
                &self.metrics,
                &self.room,
                rtc_started_at,
                segments,
                offset.num_milliseconds(),
                self.adjust_cfg.clone(),
            )
            .await
            .expect("Room adjustment failed");

            eprintln!(
                "Og room = {:?}, mod room = {:?}",
                original_room.id(),
                modified_room.id()
            );
            self.set_ran(
                original_room,
                modified_room,
                modified_segments,
                cut_original_segments,
            );

            self.room_asserts();
        }

        fn room_asserts(&self) {
            let (original_room, modified_room, started_at, duration) = match &self.state {
                TestCtxState::Ran {
                    original_room,
                    modified_room,
                    rtc_started_at,
                    duration,
                    ..
                } => (original_room, modified_room, *rtc_started_at, *duration),
                _ => panic!("Wrong state"),
            };
            let room = &self.room;

            // Assert original room.
            assert_eq!(original_room.source_room_id(), Some(room.id()));
            assert_eq!(original_room.audience(), room.audience());
            assert_eq!(original_room.time().map(|t| *t.start()), Ok(started_at));
            assert_eq!(
                original_room.time().map(|t| t.end().to_owned()),
                Ok(RoomTimeBound::Excluded(started_at + duration))
            );
            assert_eq!(original_room.tags(), room.tags());
            assert_eq!(original_room.classroom_id(), room.classroom_id());

            // Assert modified room.
            assert_eq!(modified_room.source_room_id(), Some(original_room.id()));
            assert_eq!(modified_room.audience(), original_room.audience());
            assert_eq!(modified_room.time(), original_room.time());
            assert_eq!(modified_room.tags(), original_room.tags());
            assert_eq!(modified_room.classroom_id(), room.classroom_id());
        }

        async fn events_asserts(
            &self,
            asserts: &[(i64, &str, JsonValue)],
            segments_assert: &[(i64, i64)],
        ) {
            let mut conn = self.db.get_conn().await;
            let events = EventListQuery::new()
                .room_id(self.modified_room().id())
                .execute(&mut conn)
                .await
                .expect("Failed to fetch original room events");

            assert_eq!(events.len(), asserts.len());

            events
                .iter()
                .zip(asserts)
                .for_each(|(event, (occurred_at, kind, data))| {
                    assert_event(event, *occurred_at, kind, data);
                });

            // Assert modified segments.
            let segments: Vec<(Bound<i64>, Bound<i64>)> =
                self.modified_segments().to_owned().into();

            let segments_assert = segments_assert
                .into_iter()
                .map(|(a, b)| (Bound::Included(*a), Bound::Excluded(*b)))
                .collect::<Vec<_>>();

            assert_eq!(segments.as_slice(), segments_assert)
        }

        fn assert_cut_original_segments(&self, segments_assert: &[(i64, i64)]) {
            let segments: Vec<(Bound<i64>, Bound<i64>)> =
                self.cut_original_segments().to_owned().into();

            let segments_assert = segments_assert
                .into_iter()
                .map(|(a, b)| (Bound::Included(*a), Bound::Excluded(*b)))
                .collect::<Vec<_>>();

            assert_eq!(segments.as_slice(), segments_assert)
        }
    }

    // single stream started as soon as room opened, no preroll offset
    // all events must be left as is
    #[tokio::test]
    async fn adjust_room_test_1() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (12_000_000_000, "message", json!({"message": "m2"})),
        ])
        .await;

        // RTC started when the room opened and preroll is 0 seconds long.
        ctx.set_segments(vec![(0, 20000)], ctx.opened_at, "0 seconds");

        ctx.run().await;
        ctx.events_asserts(
            &[
                (1_000_000_000, "message", json!({"message": "m1"})),
                (12_000_000_000, "message", json!({"message": "m2"})),
            ],
            &[(0, 20000)],
        )
        .await;
    }

    // single stream started as soon as room opened, 3s preroll offset
    // all events must be moved 3s to the right
    #[tokio::test]
    async fn adjust_room_test_2() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (12_000_000_000, "message", json!({"message": "m2"})),
        ])
        .await;

        // Video segments.
        ctx.set_segments(vec![(0, 20000)], ctx.opened_at, "3 seconds");

        ctx.run().await;
        ctx.events_asserts(
            &[
                (4_000_000_000, "message", json!({"message": "m1"})),
                (15_000_000_000, "message", json!({"message": "m2"})),
            ],
            &[(0, 20000)],
        )
        .await;
    }

    // single stream started as soon as room opened, one message is after the stream end, no preroll offset
    // event after the stream end must be moved to stream end
    #[tokio::test]
    async fn adjust_room_test_3() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (21_000_000_000, "message", json!({"message": "m2"})),
        ])
        .await;

        // Video segments.
        ctx.set_segments(vec![(0, 20000)], ctx.opened_at, "0 seconds");

        ctx.run().await;
        ctx.events_asserts(
            &[
                (1_000_000_000, "message", json!({"message": "m1"})),
                (20_000_000_000, "message", json!({"message": "m2"})),
            ],
            &[(0, 20000)],
        )
        .await;
    }

    // Single stream started 10 seconds after room opened, no preroll offset
    // Both events must be moved 10s to the left
    #[tokio::test]
    async fn adjust_room_test_4() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (15_000_000_000, "message", json!({"message": "m2"})),
        ])
        .await;

        // Video segments.
        ctx.set_segments(
            vec![(0, 20000)],
            ctx.opened_at + Duration::seconds(10),
            "0 seconds",
        );

        ctx.run().await;
        ctx.events_asserts(
            &[
                (0, "message", json!({"message": "m1"})),
                (5_000_000_000, "message", json!({"message": "m2"})),
            ],
            &[(0, 20000)],
        )
        .await;
    }

    // Single stream started 10 seconds after room opened, 3s preroll offset
    // Both events must be moved 10s to the left and 3s to the right
    #[tokio::test]
    async fn adjust_room_test_5() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (15_000_000_000, "message", json!({"message": "m2"})),
            (35_000_000_000, "message", json!({"message": "m3"})),
        ])
        .await;

        // Video segments.
        ctx.set_segments(
            vec![(0, 20000)],
            ctx.opened_at + Duration::seconds(10),
            "3 seconds",
        );

        ctx.run().await;
        ctx.events_asserts(
            &[
                (3_000_000_000, "message", json!({"message": "m1"})),
                (8_000_000_000, "message", json!({"message": "m2"})),
                (23_000_000_000, "message", json!({"message": "m3"})),
            ],
            &[(0, 20000)],
        )
        .await;
    }

    // Two stream started as soon as room opened, no preroll offset
    // 1st stream messages are left as is
    // 2nd stream messages are moved with gap
    // gap messages are moved to the start of second stream
    #[tokio::test]
    async fn adjust_room_test_6() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (15_000_000_000, "message", json!({"message": "m2"})),
            (22_000_000_000, "message", json!({"message": "m3"})),
            (23_000_000_000, "message", json!({"message": "m4"})),
            (28_000_000_000, "message", json!({"message": "m5"})),
            (36_000_000_000, "message", json!({"message": "m6"})),
        ])
        .await;

        // Video segments.
        ctx.set_segments(vec![(0, 20000), (26000, 34000)], ctx.opened_at, "0 seconds");

        ctx.run().await;
        ctx.events_asserts(
            &[
                (1_000_000_000, "message", json!({"message": "m1"})),
                (15_000_000_000, "message", json!({"message": "m2"})),
                (20_000_000_000, "message", json!({"message": "m3"})),
                (20_000_000_001, "message", json!({"message": "m4"})),
                (22_000_000_000, "message", json!({"message": "m5"})),
                (28_000_000_000, "message", json!({"message": "m6"})),
            ],
            &[(0, 28000)],
        )
        .await;
    }

    // Two stream started as soon as room opened, 3s preroll offset
    // 1st stream messages are left as is
    // 2nd stream messages are moved with gap
    // gap messages are moved to the start of second stream
    #[tokio::test]
    async fn adjust_room_test_7() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (15_000_000_000, "message", json!({"message": "m2"})),
            (22_000_000_000, "message", json!({"message": "m3"})),
            (23_000_000_000, "message", json!({"message": "m4"})),
            (28_000_000_000, "message", json!({"message": "m5"})),
            (36_000_000_000, "message", json!({"message": "m6"})),
        ])
        .await;

        // Video segments.
        ctx.set_segments(vec![(0, 20000), (26000, 34000)], ctx.opened_at, "3 seconds");

        ctx.run().await;
        ctx.events_asserts(
            &[
                (4_000_000_000, "message", json!({"message": "m1"})),
                (18_000_000_000, "message", json!({"message": "m2"})),
                (23_000_000_000, "message", json!({"message": "m3"})),
                (23_000_000_001, "message", json!({"message": "m4"})),
                (25_000_000_000, "message", json!({"message": "m5"})),
                (31_000_000_000, "message", json!({"message": "m6"})),
            ],
            &[(0, 28000)],
        )
        .await;
    }

    // single stream started as soon as room opened, no preroll offset
    // single cut over 1 message
    // message in cut must be moved to cut start
    // message after the cut must be moved to cut duration seconds the left
    #[tokio::test]
    async fn adjust_room_test_8() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (10_000_000_000, "stream", json!({"cut": "start"})),
            (12_000_000_000, "message", json!({"message": "m2"})),
            (13_000_000_000, "stream", json!({"cut": "stop"})),
            (15_000_000_000, "message", json!({"message": "m3"})),
        ])
        .await;

        // RTC started when the room opened and preroll is 0 seconds long.
        ctx.set_segments(vec![(0, 20000)], ctx.opened_at, "0 seconds");

        ctx.run().await;
        ctx.events_asserts(
            &[
                (1_000_000_000, "message", json!({"message": "m1"})),
                (10_000_000_000, "message", json!({"message": "m2"})),
                (12_000_000_000, "message", json!({"message": "m3"})),
            ],
            &[(0, 10000), (13000, 20000)],
        )
        .await;
    }

    // single stream started 10 seconds after room opened, no preroll offset
    // single cut over 2 messages, ending before the stream has started
    // cut messages must be moved to stream start and monotonized
    // messages during stream must be moved 10 seconds to the left
    #[tokio::test]
    async fn adjust_room_test_9() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "stream", json!({"cut": "start"})),
            (3_000_000_000, "message", json!({"message": "m1"})),
            (6_000_000_000, "message", json!({"message": "m1a"})),
            (7_000_000_000, "stream", json!({"cut": "stop"})),
            (9_000_000_000, "message", json!({"message": "m2"})),
            (12_000_000_000, "message", json!({"message": "m3"})),
            (15_000_000_000, "message", json!({"message": "m4"})),
        ])
        .await;

        ctx.set_segments(
            vec![(0, 20000)],
            ctx.opened_at + Duration::seconds(10),
            "0 seconds",
        );

        ctx.run().await;
        ctx.events_asserts(
            &[
                (0_000_000_000, "message", json!({"message": "m1"})),
                (0_000_000_001, "message", json!({"message": "m1a"})),
                (0_000_000_002, "message", json!({"message": "m2"})),
                (2_000_000_000, "message", json!({"message": "m3"})),
                (5_000_000_000, "message", json!({"message": "m4"})),
            ],
            &[(0, 20000)],
        )
        .await;
    }

    // same as previous test but with 3s offset
    // every message must be moved as before and then 3s to the right
    #[tokio::test]
    async fn adjust_room_test_10() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "stream", json!({"cut": "start"})),
            (3_000_000_000, "message", json!({"message": "m1"})),
            (6_000_000_000, "message", json!({"message": "m1a"})),
            (7_000_000_000, "stream", json!({"cut": "stop"})),
            (9_000_000_000, "message", json!({"message": "m2"})),
            (12_000_000_000, "message", json!({"message": "m3"})),
            (15_000_000_000, "message", json!({"message": "m4"})),
        ])
        .await;

        ctx.set_segments(
            vec![(0, 20000)],
            ctx.opened_at + Duration::seconds(10),
            "3 seconds",
        );

        ctx.run().await;
        ctx.events_asserts(
            &[
                (3_000_000_000, "message", json!({"message": "m1"})),
                (3_000_000_001, "message", json!({"message": "m1a"})),
                (3_000_000_002, "message", json!({"message": "m2"})),
                (5_000_000_000, "message", json!({"message": "m3"})),
                (8_000_000_000, "message", json!({"message": "m4"})),
            ],
            &[(0, 20000)],
        )
        .await;
    }

    // single stream started 10 seconds after room opened, no preroll offset
    // two cuts, one ending after the stream start, one during the stream
    // m1 must be moved to stream start
    // m2 must be moved N seconds the left, N = stream and 1st cut overlap duration
    // m3 must be moved (start of 2nd cut + N) seconds to the left
    // m4 must be moved (2nd cut duration + N) seconds to the left
    #[tokio::test]
    async fn adjust_room_test_11() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "stream", json!({"cut": "start"})),
            (3_000_000_000, "message", json!({"message": "m1"})),
            (12_000_000_000, "stream", json!({"cut": "stop"})),
            (13_000_000_000, "message", json!({"message": "m2"})),
            (14_000_000_000, "stream", json!({"cut": "start"})),
            (15_000_000_000, "message", json!({"message": "m3"})),
            (17_000_000_000, "stream", json!({"cut": "stop"})),
            (19_000_000_000, "message", json!({"message": "m4"})),
        ])
        .await;

        ctx.set_segments(
            vec![(0, 20000)],
            ctx.opened_at + Duration::seconds(10),
            "0 seconds",
        );

        ctx.run().await;
        ctx.events_asserts(
            &[
                (0_000_000_000, "message", json!({"message": "m1"})),
                (1_000_000_000, "message", json!({"message": "m2"})),
                (2_000_000_000, "message", json!({"message": "m3"})),
                (4_000_000_000, "message", json!({"message": "m4"})),
            ],
            &[(2000, 4000), (7000, 20000)],
        )
        .await;
    }

    // same as previous test + 3s preroll
    // every message must be moved as before then 3s to the right
    #[tokio::test]
    async fn adjust_room_test_12() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "stream", json!({"cut": "start"})),
            (3_000_000_000, "message", json!({"message": "m1"})),
            (12_000_000_000, "stream", json!({"cut": "stop"})),
            (13_000_000_000, "message", json!({"message": "m2"})),
            (14_000_000_000, "stream", json!({"cut": "start"})),
            (15_000_000_000, "message", json!({"message": "m3"})),
            (17_000_000_000, "stream", json!({"cut": "stop"})),
            (19_000_000_000, "message", json!({"message": "m4"})),
        ])
        .await;

        ctx.set_segments(
            vec![(0, 20000)],
            ctx.opened_at + Duration::seconds(10),
            "3 seconds",
        );

        ctx.run().await;
        ctx.events_asserts(
            &[
                (3_000_000_000, "message", json!({"message": "m1"})),
                (4_000_000_000, "message", json!({"message": "m2"})),
                (5_000_000_000, "message", json!({"message": "m3"})),
                (7_000_000_000, "message", json!({"message": "m4"})),
            ],
            &[(2000, 4000), (7000, 20000)],
        )
        .await;
    }

    // two streams started as soon as room opened, no preroll
    // cut that overlaps streams gap
    // m2, m3, m4 must be moved cut start
    // m5 must be moved to the left according to overlap between streams and cut
    #[tokio::test]
    async fn adjust_room_test_14() {
        let mut ctx = TestCtx::new(&[
            (3_000_000_000, "message", json!({"message": "m1"})),
            (18_000_000_000, "stream", json!({"cut": "start"})),
            (19_000_000_000, "message", json!({"message": "m2"})),
            (22_000_000_000, "message", json!({"message": "m3"})),
            (29_000_000_000, "message", json!({"message": "m4"})),
            (31_000_000_000, "stream", json!({"cut": "stop"})),
            (33_000_000_000, "message", json!({"message": "m5"})),
        ])
        .await;

        ctx.set_segments(vec![(0, 20000), (28000, 34000)], ctx.opened_at, "0 seconds");

        ctx.run().await;
        ctx.events_asserts(
            &[
                (3_000_000_000, "message", json!({"message": "m1"})),
                (18_000_000_000, "message", json!({"message": "m2"})),
                (18_000_000_001, "message", json!({"message": "m3"})),
                (18_000_000_002, "message", json!({"message": "m4"})),
                (20_000_000_000, "message", json!({"message": "m5"})),
            ],
            &[(0, 18000), (23000, 26000)],
        )
        .await;
    }

    // same as previous test but 3 seconds offset
    #[tokio::test]
    async fn adjust_room_test_15() {
        let mut ctx = TestCtx::new(&[
            (3_000_000_000, "message", json!({"message": "m1"})),
            (18_000_000_000, "stream", json!({"cut": "start"})),
            (19_000_000_000, "message", json!({"message": "m2"})),
            (22_000_000_000, "message", json!({"message": "m3"})),
            (29_000_000_000, "message", json!({"message": "m4"})),
            (31_000_000_000, "stream", json!({"cut": "stop"})),
            (33_000_000_000, "message", json!({"message": "m5"})),
        ])
        .await;

        ctx.set_segments(vec![(0, 20000), (28000, 34000)], ctx.opened_at, "3 seconds");

        ctx.run().await;
        ctx.events_asserts(
            &[
                (6_000_000_000, "message", json!({"message": "m1"})),
                (21_000_000_000, "message", json!({"message": "m2"})),
                (21_000_000_001, "message", json!({"message": "m3"})),
                (21_000_000_002, "message", json!({"message": "m4"})),
                (23_000_000_000, "message", json!({"message": "m5"})),
            ],
            &[(0, 18000), (23000, 26000)],
        )
        .await;
    }

    // single stream started as soon as room opened, no preroll offset
    // single cut that ends after the stream end
    // message in cut must be moved to cut start
    // message after the cut must be moved to cut start
    #[tokio::test]
    async fn adjust_room_test_16() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (10_000_000_000, "stream", json!({"cut": "start"})),
            (12_000_000_000, "message", json!({"message": "m2"})),
            (25_000_000_000, "stream", json!({"cut": "stop"})),
            (27_000_000_000, "message", json!({"message": "m3"})),
        ])
        .await;

        // RTC started when the room opened and preroll is 0 seconds long.
        ctx.set_segments(vec![(0, 20000)], ctx.opened_at, "0 seconds");

        ctx.run().await;
        ctx.events_asserts(
            &[
                (1_000_000_000, "message", json!({"message": "m1"})),
                (10_000_000_000, "message", json!({"message": "m2"})),
                (10_000_000_001, "message", json!({"message": "m3"})),
            ],
            &[(0, 10000)],
        )
        .await;
    }

    #[tokio::test]
    async fn adjust_room_test_my() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (10_000_000_000, "stream", json!({"cut": "start"})),
            (12_000_000_000, "message", json!({"message": "m2"})),
            (13_000_000_000, "stream", json!({"cut": "stop"})),
            (15_000_000_000, "message", json!({"message": "m3"})),
        ])
        .await;

        ctx.set_segments(vec![(0, 11000), (12000, 20000)], ctx.opened_at, "0 seconds");

        ctx.run().await;
        ctx.events_asserts(
            &[
                (1_000_000_000, "message", json!({"message": "m1"})),
                (10_000_000_000, "message", json!({"message": "m2"})),
                (12_000_000_000, "message", json!({"message": "m3"})),
            ],
            &[(0, 10000), (12000, 19000)],
        )
        .await;

        ctx.assert_cut_original_segments(&[(0, 10000), (12000, 20000)])
    }

    #[tokio::test]
    async fn adjust_room_test_my2() {
        let mut ctx = TestCtx::new(&[
            (21_000_000_000, "stream", json!({"cut": "start"})),
            (24_000_000_000, "stream", json!({"cut": "stop"})),
        ])
        .await;

        ctx.set_segments(
            vec![(0, 10000), (13000, 20000)],
            ctx.opened_at + Duration::seconds(10),
            "0 seconds",
        );

        ctx.run().await;
        ctx.events_asserts(&[], &[(0, 10000), (11000, 17000)]).await;

        ctx.assert_cut_original_segments(&[(1000, 10000), (13000, 20000)])
    }

    #[tokio::test]
    async fn adjust_room_test_pin() {
        let mut ctx = TestCtx::new(&[(1_000_000_000, "message", json!({"message": "m1"}))]).await;

        {
            let mut conn = ctx.get_conn().await;
            ctx.create_event_with_attribute(
                &mut conn,
                2_000_000_000,
                "message",
                json!({"message": "m1"}),
                Some("pinned"),
            )
            .await;
        }

        ctx.set_segments(vec![(0, 10000)], ctx.opened_at, "0 seconds");

        ctx.run().await;
        ctx.events_asserts(
            &[
                (1_000_000_000, "message", json!({"message": "m1"})),
                (2_000_000_000, "message", json!({"message": "m1"})),
            ],
            &[(0, 10000)],
        )
        .await;

        ctx.assert_cut_original_segments(&[(0, 10000)]);

        {
            let mut conn = ctx.get_conn().await;
            let events = EventListQuery::new()
                .room_id(ctx.get_modified_room().id())
                .attribute("pinned")
                .execute(&mut conn)
                .await
                .unwrap();
            assert_eq!(events.len(), 1);
        }
    }

    #[tokio::test]
    async fn adjust_room_test_removed() {
        let mut ctx = TestCtx::new(&[]).await;

        {
            let mut conn = ctx.get_conn().await;
            ctx.create_event_f(
                &mut conn,
                1_000_000_000,
                "message",
                json!({"message": "m1"}),
                Some(|q: EventInsertQuery| q.removed(true)),
            )
            .await;
        }

        ctx.set_segments(vec![(0, 10000)], ctx.opened_at, "0 seconds");

        ctx.run().await;
        ctx.events_asserts(
            &[(1_000_000_000, "message", json!({"message": "m1"}))],
            &[(0, 10000)],
        )
        .await;

        ctx.assert_cut_original_segments(&[(0, 10000)]);

        {
            let mut conn = ctx.get_conn().await;
            let events = EventListQuery::new()
                .room_id(ctx.get_modified_room().id())
                .execute(&mut conn)
                .await
                .unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].removed(), true);
        }
    }

    fn assert_event(event: &Event, occurred_at: i64, kind: &str, data: &JsonValue) {
        assert_eq!(event.kind(), kind);
        assert_eq!(event.data(), data);
        assert_eq!(event.occurred_at(), occurred_at);
    }

    #[tokio::test]
    async fn adjust_room_test_host_event_as_stream_cut() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (10_000_000_000, "host", json!({})),
            (12_000_000_000, "message", json!({"message": "m2"})),
            (15_000_000_000, "message", json!({"message": "m3"})),
            (17_000_000_000, "host", json!({})),
        ])
        .await;

        ctx.set_segments(vec![(0, 20000)], ctx.opened_at, "0 seconds");
        ctx.run().await;

        let mod_segments: Vec<(Bound<i64>, Bound<i64>)> = ctx.modified_segments().to_owned().into();
        assert_eq!(
            mod_segments.as_slice(),
            &[(Bound::Included(10000), Bound::Excluded(20000))]
        )
    }

    #[tokio::test]
    async fn adjust_room_test_break_event_as_stream_cut() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (10_000_000_000, "break", json!({"value": true})),
            (12_000_000_000, "message", json!({"message": "m2"})),
            (13_000_000_000, "break", json!({"value": false})),
            (15_000_000_000, "message", json!({"message": "m3"})),
        ])
        .await;

        ctx.set_segments(vec![(0, 20000)], ctx.opened_at, "0 seconds");
        ctx.run().await;

        let mod_segments: Vec<(Bound<i64>, Bound<i64>)> = ctx.modified_segments().to_owned().into();
        assert_eq!(
            mod_segments.as_slice(),
            &[
                (Bound::Included(0), Bound::Excluded(10000)),
                (Bound::Included(13000), Bound::Excluded(20000))
            ]
        )
    }

    #[tokio::test]
    async fn adjust_room_test_video_group_event_as_stream_cut() {
        let mut ctx = TestCtx::new(&[
            (1_000_000_000, "message", json!({"message": "m1"})),
            (
                11_000_000_000,
                "video_group",
                json!({"video_group": "created"}),
            ),
            (12_000_000_000, "message", json!({"message": "m2"})),
            (
                13_000_000_000,
                "video_group",
                json!({"video_group": "deleted"}),
            ),
            (14_000_000_000, "message", json!({"message": "m3"})),
        ])
        .await;

        ctx.set_segments(vec![(0, 20000)], ctx.opened_at, "0 seconds");
        ctx.run().await;

        let mod_segments: Vec<(Bound<i64>, Bound<i64>)> = ctx.modified_segments().to_owned().into();
        assert_eq!(
            mod_segments.as_slice(),
            &[
                (Bound::Included(0), Bound::Excluded(11000)),
                (Bound::Included(13000), Bound::Excluded(20000))
            ]
        )
    }
}
