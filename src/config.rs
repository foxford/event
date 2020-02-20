use config;
use serde_derive::Deserialize;
use svc_agent::{mqtt::AgentConfig, AccountId};
use svc_authn::jose::{Algorithm, ConfigMap as Authn};
use svc_authz::ConfigMap as Authz;
use svc_error::extension::sentry::Config as SentryConfig;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Config {
    pub(crate) id: AccountId,
    pub(crate) id_token: JwtConfig,
    pub(crate) agent_label: String,
    pub(crate) broker_id: AccountId,
    pub(crate) authn: Authn,
    pub(crate) authz: Authz,
    pub(crate) authz_cache: AuthzCacheConfig,
    pub(crate) mqtt: AgentConfig,
    pub(crate) sentry: Option<SentryConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JwtConfig {
    #[serde(deserialize_with = "svc_authn::serde::algorithm")]
    pub(crate) algorithm: Algorithm,
    #[serde(deserialize_with = "svc_authn::serde::file")]
    pub(crate) key: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AuthzCacheConfig {
    pub(crate) lifetime: i64,
    pub(crate) vacuum_period: u64,
}

pub(crate) fn load() -> Result<Config, config::ConfigError> {
    let mut parser = config::Config::default();
    parser.merge(config::File::with_name("App"))?;
    parser.merge(config::Environment::with_prefix("APP").separator("__"))?;
    parser.try_into::<Config>()
}
