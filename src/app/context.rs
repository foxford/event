use std::sync::Arc;

use svc_agent::AgentId;
use svc_authz::ClientMap as Authz;

use crate::config::Config;
use crate::db::ConnectionPool as Db;

#[derive(Clone)]
pub(crate) struct AppContext {
    config: Arc<Config>,
    authz: Authz,
    db: Db,
    agent_id: AgentId,
}

impl AppContext {
    pub(crate) fn new(config: Config, authz: Authz, db: Db) -> Self {
        let agent_id = AgentId::new(&config.agent_label, config.id.to_owned());

        Self {
            config: Arc::new(config),
            authz,
            db,
            agent_id,
        }
    }
}

pub(crate) trait Context: Sync {
    fn authz(&self) -> &Authz;
    fn config(&self) -> &Config;
    fn db(&self) -> &Db;
    fn agent_id(&self) -> &AgentId;
}

impl Context for AppContext {
    fn authz(&self) -> &Authz {
        &self.authz
    }

    fn config(&self) -> &Config {
        &self.config
    }

    fn db(&self) -> &Db {
        &self.db
    }

    fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }
}
