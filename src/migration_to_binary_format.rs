use std::{
    ffi::OsStr,
    str::FromStr,
    sync::{atomic::AtomicBool, Arc},
};

use anyhow::Result;
use sqlx::postgres::{PgConnection, PgPool as Db};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use uuid::Uuid;

use crate::db::event::select_not_encoded_events;

async fn disable_autovacuum(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query!(
        r#"
        ALTER TABLE event
        SET (autovacuum_enabled = false, toast.autovacuum_enabled = false)
        "#,
    )
    .execute(conn)
    .await?;

    Ok(())
}

async fn enable_autovacuum(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query!(
        r#"
        ALTER TABLE event
        SET (autovacuum_enabled = true, toast.autovacuum_enabled = true)
        "#,
    )
    .execute(conn)
    .await?;

    Ok(())
}

async fn vacuum(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query!("VACUUM ANALYZE event").execute(conn).await?;
    Ok(())
}

async fn create_temp_table_binary(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query!(
        r#"
        CREATE TEMP TABLE updates_table (
            id uuid NOT NULL PRIMARY KEY,
            binary_data bytea NOT NULL
        )
    "#
    )
    .execute(conn)
    .await?;

    Ok(())
}

async fn insert_data_into_temp_table_binary(
    event_ids: Vec<uuid::Uuid>,
    event_binary_data: Vec<Vec<u8>>,
    conn: &mut PgConnection,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO updates_table (id, binary_data)
        SELECT * FROM UNNEST ($1, $2)"#,
    )
    .bind(event_ids)
    .bind(event_binary_data)
    .execute(conn)
    .await?;

    Ok(())
}

async fn update_event_data_binary(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE event AS e
        SET data = NULL,
            binary_data = u.binary_data
        FROM updates_table AS u
        WHERE e.id = u.id
        "#,
    )
    .execute(conn)
    .await?;

    Ok(())
}

async fn cleanup_temp_table(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        DELETE FROM updates_table
        "#,
    )
    .execute(conn)
    .await?;

    Ok(())
}

pub(crate) async fn migrate_to_binary(db: Db, room_id: Option<Uuid>) -> Result<()> {
    {
        let mut conn = db.acquire().await?;
        disable_autovacuum(&mut conn).await?;
    }

    let stop = Arc::new(AtomicBool::new(false));
    let handle = tokio::spawn(do_migrate_to_binary(db.clone(), stop.clone(), room_id));
    tokio::spawn(async move {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(%err, "error on signal");
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    });

    if let Err(err) = handle.await {
        tracing::error!(%err, "migration failed");
    }

    {
        let mut conn = db.acquire().await?;
        enable_autovacuum(&mut conn).await?;
    }

    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Event {
    data: serde_json::Value,
    id: Uuid,
}

async fn do_migrate_to_binary(
    db: Db,
    stop: Arc<AtomicBool>,
    maybe_room_id: Option<Uuid>,
) -> Result<()> {
    let mut conn = db.acquire().await?;
    create_temp_table_binary(&mut conn).await?;

    let mut skip_rooms = Vec::new();
    let mut total_for_this_cycle = 0;
    let mut runs_in_this_cycle = 0;

    loop {
        let events =
            select_not_encoded_events(100_000, &skip_rooms, maybe_room_id, &mut conn).await?;

        if events.is_empty() {
            tracing::info!("DONE");
            break;
        }

        // all events have the same room id
        let room_id = events[0].room_id();
        let filename = format!("{room_id}.json");
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(filename)
            .await?;

        let mut event_ids = Vec::with_capacity(events.len());
        let mut event_binary_data = Vec::with_capacity(events.len());

        for evt in events {
            let data = evt.data();

            match evt.encode_to_binary() {
                Ok((id, Some(data), Some(binary_data))) => match binary_data.to_bytes() {
                    Ok(binary_data) => {
                        let evt_data = serde_json::to_vec(&Event { data, id })?;
                        file.write_all(&evt_data).await?;
                        file.write_all(b"\n").await?;

                        event_ids.push(id);
                        event_binary_data.push(binary_data);
                    }
                    Err(err) => {
                        tracing::error!(%err, ?data, "failed to encode binary data");
                    }
                },
                Ok(_) => {
                    // no data?
                }
                Err(err) => {
                    tracing::error!(%err, ?data, "failed to encode event");
                }
            }
        }

        if !event_ids.is_empty() {
            let event_count = event_ids.len();

            insert_data_into_temp_table_binary(event_ids, event_binary_data, &mut conn).await?;
            update_event_data_binary(&mut conn).await?;
            cleanup_temp_table(&mut conn).await?;

            total_for_this_cycle += event_count;
            runs_in_this_cycle += 1;

            if total_for_this_cycle > 200_000 || runs_in_this_cycle > 15 {
                vacuum(&mut conn).await?;
                total_for_this_cycle = 0;
                runs_in_this_cycle = 0;
            }
        } else {
            tracing::info!(%room_id, "failed to encode whole room");
            skip_rooms.push(room_id);
        }

        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
    }

    Ok(())
}

async fn create_temp_table_json(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query!(
        r#"
        CREATE TEMP TABLE updates_table (
            id uuid NOT NULL PRIMARY KEY,
            data jsonb NOT NULL
        )
    "#
    )
    .execute(conn)
    .await?;

    Ok(())
}

async fn insert_data_into_temp_table_json(
    event_ids: Vec<uuid::Uuid>,
    event_data: Vec<serde_json::Value>,
    conn: &mut PgConnection,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO updates_table (id, data)
        SELECT * FROM UNNEST ($1, $2)"#,
    )
    .bind(event_ids)
    .bind(event_data)
    .execute(conn)
    .await?;

    Ok(())
}

