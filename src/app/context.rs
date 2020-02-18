use std::sync::Arc;

use futures::executor::ThreadPool;
use svc_agent::mqtt::Agent;
use svc_authz::ClientMap as Authz;

use crate::config::Config;
use crate::db::ConnectionPool as Db;

#[derive(Clone)]
pub(crate) struct Context {
    agent: Agent,
    config: Arc<Config>,
    authz: Authz,
    db: Db,
    thread_pool: Arc<ThreadPool>,
}

#[allow(dead_code)]
impl Context {
    pub(crate) fn new(
        agent: Agent,
        config: Config,
        authz: Authz,
        db: Db,
        thread_pool: Arc<ThreadPool>,
    ) -> Self {
        Self {
            agent,
            config: Arc::new(config),
            authz,
            db,
            thread_pool,
        }
    }

    pub(crate) fn agent(&self) -> &Agent {
        &self.agent
    }

    pub(crate) fn authz(&self) -> &Authz {
        &self.authz
    }

    pub(crate) fn config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn db(&self) -> &Db {
        &self.db
    }

    pub(crate) fn thread_pool(&self) -> Arc<ThreadPool> {
        self.thread_pool.clone()
    }
}
