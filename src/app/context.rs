use std::sync::Arc;

use svc_authz::ClientMap as Authz;

use crate::app::task_executor::TaskExecutor;
use crate::authz_cache::AuthzCache;
use crate::config::Config;
use crate::db::ConnectionPool as Db;

#[derive(Clone)]
pub(crate) struct AppContext {
    config: Arc<Config>,
    authz: Authz,
    authz_cache: Arc<AuthzCache>,
    db: Db,
    task_executor: Arc<TaskExecutor>,
}

#[allow(dead_code)]
impl AppContext {
    pub(crate) fn new(
        config: Config,
        authz: Authz,
        authz_cache: Arc<AuthzCache>,
        db: Db,
        task_executor: TaskExecutor,
    ) -> Self {
        Self {
            config: Arc::new(config),
            authz,
            authz_cache,
            db,
            task_executor: Arc::new(task_executor),
        }
    }
}

pub(crate) trait Context: Sync {
    fn authz(&self) -> &Authz;
    fn authz_cache(&self) -> &AuthzCache;
    fn config(&self) -> &Config;
    fn db(&self) -> &Db;
    fn task_executor(&self) -> &TaskExecutor;
}

impl Context for AppContext {
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
        &self.db
    }

    fn task_executor(&self) -> &TaskExecutor {
        &self.task_executor
    }
}
