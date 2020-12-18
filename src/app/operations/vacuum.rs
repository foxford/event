use anyhow::{Context, Result};
use sqlx::postgres::PgPool as Db;

use crate::app::metrics::ProfilerKeys;
use crate::config::VacuumConfig;
use crate::profiler::Profiler;

pub(crate) async fn call(
    db: &Db,
    profiler: &Profiler<(ProfilerKeys, Option<String>)>,
    config: &VacuumConfig,
) -> Result<()> {
    let mut conn = db
        .acquire()
        .await
        .context("Failed to acquire db connection")?;

    let query =
        crate::db::event::VacuumQuery::new(config.max_history_size, config.max_history_lifetime);

    profiler
        .measure(
            (ProfilerKeys::EventVacuumQuery, Some("system.vacuum".into())),
            query.execute(&mut conn),
        )
        .await?;

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use chrono::{Duration, SubsecRound, Utc};
    use serde_json::json;
    use sqlx::postgres::PgConnection;
    use uuid::Uuid;

    use crate::app::metrics::ProfilerKeys;
    use crate::config::VacuumConfig;
    use crate::db::event::{ListQuery as EventListQuery, Object as Event};
    use crate::db::room::Object as Room;
    use crate::profiler::Profiler;
    use crate::test_helpers::prelude::*;

    #[test]
    fn vacuum() {
        async_std::task::block_on(async {
            let config: VacuumConfig = serde_json::from_value(json!({
                "max_history_size": 2,
                "max_history_lifetime": 3600
            }))
            .expect("Failed to parse vacuum config");

            let profiler = Profiler::<(ProfilerKeys, Option<String>)>::start();
            let db = TestDb::new().await;

            // Prepare 3 rooms.
            let mut conn = db.get_conn().await;
            let room1 = insert_room(&mut conn, false).await;
            let room2 = insert_room(&mut conn, false).await;
            let room3 = insert_room(&mut conn, true).await;

            // In the first room there's an old event and a recent event.
            let r1e1 = insert_event(&mut conn, &room1, 70).await;
            let r1e2 = insert_event(&mut conn, &room1, 30).await;

            // In the second room there's a lot of events.
            let r2e1 = insert_event(&mut conn, &room2, 3).await;
            let r2e2 = insert_event(&mut conn, &room2, 2).await;
            let r2e3 = insert_event(&mut conn, &room2, 1).await;

            // In the third room there're both cases but it's preserved.
            let r3e1 = insert_event(&mut conn, &room3, 90).await;
            let r3e2 = insert_event(&mut conn, &room3, 3).await;
            let r3e3 = insert_event(&mut conn, &room3, 2).await;
            let r3e4 = insert_event(&mut conn, &room3, 1).await;

            drop(conn);

            // Run vacuum.
            super::call(&db.connection_pool(), &profiler, &config)
                .await
                .expect("Vacuum failed");

            // Assert some events to be deleted and others don't.
            let mut conn = db.get_conn().await;

            let r1_event_ids = fetch_room_event_ids(&mut conn, &room1).await;
            assert!(!r1_event_ids.contains(&r1e1.id()));
            assert!(r1_event_ids.contains(&r1e2.id()));

            let r2_event_ids = fetch_room_event_ids(&mut conn, &room2).await;
            assert!(!r2_event_ids.contains(&r2e1.id()));
            assert!(r2_event_ids.contains(&r2e2.id()));
            assert!(r2_event_ids.contains(&r2e3.id()));

            let r3_event_ids = fetch_room_event_ids(&mut conn, &room3).await;
            assert!(r3_event_ids.contains(&r3e1.id()));
            assert!(r3_event_ids.contains(&r3e2.id()));
            assert!(r3_event_ids.contains(&r3e3.id()));
            assert!(r3_event_ids.contains(&r3e4.id()));
        });
    }

    async fn insert_room(conn: &mut PgConnection, preserve_history: bool) -> Room {
        let now = Utc::now().trunc_subsecs(0);

        let time = (
            Bound::Included(now),
            Bound::Excluded(now + Duration::hours(1)),
        );

        factory::Room::new()
            .audience(USR_AUDIENCE)
            .time(time)
            .preserve_history(preserve_history)
            .insert(conn)
            .await
    }

    async fn insert_event(conn: &mut PgConnection, room: &Room, minutes_ago: i64) -> Event {
        let creator = TestAgent::new("web", "user123", USR_AUDIENCE);

        factory::Event::new()
            .room_id(room.id())
            .kind("draw")
            .set("page1")
            .label("drawing1")
            .occurred_at(10_000_000_000_000 - minutes_ago * 60_000_000_000)
            .data(&json!({}))
            .created_at(Utc::now() - Duration::minutes(minutes_ago))
            .created_by(creator.agent_id())
            .insert(conn)
            .await
    }

    async fn fetch_room_event_ids(conn: &mut PgConnection, room: &Room) -> Vec<Uuid> {
        EventListQuery::new()
            .room_id(room.id())
            .execute(conn)
            .await
            .expect("Failed to list events")
            .into_iter()
            .map(|event| event.id())
            .collect()
    }
}
