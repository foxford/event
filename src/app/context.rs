use std::sync::Arc;

use svc_agent::mqtt::Address;
use svc_authz::ClientMap as Authz;

use crate::backend::Client as BackendClient;
use crate::config::Config;
use crate::db::ConnectionPool as Db;

#[derive(Clone)]
pub(crate) struct Context {
    address: Address,
    config: Arc<Config>,
    authz: Authz,
    db: Db,
    backend: Arc<BackendClient>,
}

impl Context {
    pub(crate) fn new(address: Address, config: Config, authz: Authz, db: Db) -> Self {
        let backend = BackendClient::new(config.backend.to_owned(), address.id().to_owned());

        Self {
            address,
            config: Arc::new(config),
            authz,
            db,
            backend: Arc::new(backend),
        }
    }

    pub(crate) fn address(&self) -> &Address {
        &self.address
    }

    pub(crate) fn config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn authz(&self) -> &Authz {
        &self.authz
    }

    pub(crate) fn db(&self) -> &Db {
        &self.db
    }

    pub(crate) fn backend(&self) -> &BackendClient {
        &self.backend
    }
}
