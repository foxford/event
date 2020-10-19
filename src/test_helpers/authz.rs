use svc_agent::AccountId;
use svc_authz::{
    Authenticable, BanCallback, ClientMap, Config, ConfigMap, IntentObject, LocalWhitelistConfig,
    LocalWhitelistRecord,
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
        config_map.insert(USR_AUDIENCE.to_owned(), Config::LocalWhitelist(config));

        let account_id = AccountId::new("conference", USR_AUDIENCE);
        let is_banned_f = std::sync::Arc::new(
            move |_account_id: AccountId, _intent: Box<dyn IntentObject>| {
                Box::pin(async move { false })
                    as std::pin::Pin<Box<dyn futures::Future<Output = bool> + Send>>
            },
        ) as BanCallback;

        ClientMap::new(&account_id, None, config_map, is_banned_f).expect("Failed to build authz")
    }
}

pub(crate) struct DbBanTestAuthz {
    records: Vec<LocalWhitelistRecord>,
    f: BanCallback,
}

impl DbBanTestAuthz {
    pub(crate) fn new(f: BanCallback) -> Self {
        Self {
            records: vec![],
            f: f,
        }
    }

    pub(crate) fn allow<A: Authenticable>(&mut self, subject: &A, object: Vec<&str>, action: &str) {
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

        ClientMap::new(&account_id, None, config_map, self.f).expect("Failed to build authz")
    }
}
