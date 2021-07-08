use std::time::Instant;

use anyhow::{Context, Result};
use rusoto_s3::PutObjectRequest;
use sqlx::postgres::PgPool as Db;

use crate::app::metrics::ProfilerKeys;
use crate::app::s3_client::S3Client;
use crate::db::event::{ListQuery as EventListQuery, Object as Event};
use crate::db::room::Object as Room;
use crate::profiler::Profiler;

////////////////////////////////////////////////////////////////////////////////

struct S3Destination {
    bucket: String,
    key: String,
}

pub(crate) async fn call(
    db: &Db,
    profiler: &Profiler<(ProfilerKeys, Option<String>)>,
    s3_client: S3Client,
    room: &Room,
) -> Result<String> {
    info!(
        crate::LOG,
        "Dump events to S3 task started, room id = {}",
        room.id()
    );

    let start_timestamp = Instant::now();

    let destination = s3_destination(room);

    let events = load_room_events(db, profiler, room).await?;

    let s3_uri = upload_events(s3_client, events, destination).await?;

    info!(
        crate::LOG,
        "Dump events to S3 task started, room id = {}, duration = {} ms",
        room.id(),
        start_timestamp.elapsed().as_millis()
    );

    Ok(s3_uri)
}

async fn load_room_events(
    db: &Db,
    profiler: &Profiler<(ProfilerKeys, Option<String>)>,
    room: &Room,
) -> Result<Vec<Event>> {
    let mut conn = db.acquire().await.context("Failed to get db connection")?;

    let query = EventListQuery::new().room_id(room.id());
    let events = profiler
        .measure(
            (
                ProfilerKeys::EventDumpQuery,
                Some("room.dump_events".into()),
            ),
            query.execute(&mut conn),
        )
        .await
        .with_context(|| format!("failed to fetch events for room_id = '{}'", room.id()))?;

    Ok(events)
}

async fn upload_events(
    s3_client: S3Client,
    events: Vec<Event>,
    destination: S3Destination,
) -> Result<String> {
    let S3Destination { bucket, key } = destination;
    let s3_uri = format!("s3://{}/{}", bucket, key);

    let events = async_std::task::spawn_blocking(move || {
        serde_json::to_vec(&events)
            .map_err(|e| anyhow!("Failed to serialize events, reason = {:?}", e))
    })
    .await?;

    let request = PutObjectRequest {
        bucket,
        key,
        body: Some(events.into()),
        ..Default::default()
    };

    s3_client
        .put_object(request)
        .await
        .map_err(|e| anyhow!("Failed to upload events to s3, reason = {:?}", e))?;

    Ok(s3_uri)
}

fn s3_destination(room: &Room) -> S3Destination {
    S3Destination {
        bucket: format!("eventsdump.{}", room.audience()),
        key: format!("{}.json", room.id()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::prelude::*;

    use serde_json::{json, Value as JsonValue};
    use sqlx::postgres::PgConnection;

    use crate::db::event::InsertQuery as EventInsertQuery;
    use std::ops::Bound;
    use svc_agent::{AccountId, AgentId};

    use crate::test_helpers::USR_AUDIENCE;

    #[test]
    fn test_upload() {
        async_std::task::block_on(async {
            let db = TestDb::new().await;

            let room = {
                let mut conn = db.get_conn().await;
                let room = shared_helpers::insert_room(&mut conn).await;

                create_event(
                    &mut conn,
                    &room,
                    19_000_000_000,
                    "message",
                    json!({"message": "m9"}),
                )
                .await;

                create_event(
                    &mut conn,
                    &room,
                    20_000_000_000,
                    "stream",
                    json!({"cut": "stop"}),
                )
                .await;

                create_event(
                    &mut conn,
                    &room,
                    21_000_000_000,
                    "message",
                    json!({"message": "m11"}),
                )
                .await;

                room
            };

            let mut context = TestContext::new(db, TestAuthz::new());
            context.set_s3(shared_helpers::mock_s3());

            let s3_uri = super::call(
                context.db(),
                &context.profiler(),
                context.s3_client().unwrap(),
                &room,
            )
            .await
            .expect("No failure");
            assert_eq!(
                s3_uri,
                format!("s3://eventsdump.{}/{}.json", room.audience(), room.id())
            );
        });
    }

    async fn create_event(
        conn: &mut PgConnection,
        room: &Room,
        occurred_at: i64,
        kind: &str,
        data: JsonValue,
    ) {
        let created_by = AgentId::new("test", AccountId::new("test", USR_AUDIENCE));

        let opened_at = match room.time().map(|t| t.into()) {
            Ok((Bound::Included(opened_at), _)) => opened_at,
            _ => panic!("Invalid room time"),
        };

        EventInsertQuery::new(
            room.id(),
            kind.to_owned(),
            data.clone(),
            occurred_at,
            created_by,
        )
        .created_at(opened_at + chrono::Duration::nanoseconds(occurred_at))
        .execute(conn)
        .await
        .expect("Failed to insert event");
    }
}
