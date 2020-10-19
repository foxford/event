use std::env::var;
use std::pin::Pin;
use std::sync::Arc;

use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgPool, PgPoolOptions, Postgres};
use svc_authn::AccountId;
use svc_authz::{BanCallback, IntentObject};
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct TestDb {
    pool: PgPool,
}

impl TestDb {
    pub(crate) async fn new() -> Self {
        let url = var("DATABASE_URL").expect("DATABASE_URL must be specified");

        let pool = PgPoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect(&url)
            .await
            .expect("Failed to connect to the DB");

        Self { pool }
    }

    pub(crate) fn connection_pool(&self) -> &PgPool {
        &self.pool
    }

    pub(crate) async fn get_conn(&self) -> PoolConnection<Postgres> {
        self.pool
            .acquire()
            .await
            .expect("Failed to get DB connection")
    }
}

pub(crate) fn test_db_ban_callback(db: TestDb) -> BanCallback {
    Arc::new(
        move |account_id: AccountId, intent: Box<dyn IntentObject>| {
            let db_ = db.clone();
            Box::pin(async move {
                if intent.to_ban_key().is_some() {
                    if let Some(room_id) = intent.to_vec().get(1) {
                        if let Ok(room_id) = Uuid::parse_str(&room_id) {
                            let mut conn = db_.get_conn().await;
                            if let Ok(maybe_ban) =
                                crate::db::room_ban::FindQuery::new(account_id.to_owned(), room_id)
                                    .execute(&mut conn)
                                    .await
                            {
                                return maybe_ban.is_some();
                            }
                        }
                    }
                }

                false
            }) as Pin<Box<dyn futures::Future<Output = bool> + Send>>
        },
    ) as BanCallback
}
