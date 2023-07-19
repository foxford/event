use std::ops::Bound;

use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::postgres::{PgConnection, PgPool as Db};
use tracing::{info, instrument};

use crate::config::AdjustConfig;
use crate::db::change::{ListQuery as ChangeListQuery, Object as Change};
use crate::db::edition::Object as Edition;
use crate::db::event::{
    DeleteQuery as EventDeleteQuery, ListQuery as EventListQuery, Object as Event,
};
use crate::db::room::{InsertQuery as RoomInsertQuery, Object as Room};
use crate::db::room_time::RoomTimeBound;
use crate::{
    app::operations::adjust_room::{segments::invert_segments, NANOSECONDS_IN_MILLISECOND},
    metrics::Metrics,
};
use crate::{db::adjustment::Segments, metrics::QueryKey};

////////////////////////////////////////////////////////////////////////////////

#[instrument(
    skip_all,
    fields(
        source_room_id = %source.id(),
        edition_id = %edition.id(),
        offset = ?offset,
    )
)]
pub async fn call(
    db: &Db,
    metrics: &Metrics,
    edition: &Edition,
    source: &Room,
    offset: i64,
    cfg: AdjustConfig,
) -> Result<(Room, Segments)> {
    info!("Edition commit task started");

    let start_timestamp = Utc::now();

    let mut txn = db
        .begin()
        .await
        .context("Failed to begin sqlx db transaction")?;

    let room_duration = match source.time() {
        Ok(t) => match t.end() {
            RoomTimeBound::Excluded(stop) => stop.signed_duration_since(*t.start()),
            _ => bail!("invalid duration for room = '{}'", source.id()),
        },
        _ => bail!("invalid duration for room = '{}'", source.id()),
    };

    let query = EventListQuery::new()
        .room_id(source.id())
        .kind("stream".to_string());

    let cut_events = metrics
        .measure_query(QueryKey::EventListQuery, query.execute(&mut txn))
        .await
        .with_context(|| format!("failed to fetch cut events for room_id = '{}'", source.id()))?;

    let query = ChangeListQuery::new(edition.id()).kind("stream");

    let cut_changes = metrics
        .measure_query(QueryKey::ChangeListQuery, query.execute(&mut txn))
        .await
        .with_context(|| {
            format!(
                "failed to fetch cut changes for room_id = '{}'",
                source.id(),
            )
        })?;

    let cut_gaps = collect_gaps(&cut_events, &cut_changes)?;
    let destination = clone_room(&mut txn, metrics, source).await?;

    clone_events(
        &mut txn,
        metrics,
        source,
        &destination,
        edition,
        &cut_gaps,
        offset * NANOSECONDS_IN_MILLISECOND,
    )
    .await?;

    let query = EventDeleteQuery::new(destination.id(), "stream");

    metrics
        .measure_query(QueryKey::EventDeleteQuery, query.execute(&mut txn))
        .await
        .with_context(|| {
            format!(
                "failed to delete cut events for room_id = '{}'",
                destination.id()
            )
        })?;

    let modified_segments = invert_segments(&cut_gaps, room_duration, cfg.min_segment_length)?
        .into_iter()
        .map(|(start, stop)| {
            (
                Bound::Included(start / NANOSECONDS_IN_MILLISECOND),
                Bound::Excluded(stop / NANOSECONDS_IN_MILLISECOND),
            )
        })
        .collect::<Vec<(Bound<i64>, Bound<i64>)>>();

    metrics
        .measure_query(QueryKey::EditionCommitTxnCommit, txn.commit())
        .await?;

    info!(
        duration_ms = (Utc::now() - start_timestamp).num_milliseconds(),
        destination_id = %destination.id(),
        segments = ?modified_segments,
        "Edition commit successfully finished",
    );

    Ok((destination, Segments::from(modified_segments))) as Result<(Room, Segments)>
}

async fn clone_room(conn: &mut PgConnection, metrics: &Metrics, source: &Room) -> Result<Room> {
    let time = match source.time() {
        Ok(t) => t.into(),
        Err(_e) => bail!("invalid time for room = '{}'", source.id()),
    };
    let mut query = RoomInsertQuery::new(
        source.audience(),
        time,
        source.classroom_id(),
        source.kind(),
    );
    query = query.source_room_id(source.id());

    if let Some(tags) = source.tags() {
        query = query.tags(tags.to_owned());
    }

    metrics
        .measure_query(QueryKey::RoomInsertQuery, query.execute(conn))
        .await
        .context("Failed to insert room")
}

