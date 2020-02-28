use svc_agent::AccountId;
use svc_authz::{
    Authenticable, ClientMap, Config, ConfigMap, LocalWhitelistConfig, LocalWhitelistRecord,
};

use crate::test_helpers::USR_AUDIENCE;

///////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug)]
pub(crate) struct TestAuthz {
    records: Vec<LocalWhitelistRecord>,
}

impl TestAuthz {
    pub(crate) fn new() -> Self {
        Self { records: vec![] }
    }

    pub(crate) fn allow<A: Authenticable>(&mut self, subject: &A, object: Vec<&str>, action: &str) {
        let record = LocalWhitelistRecord::new(subject, object, action);
        self.records.push(record);
    }
}

impl Into<ClientMap> for TestAuthz {
    fn into(self) -> ClientMap {
        let config = LocalWhitelistConfig::new(self.records);

        let mut config_map = ConfigMap::new();
        config_map.insert(USR_AUDIENCE.to_owned(), Config::LocalWhitelist(config));

        let account_id = AccountId::new("conference", USR_AUDIENCE);
        ClientMap::new(&account_id, None, config_map).expect("Failed to build authz")
    }
}
