use svc_agent::AccountId;
use svc_authz::{
    Authenticable, BanCallback, ClientMap, Config, ConfigMap, IntentObject, LocalWhitelistConfig,
    LocalWhitelistRecord,
};

use crate::test_helpers::USR_AUDIENCE;

///////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug)]
pub struct TestAuthz {
    records: Vec<LocalWhitelistRecord>,
    audience: String,
}

impl TestAuthz {
    pub fn new() -> Self {
        Self {
            records: vec![],
            audience: USR_AUDIENCE.to_owned(),
        }
    }

    pub fn set_audience(&mut self, audience: &str) -> &mut Self {
        self.audience = audience.to_owned();
        self
    }

    pub fn allow<A: Authenticable>(&mut self, subject: &A, object: Vec<&str>, action: &str) {
        let object: Box<dyn IntentObject> =
            crate::app::endpoint::authz::AuthzObject::new(&object).into();
        let record = LocalWhitelistRecord::new(subject, object, action);
        self.records.push(record);
    }
}

impl Into<ClientMap> for TestAuthz {
    fn into(self) -> ClientMap {
        let config = LocalWhitelistConfig::new(self.records);

        let mut config_map = ConfigMap::new();
        config_map.insert(self.audience.to_owned(), Config::LocalWhitelist(config));

        let account_id = AccountId::new("conference", &self.audience);

        ClientMap::new(&account_id, None, config_map, None).expect("Failed to build authz")
    }
}

pub struct DbBanTestAuthz {
    records: Vec<LocalWhitelistRecord>,
    f: BanCallback,
}

impl DbBanTestAuthz {
    pub fn new(f: BanCallback) -> Self {
        Self {
            records: vec![],
            f: f,
        }
    }

    pub fn allow<A: Authenticable>(&mut self, subject: &A, object: Vec<&str>, action: &str) {
        let object: Box<dyn IntentObject> =
            crate::app::endpoint::authz::AuthzObject::new(&object).into();
        let record = LocalWhitelistRecord::new(subject, object, action);
        self.records.push(record);
    }
}

impl Into<ClientMap> for DbBanTestAuthz {
    fn into(self) -> ClientMap {
        let config = LocalWhitelistConfig::new(self.records);

        let mut config_map = ConfigMap::new();
        config_map.insert(USR_AUDIENCE.to_owned(), Config::LocalWhitelist(config));

        let account_id = AccountId::new("conference", USR_AUDIENCE);

        ClientMap::new(&account_id, None, config_map, Some(self.f)).expect("Failed to build authz")
    }
}
