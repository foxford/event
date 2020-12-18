use chrono::Duration;
use serde_derive::Deserialize;
use svc_agent::{mqtt::AgentConfig, AccountId};
use svc_authn::jose::Algorithm;
use svc_authz::ConfigMap as Authz;
use svc_error::extension::sentry::Config as SentryConfig;

const DEFAULT_BAN_DUR_SECS: u64 = 5 * 3600;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Config {
    pub(crate) id: AccountId,
    pub(crate) id_token: JwtConfig,
    pub(crate) agent_label: String,
    pub(crate) broker_id: AccountId,
    pub(crate) authz: Authz,
    pub(crate) mqtt: AgentConfig,
    pub(crate) sentry: Option<SentryConfig>,
    #[serde(default)]
    pub(crate) telemetry: TelemetryConfig,
    #[serde(default)]
    pub(crate) kruonis: KruonisConfig,
    pub(crate) metrics: Option<MetricsConfig>,
    ban_duration_s: Option<u64>,
    #[serde(default)]
    pub(crate) vacuum: VacuumConfig,
}

impl Config {
    pub fn ban_duration(&self) -> u64 {
        self.ban_duration_s.unwrap_or(DEFAULT_BAN_DUR_SECS)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct MetricsConfig {
    pub http: MetricsHttpConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MetricsHttpConfig {
    pub bind_address: std::net::SocketAddr,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JwtConfig {
    #[serde(deserialize_with = "svc_authn::serde::algorithm")]
    pub(crate) algorithm: Algorithm,
    #[serde(deserialize_with = "svc_authn::serde::file")]
    pub(crate) key: Vec<u8>,
}

pub(crate) fn load() -> Result<Config, config::ConfigError> {
    let mut parser = config::Config::default();
    parser.merge(config::File::with_name("App"))?;
    parser.merge(config::Environment::with_prefix("APP").separator("__"))?;
    parser.try_into::<Config>()
}

#[derive(Clone, Debug, Deserialize, Default)]
pub(crate) struct TelemetryConfig {
    pub(crate) id: Option<AccountId>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub(crate) struct KruonisConfig {
    pub(crate) id: Option<AccountId>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct VacuumConfig {
    pub(crate) max_history_size: usize,
    #[serde(with = "crate::serde::duration_seconds")]
    pub(crate) max_history_lifetime: Duration,
}

impl Default for VacuumConfig {
    fn default() -> Self {
        Self {
            max_history_size: 10,
            max_history_lifetime: Duration::days(1),
        }
    }
}
