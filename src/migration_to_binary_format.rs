use anyhow::Result;
use sqlx::postgres::{PgConnection, PgPool as Db};

use crate::db::event::{select_not_encoded_events, update_event_data};

async fn vacuum(conn: &mut PgConnection) -> sqlx::Result<sqlx::postgres::PgQueryResult> {
    sqlx::query!("VACUUM ANALYZE event").execute(conn).await
}

pub(crate) async fn run_migration(db: Db) -> Result<()> {
    loop {
        let mut conn = db.acquire().await?;
        let events = select_not_encoded_events(&mut conn).await?;

        if events.is_empty() {
            tracing::info!("DONE");
            break;
        }

        for mut evt in events {
            evt.encode_to_binary()?;
            update_event_data(evt, &mut conn).await?;
        }

        vacuum(&mut conn).await?;
    }

    Ok(())
}
