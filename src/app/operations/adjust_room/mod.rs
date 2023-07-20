use crate::{
    db::{event::Object as Event, room::Object as Room},
    metrics::{Metrics, QueryKey},
};
use anyhow::{Context, Result};
use sqlx::postgres::PgConnection;

mod intersect;

pub mod segments;
pub mod v1;
pub mod v2;

pub const NANOSECONDS_IN_MILLISECOND: i64 = 1_000_000;

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

#[derive(Clone, Copy, Debug)]
enum CutEventsToGapsState {
    Started(i64),
    Stopped,
}

/// Transforms cut-start/stop events ordered list to gaps list with a simple FSM.
pub fn cut_events_to_gaps(cut_events: &[Event]) -> anyhow::Result<Vec<(i64, i64)>> {
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
