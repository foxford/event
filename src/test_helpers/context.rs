use std::future::Future;
use std::sync::Arc;

use serde_json::json;
use svc_agent::mqtt::IntoPublishableDump;
use svc_authz::ClientMap as Authz;
use svc_error::Error as SvcError;

use crate::app::context::Context;
use crate::authz_cache::AuthzCache;
use crate::config::Config;
use crate::db::ConnectionPool as Db;

use super::authz::TestAuthz;
use super::db::TestDb;
use super::SVC_AUDIENCE;

///////////////////////////////////////////////////////////////////////////////

fn build_config() -> Config {
    let id = format!("event.{}", SVC_AUDIENCE);
    let broker_id = format!("mqtt-gateway.{}", SVC_AUDIENCE);

    let config = json!({
        "id": id,
        "agent_label": "alpha",
        "broker_id": broker_id,
        "id_token": {
            "algorithm": "ES256",
            "key": "data/keys/svc.private_key.p8.der.sample",
        },
        "authn": {},
        "authz": {},
        "authz_cache": {
            "lifetime": 600,
            "vacuum_period": 3600,
        },
        "mqtt": {
            "uri": "mqtt://0.0.0.0:1883",
            "clean_session": false,
        }
    });

    serde_json::from_value::<Config>(config).expect("Failed to parse test config")
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Clone)]
pub(crate) struct TestContext {
    config: Config,
    authz: Authz,
    authz_cache: Arc<AuthzCache>,
    db: TestDb,
}

impl TestContext {
    pub(crate) fn new(authz: TestAuthz) -> Self {
        let config = build_config();
        let authz_cache = AuthzCache::new(&config.authz_cache);

        Self {
            config,
            authz: authz.into(),
            authz_cache: Arc::new(authz_cache),
            db: TestDb::new(),
        }
    }
}

impl Context for TestContext {
    fn authz(&self) -> &Authz {
        &self.authz
    }

    fn authz_cache(&self) -> &AuthzCache {
        &self.authz_cache
    }

    fn config(&self) -> &Config {
        &self.config
    }

    fn db(&self) -> &Db {
        self.db.connection_pool()
    }

    fn run_task(
        &self,
        _task: impl Future<Output = Vec<Box<dyn IntoPublishableDump>>> + Send + 'static,
    ) -> Result<(), SvcError> {
        // Skip running asynchronous tasks in tests.
        Ok(())
    }
}
