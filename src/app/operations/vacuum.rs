use anyhow::{Context, Result};
use sqlx::postgres::PgPool as Db;

use crate::{
    config::VacuumConfig,
    metrics::{Metrics, QueryKey},
};

pub async fn call(db: &Db, metrics: &Metrics, config: &VacuumConfig) -> Result<()> {
    let mut conn = db
        .acquire()
        .await
        .context("Failed to acquire db connection")?;

    let query = crate::db::event::VacuumQuery::new(
        config.max_history_size,
        config.max_history_lifetime,
        config.max_deleted_lifetime,
    );

    metrics
        .measure_query(QueryKey::EventVacuumQuery, query.execute(&mut conn))
        .await?;

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use chrono::{Duration, SubsecRound, Utc};
    use prometheus::Registry;
    use serde_json::json;
    use serial_test::serial;
    use sqlx::postgres::PgConnection;
    use uuid::Uuid;

    use crate::config::VacuumConfig;
    use crate::db::event::{ListQuery as EventListQuery, Object as Event};
    use crate::db::room::Object as Room;
    use crate::metrics::Metrics;
    use crate::test_helpers::prelude::*;

    #[tokio::test]
    #[serial]
    async fn vacuum_history() {
        let config: VacuumConfig = serde_json::from_value(json!({
            "max_history_size": 2,
            "max_history_lifetime": 3600,
            "max_deleted_lifetime": 1_000_000,
        }))
        .expect("Failed to parse vacuum config");

        let metrics = Metrics::new(&Registry::new()).unwrap();
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
        super::call(&db.connection_pool(), &metrics, &config)
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
    }

    #[tokio::test]
    #[serial]
    async fn vacuum_deleted() {
        let config: VacuumConfig = serde_json::from_value(json!({
            "max_history_size": 100,
            "max_history_lifetime": 1_000_000,
            "max_deleted_lifetime": 3600,
        }))
        .expect("Failed to parse vacuum config");

        let metrics = Metrics::new(&Registry::new()).unwrap();
        let db = TestDb::new().await;

        // Prepare rooms.
        let mut conn = db.get_conn().await;
        let room1 = insert_room(&mut conn, false).await;
        let room2 = insert_room(&mut conn, false).await;
        let room3 = insert_room(&mut conn, false).await;
        let room4 = insert_room(&mut conn, true).await;

        // In the first room there's an old deleted label.
        let _r1e1 = insert_event(&mut conn, &room1, 75).await;
        let _r1e2 = insert_deleted_event(&mut conn, &room1, 70).await;

        // In the second room there's not too old deleted label.
        let r2e1 = insert_event(&mut conn, &room2, 55).await;
        let r2e2 = insert_deleted_event(&mut conn, &room2, 50).await;

        // In the third room there's a restored label.
        let r3e1 = insert_event(&mut conn, &room3, 100).await;
        let r3e2 = insert_event(&mut conn, &room3, 90).await;
        let r3e3 = insert_event(&mut conn, &room3, 10).await;

        // The fourth room has an old deleted label but it's preserved.
        let r4e1 = insert_event(&mut conn, &room4, 90).await;
        let r4e2 = insert_deleted_event(&mut conn, &room4, 80).await;

        drop(conn);

        // Run vacuum.
        super::call(&db.connection_pool(), &metrics, &config)
            .await
            .expect("Vacuum failed");

        // Assert some events to be deleted and others don't.
        let mut conn = db.get_conn().await;

        let r1_event_ids = fetch_room_event_ids(&mut conn, &room1).await;
        assert!(r1_event_ids.is_empty());

        let r2_event_ids = fetch_room_event_ids(&mut conn, &room2).await;
        assert!(r2_event_ids.contains(&r2e1.id()));
        assert!(r2_event_ids.contains(&r2e2.id()));

        let r3_event_ids = fetch_room_event_ids(&mut conn, &room3).await;
        assert!(r3_event_ids.contains(&r3e1.id()));
        assert!(r3_event_ids.contains(&r3e2.id()));
        assert!(r3_event_ids.contains(&r3e3.id()));

        let r4_event_ids = fetch_room_event_ids(&mut conn, &room4).await;
        assert!(r4_event_ids.contains(&r4e1.id()));
        assert!(r4_event_ids.contains(&r4e2.id()));
    }

    async fn insert_room(conn: &mut PgConnection, preserve_history: bool) -> Room {
        let now = Utc::now().trunc_subsecs(0);

        let time = (
            Bound::Included(now),
            Bound::Excluded(now + Duration::hours(1)),
        );

        factory::Room::new(Uuid::new_v4())
            .audience(USR_AUDIENCE)
            .time(time)
            .preserve_history(preserve_history)
            .insert(conn)
            .await
    }

    async fn insert_event(conn: &mut PgConnection, room: &Room, minutes_ago: i64) -> Event {
        build_event_factory(room, minutes_ago).insert(conn).await
    }

    async fn insert_deleted_event(conn: &mut PgConnection, room: &Room, minutes_ago: i64) -> Event {
        build_event_factory(room, minutes_ago)
            .attribute("deleted")
            .insert(conn)
            .await
    }

    fn build_event_factory(room: &Room, minutes_ago: i64) -> factory::Event {
        let creator = TestAgent::new("web", "user123", USR_AUDIENCE);

        factory::Event::new()
            .room_id(room.id())
            .kind("test-draw")
            .set("page1")
            .label("drawing1")
            .occurred_at(10_000_000_000_000 - minutes_ago * 60_000_000_000)
            .data(&json!({}))
            .created_at(Utc::now() - Duration::minutes(minutes_ago))
            .created_by(creator.agent_id())
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
