use anyhow::Result;
use sqlx::{
    postgres::{PgConnection, PgPool as Db, PgQueryResult},
    Connection,
};

use crate::db::event::{create_temp_table, select_not_encoded_events, update_event_data};

async fn vacuum(conn: &mut PgConnection) -> sqlx::Result<PgQueryResult> {
    sqlx::query!("VACUUM ANALYZE event").execute(conn).await
}

pub(crate) async fn run_migration(db: Db) -> Result<()> {
    {
        let mut conn = db.acquire().await?;
        create_temp_table(&mut conn).await?;
    }

    loop {
        let mut conn = db.acquire().await?;
        let events = select_not_encoded_events(&mut conn).await?;

        if events.is_empty() {
            tracing::info!("DONE");
            break;
        }

        let mut event_ids = Vec::with_capacity(events.len());
        let mut event_binary_data = Vec::with_capacity(events.len());

        for evt in events {
            let (id, binary_data) = evt.encode_to_binary()?;
            if let Some(binary_data) = binary_data {
                event_ids.push(id);
                event_binary_data.push(binary_data);
            }
        }

        let mut tx = conn.begin().await?;
        update_event_data(event_ids, event_binary_data, &mut tx).await?;
        tx.commit().await?;

        vacuum(&mut conn).await?;
    }

    Ok(())
}
