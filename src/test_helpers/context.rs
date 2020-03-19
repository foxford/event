use serde_json::json;
use svc_authz::ClientMap as Authz;

use crate::app::context::Context;
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
        "authz": {},
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
    db: TestDb,
}

impl TestContext {
    pub(crate) fn new(db: TestDb, authz: TestAuthz) -> Self {
        let config = build_config();

        Self {
            config,
            authz: authz.into(),
            db,
        }
    }
}

impl Context for TestContext {
    fn authz(&self) -> &Authz {
        &self.authz
    }

    fn config(&self) -> &Config {
        &self.config
    }

    fn db(&self) -> &Db {
        self.db.connection_pool()
    }
}