async fn clone_events(
    conn: &mut PgConnection,
    metrics: &Metrics,
    source: &Room,
    destination: &Room,
    edition: &Edition,
    gaps: &[(i64, i64)],
    offset: i64,
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
            ),
            removed_sets AS (
                SELECT DISTINCT event_set
                FROM change
                WHERE change.edition_id = $3 AND change.kind = 'bulk_removal'
            )
        INSERT INTO event (id, room_id, kind, set, label, data, binary_data, occurred_at, created_by, created_at)
        SELECT
            id,
            room_id,
            kind,
            set,
            label,
            data,
            binary_data,
            occurred_at + ROW_NUMBER() OVER (partition by occurred_at order by created_at) - 1 + $6,
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
                event.binary_data,
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
                (SELECT * FROM event 
                    WHERE   event.room_id = $1 
                        AND deleted_at IS NULL 
                        AND event.set NOT IN (SELECT event_set FROM removed_sets)
                ) AS event
                FULL OUTER JOIN
                (SELECT * FROM change WHERE change.edition_id = $3 AND change.kind <> 'bulk_removal')
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
        sqlx::types::BigDecimal::from(offset)
    );

    metrics
        .measure_query(QueryKey::EditionCloneEventsQuery, query.execute(conn))
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
        .for_each(|ev| cut_vec.push(EventOrChangeAtDur::Event(ev, ev.occurred_at())));

    cut_changes.iter().for_each(|ch| {
        cut_vec.push(EventOrChangeAtDur::Change(
            ch,
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
            EventOrChangeAtDur::Event(event, _) => (
                event.data().get("cut").and_then(|v| v.as_str()),
                event.occurred_at(),
            ),
            EventOrChangeAtDur::Change(change, _) => (
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
                EventOrChangeAtDur::Event(event, _) => bail!(
                    "invalid cut event, id = '{}', command = {:?}, state = {:?}",
                    event.id(),
                    command,
                    state
                ),
                EventOrChangeAtDur::Change(change, _) => bail!(
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
    use std::ops::Bound::{Excluded, Included};
    use std::time::Duration as StdDuration;

    use chrono::{Duration, SubsecRound, TimeZone, Utc};
    use prometheus::Registry;
    use serde_json::{json, Value as JsonValue};
    use sqlx::postgres::PgConnection;
    use svc_agent::{AccountId, AgentId};
    use svc_authn::Authenticable;

    use crate::app::operations::adjust_room::{
        segments::invert_segments, NANOSECONDS_IN_MILLISECOND,
    };
    use crate::app::operations::commit_edition::collect_gaps;
    use crate::config::AdjustConfig;
    use crate::db::event::{ListQuery as EventListQuery, Object as Event};
    use crate::db::room::{ClassType, Object as Room};
    use crate::test_helpers::db::TestDb;
    use crate::test_helpers::prelude::*;
    use crate::{db::change::ChangeType, metrics::Metrics};

    const AUDIENCE: &str = "dev.svc.example.org";

    #[tokio::test]
    async fn commit_edition() {
        let metrics = Metrics::new(&Registry::new()).unwrap();
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

        let adjust_cfg = AdjustConfig {
            min_segment_length: StdDuration::from_secs(1),
        };
        let (destination, segments) = super::call(
            &db.connection_pool(),
            &metrics,
            &edition,
            &room,
            0,
            adjust_cfg,
        )
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

        assert_eq!(events[2].occurred_at(), 3_000_000_000);
        assert_eq!(events[2].data()["message"], "newmessage");
        let aid = events[2].created_by();
        assert_eq!(aid.label(), "barbaz");
        assert_eq!(aid.as_account_id().label(), "foo");

        assert_eq!(events[3].occurred_at(), 3_000_000_002);
        assert_eq!(events[3].data()["message"], "m4");

        assert_eq!(events[4].occurred_at(), 4_000_000_000);
        assert_eq!(events[4].data()["message"], "m5");
    }

    #[tokio::test]
    async fn commit_edition_with_cut_changes() {
        let metrics = Metrics::new(&Registry::new()).unwrap();
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let classroom_id = uuid::Uuid::new_v4();
        let mut conn = db.get_conn().await;
        let now = Utc::now().trunc_subsecs(0);
        let room = factory::Room::new(classroom_id, ClassType::Webinar)
            .audience(USR_AUDIENCE)
            .time((
                Bound::Included(now),
                Bound::Excluded(now + Duration::hours(1)),
            ))
            .tags(&json!({ "webinar_id": "123" }))
            .insert(&mut conn)
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
            3_500_000_000,
            "message",
            json!({"message": "cut out"}),
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

        let adjust_cfg = AdjustConfig {
            min_segment_length: StdDuration::from_secs(1),
        };
        let (destination, segments) = super::call(
            &db.connection_pool(),
            &metrics,
            &edition,
            &room,
            0,
            adjust_cfg,
        )
        .await
        .expect("edition commit failed");

        // Assert original room.
        assert_eq!(destination.source_room_id().unwrap(), room.id());
        assert_eq!(room.audience(), destination.audience());
        assert_eq!(room.tags(), destination.tags());
        assert_eq!(classroom_id, destination.classroom_id());
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
        assert_eq!(events[1].data()["message"], "cut out");
        assert_eq!(events[1].occurred_at(), 3_000_000_001);
        assert_eq!(events[2].data()["message"], "some message");
        assert_eq!(events[2].occurred_at(), 3_200_000_001);
    }

    #[tokio::test]
    async fn commit_edition_with_intersecting_gaps() {
        let metrics = Metrics::new(&Registry::new()).unwrap();
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

        let adjust_cfg = AdjustConfig {
            min_segment_length: StdDuration::from_secs(1),
        };
        let (destination, segments) = super::call(
            &db.connection_pool(),
            &metrics,
            &edition,
            &room,
            0,
            adjust_cfg,
        )
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
    }

    async fn create_event(
        conn: &mut PgConnection,
        room: &Room,
        occurred_at: i64,
        kind: &str,
        data: JsonValue,
    ) -> Event {
        let created_by = AgentId::new("test", AccountId::new("test", AUDIENCE));

        let opened_at = match room.time().map(|t| t.into()) {
            Ok((Included(opened_at), _)) => opened_at,
            _ => panic!("Invalid room time"),
        };

        factory::Event::new()
            .room_id(room.id())
            .kind(kind)
            .data(&data)
            .occurred_at(occurred_at)
            .created_by(&created_by)
            .created_at(opened_at + Duration::nanoseconds(occurred_at))
            .insert(conn)
            .await
    }

    #[tokio::test]
    async fn commit_edition_with_min_segment_length() {
        let db = TestDb::new().await;
        let mut conn = db.get_conn().await;
        let classroom_id = uuid::Uuid::new_v4();
        let t1 =
            Utc.with_ymd_and_hms(2022, 12, 29, 11, 00, 57).unwrap() + Duration::milliseconds(88);
        let t2 =
            Utc.with_ymd_and_hms(2022, 12, 29, 11, 39, 22).unwrap() + Duration::milliseconds(888);
        let room_duration = t2.signed_duration_since(t1);

        let room = factory::Room::new(classroom_id, ClassType::Webinar)
            .audience(USR_AUDIENCE)
            .time((Bound::Included(t1), Bound::Excluded(t2)))
            .tags(&json!({ "webinar_id": "123" }))
            .insert(&mut conn)
            .await;

        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);

        let edition = factory::Edition::new(room.id(), agent.agent_id())
            .insert(&mut conn)
            .await;

        let ch1 = factory::Change::new(edition.id(), ChangeType::Addition)
            .event_data(json!({"cut": "start"}))
            .event_kind("stream")
            .event_set("stream")
            .event_occurred_at(2243994000000)
            .event_created_by(agent.agent_id())
            .insert(&mut conn)
            .await;

        let ch2 = factory::Change::new(edition.id(), ChangeType::Addition)
            .event_data(json!({"cut": "stop"}))
            .event_kind("stream")
            .event_set("stream")
            .event_occurred_at(2263628000000)
            .event_created_by(agent.agent_id())
            .insert(&mut conn)
            .await;

        let ch3 = factory::Change::new(edition.id(), ChangeType::Addition)
            .event_data(json!({"cut": "start"}))
            .event_kind("stream")
            .event_set("stream")
            .event_occurred_at(2273725000000)
            .event_created_by(agent.agent_id())
            .insert(&mut conn)
            .await;

        let ch4 = factory::Change::new(edition.id(), ChangeType::Addition)
            .event_data(json!({"cut": "stop"}))
            .event_kind("stream")
            .event_set("stream")
            .event_occurred_at(2305000000000)
            .event_created_by(agent.agent_id())
            .insert(&mut conn)
            .await;

        let cut_gaps = collect_gaps(&[], &[ch1, ch2, ch3, ch4]).unwrap();

        let modified_segments =
            invert_segments(&cut_gaps, room_duration, StdDuration::from_secs(1))
                .unwrap()
                .into_iter()
                .map(|(start, stop)| {
                    (
                        Included(start / NANOSECONDS_IN_MILLISECOND),
                        Bound::Excluded(stop / NANOSECONDS_IN_MILLISECOND),
                    )
                })
                .collect::<Vec<(Bound<i64>, Bound<i64>)>>();

        assert_eq!(
            modified_segments,
            &[
                (Included(0), Excluded(2243994)),
                (Included(2263628), Excluded(2273725))
            ]
        )
    }
}
