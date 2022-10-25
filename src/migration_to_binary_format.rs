use anyhow::Result;
use sqlx::postgres::{PgConnection, PgPool as Db, PgQueryResult};

use crate::db::event::select_not_encoded_events;

async fn vacuum(conn: &mut PgConnection) -> sqlx::Result<PgQueryResult> {
    sqlx::query!("VACUUM ANALYZE event").execute(conn).await
}

async fn create_temp_table(conn: &mut PgConnection) -> sqlx::Result<()> {
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

async fn insert_data_into_temp_table(
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

async fn update_event_data(conn: &mut PgConnection) -> sqlx::Result<()> {
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

pub(crate) async fn run_migration(db: Db) -> Result<()> {
    let mut conn = db.acquire().await?;
    create_temp_table(&mut conn).await?;

    loop {
        let events = select_not_encoded_events(&mut conn).await?;

        if events.is_empty() {
            tracing::info!("DONE");
            break;
        }

        let mut event_ids = Vec::with_capacity(events.len());
        let mut event_binary_data = Vec::with_capacity(events.len());

        for evt in events {
            let data = evt.data();

            match evt.encode_to_binary() {
                Ok((id, Some(binary_data))) => match binary_data.to_bytes() {
                    Ok(binary_data) => {
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

        if event_ids.is_empty() {
            tracing::info!("failed to encode all events");
            break;
        }

        insert_data_into_temp_table(event_ids, event_binary_data, &mut conn).await?;
        update_event_data(&mut conn).await?;
        cleanup_temp_table(&mut conn).await?;

        vacuum(&mut conn).await?;
    }

    Ok(())
}
