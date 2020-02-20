use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{DateTime, Duration, Utc};
use failure::{format_err, Error};
use svc_agent::AccountId;

use crate::config::AuthzCacheConfig;

#[derive(Debug, Hash, PartialEq, Eq)]
struct Key {
    account_id: AccountId,
    object: Vec<String>,
    action: &'static str,
}

impl Key {
    fn new(account_id: &AccountId, object: &[&str], action: &'static str) -> Self {
        let owned_object = object.into_iter().map(|x| (*x).to_owned()).collect();

        Self {
            account_id: account_id.to_owned(),
            object: owned_object,
            action,
        }
    }
}

#[derive(Debug)]
pub(crate) struct AuthzCache {
    entries: RwLock<HashMap<Key, (bool, DateTime<Utc>)>>,
    lifetime: Duration,
}

impl AuthzCache {
    pub(crate) fn new(config: &AuthzCacheConfig) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            lifetime: Duration::seconds(config.lifetime),
        }
    }

    pub(crate) fn set(
        &self,
        account_id: &AccountId,
        object: &[&str],
        action: &'static str,
        value: bool,
    ) -> Result<(), Error> {
        let key = Key::new(account_id, object, action);

        let mut entries = self
            .entries
            .write()
            .map_err(|err| format_err!("Failed to obtain authz cache write lock: {}", err))?;

        entries.insert(key, (value, Utc::now()));
        Ok(())
    }

    pub(crate) fn get(
        &self,
        account_id: &AccountId,
        object: &[&str],
        action: &'static str,
    ) -> Result<Option<bool>, Error> {
        let key = Key::new(account_id, object, action);

        let entries = self
            .entries
            .read()
            .map_err(|err| format_err!("Failed to obtain authz cache read lock: {}", err))?;

        let maybe_value = match entries.get(&key) {
            Some((value, set_at)) if Utc::now() < *set_at + self.lifetime => Some(*value),
            _ => None,
        };

        Ok(maybe_value)
    }

    pub(crate) fn vacuum(&self) -> Result<(), Error> {
        let limit = Utc::now() - self.lifetime;

        let mut entries = self
            .entries
            .write()
            .map_err(|err| format_err!("Failed to obtain authz cache write lock: {}", err))?;

        entries.retain(|_, (_, set_at)| *set_at < limit);
        Ok(())
    }
}
