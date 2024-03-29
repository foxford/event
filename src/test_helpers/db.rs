use std::env::var;
use std::pin::Pin;
use std::sync::Arc;

use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgPool, PgPoolOptions, Postgres};
use svc_authn::AccountId;
use svc_authz::{BanCallback, IntentObject};
use uuid::Uuid;

#[derive(Clone)]
pub struct TestDb {
    pool: PgPool,
}

impl TestDb {
    pub async fn new() -> Self {
        #[cfg(feature = "dotenv")]
        dotenv::dotenv().ok();

        let url = var("DATABASE_URL").expect("DATABASE_URL must be specified");

        Self {
            pool: PgPoolOptions::new()
                .min_connections(1)
                .max_connections(50)
                .connect(&url)
                .await
                .expect("Failed to connect to the DB"),
        }
    }

    pub fn connection_pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn get_conn(&self) -> PoolConnection<Postgres> {
        self.pool
            .acquire()
            .await
            .expect("Failed to get DB connection")
    }
}

pub fn test_db_ban_callback(db: TestDb) -> BanCallback {
    Arc::new(
        move |account_id: AccountId, intent: Box<dyn IntentObject>| {
            let db_ = db.clone();
            Box::pin(async move {
                if intent.to_ban_key().is_some() {
                    if let Some(classroom_id) = intent.to_vec().get(1) {
                        if let Ok(classroom_id) = Uuid::parse_str(classroom_id) {
                            let mut conn = db_.get_conn().await;
                            if let Ok(maybe_ban) = crate::db::room_ban::ClassroomFindQuery::new(
                                account_id.to_owned(),
                                classroom_id,
                            )
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