async fn update_event_data_json(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE event AS e
        SET data = u.data,
            binary_data = NULL
        FROM updates_table AS u
        WHERE e.id = u.id
        "#,
    )
    .execute(conn)
    .await?;

    Ok(())
}

pub(crate) async fn migrate_to_json(db: Db, dir: String) -> Result<()> {
    {
        let mut conn = db.acquire().await?;
        disable_autovacuum(&mut conn).await?;
    }

    let stop = Arc::new(AtomicBool::new(false));
    let handle = tokio::spawn(do_migrate_to_json(db.clone(), dir, stop.clone()));
    tokio::spawn(async move {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(%err, "error on signal");
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    });

    if let Err(err) = handle.await {
        tracing::error!(%err, "migration failed");
    }

    {
        let mut conn = db.acquire().await?;
        enable_autovacuum(&mut conn).await?;
    }

    Ok(())
}

async fn do_migrate_to_json(db: Db, dir: String, stop: Arc<AtomicBool>) -> Result<()> {
    let mut conn = db.acquire().await?;
    create_temp_table_json(&mut conn).await?;

    let mut files = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = files.next_entry().await? {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        if entry.path().extension() != Some(OsStr::new("json")) {
            continue;
        }

        let path = entry.path();
        let room_id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(room_id) => room_id,
            None => continue,
        };
        if Uuid::from_str(room_id).is_err() {
            continue;
        }

        let f = tokio::fs::File::open(path).await?;
        let f = tokio::io::BufReader::new(f);
        let mut lines = f.lines();

        let mut event_ids = Vec::new();
        let mut event_data = Vec::new();

        while let Some(line) = lines.next_line().await? {
            let event: Event = serde_json::from_str(&line)?;
            event_ids.push(event.id);
            event_data.push(event.data);
        }

        insert_data_into_temp_table_json(event_ids, event_data, &mut conn).await?;
        update_event_data_json(&mut conn).await?;
        cleanup_temp_table(&mut conn).await?;

        vacuum(&mut conn).await?;
    }

    Ok(())
}
