use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rusoto_s3::PutObjectRequest;
use serde_derive::Serialize;
use sqlx::postgres::PgPool as Db;
use tracing::{error, info};

use crate::db::room::Object as Room;
use crate::{
    app::{
        error::{Error, ErrorKind},
        s3_client::S3Client,
    },
    metrics::Metrics,
};
use crate::{
    db::event::{ListQuery as EventListQuery, Object as Event},
    metrics::QueryKey,
};

////////////////////////////////////////////////////////////////////////////////

const RETRIES: u8 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(200);

struct S3Destination {
    bucket: String,
    key: String,
}

#[derive(Serialize)]
struct S3Content {
    room: Room,
    events: Vec<Event>,
}

pub async fn call(db: &Db, metrics: &Metrics, s3_client: S3Client, room: &Room) -> Result<String> {
    info!(room = ?room.id(), classroom_id = ?room.classroom_id(), "Dump events to S3 task started");

    let start_timestamp = Instant::now();

    let destination = s3_destination(room);

    let events = load_room_events(db, metrics, room).await?;

    let s3_uri = upload_events(s3_client, room, events, destination).await?;

    info!(
        room = ?room.id(),
        classroom_id = ?room.classroom_id(),
        duration = %start_timestamp.elapsed().as_millis(),
        "Dump events to S3 task successfully finished"
    );

    Ok(s3_uri)
}

async fn load_room_events(db: &Db, metrics: &Metrics, room: &Room) -> Result<Vec<Event>> {
    let mut conn = db.acquire().await.context("Failed to get db connection")?;

    let query = EventListQuery::new().room_id(room.id());
    let events = metrics
        .measure_query(QueryKey::EventDumpQuery, query.execute(&mut conn))
        .await
        .with_context(|| {
            format!(
                "failed to fetch events for room_id = '{}', classroom_id = {}",
                room.id(),
                room.classroom_id()
            )
        })?;

    Ok(events)
}

async fn upload_events(
    s3_client: S3Client,
    room: &Room,
    events: Vec<Event>,
    destination: S3Destination,
) -> Result<String> {
    let S3Destination { bucket, key } = destination;
    let s3_uri = format!("s3://{bucket}/{key}");

    let body = S3Content {
        room: room.to_owned(),
        events,
    };

    let classroom_id = room.classroom_id();
    let body = tokio::task::spawn_blocking(move || {
        serde_json::to_vec(&body).map_err(|e| {
            anyhow!(
                "Failed to serialize events, reason = {:?}, classroom_id = {}",
                e,
                classroom_id
            )
        })
    })
    .await
    .map_err(|e| {
        anyhow!(
            "Failed to join events serialization task, reason = {:?}, classroom_id = {}",
            e,
            room.classroom_id()
        )
    })??;

    let mut result;
    for _ in 0..RETRIES {
        let request = PutObjectRequest {
            bucket: bucket.clone(),
            key: key.clone(),
            body: Some(body.clone().into()),
            content_type: Some("application/json".into()),
            ..Default::default()
        };

        result = s3_client.put_object(request).await.map_err(|e| {
            Error::new(
                ErrorKind::S3UploadFailed,
                anyhow!(
                    "Failed to upload events to s3, reason = {:?}, classroom_id = {}",
                    e,
                    room.classroom_id()
                ),
            )
        });

        if let Err(ref e) = result {
            error!(
                room = ?room.id(),
                classroom_id = ?room.classroom_id(),
                "Dump events to S3 task errored, error = {:?}",
                result
            );

            e.notify_sentry();
            tokio::time::sleep(RETRY_DELAY).await;
        } else {
            break;
        }
    }

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

    #[tokio::test]
    async fn test_upload() {
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
            &context.metrics(),
            context.s3_client().unwrap(),
            &room,
        )
        .await
        .expect("No failure");
        assert_eq!(
            s3_uri,
            format!("s3://eventsdump.{}/{}.json", room.audience(), room.id())
        );
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
        .expect("Failed to create insert query")
        .created_at(opened_at + chrono::Duration::nanoseconds(occurred_at))
        .execute(conn)
        .await
        .expect("Failed to insert event");
    }
}
