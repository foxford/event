use std::future::Future;
use std::sync::Arc;

use svc_agent::mqtt::IntoPublishableDump;
use svc_authz::ClientMap as Authz;

use crate::app::task_executor::{AppTaskExecutor, TaskExecutor};
use crate::config::Config;
use crate::db::ConnectionPool as Db;

#[derive(Clone)]
pub(crate) struct AppContext {
    config: Arc<Config>,
    authz: Authz,
    db: Db,
    task_executor: Arc<AppTaskExecutor>,
}

impl AppContext {
    pub(crate) fn new(
        config: Config,
        authz: Authz,
        db: Db,
        task_executor: AppTaskExecutor,
    ) -> Self {
        Self {
            config: Arc::new(config),
            authz,
            db,
            task_executor: Arc::new(task_executor),
        }
    }
}

pub(crate) trait Context: Sync {
    fn authz(&self) -> &Authz;
    fn config(&self) -> &Config;
    fn db(&self) -> &Db;

    fn run_task(
        &self,
        task: impl Future<Output = Vec<Box<dyn IntoPublishableDump>>> + Send + 'static,
    );
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

    fn run_task(
        &self,
        task: impl Future<Output = Vec<Box<dyn IntoPublishableDump>>> + Send + 'static,
    ) {
        self.task_executor.run(task);
    }
}
