use std::net::SocketAddr;
use std::time::Duration as StdDuration;

use chrono::Duration;
use serde_derive::Deserialize;
use svc_agent::{mqtt::AgentConfig, AccountId};
use svc_authn::jose::{Algorithm, ConfigMap};
use svc_authz::ConfigMap as Authz;
use svc_error::extension::sentry::Config as SentryConfig;

const DEFAULT_BAN_DUR_SECS: u64 = 5 * 3600;

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub id: AccountId,
    pub id_token: JwtConfig,
    pub agent_label: String,
    pub broker_id: AccountId,
    pub authn: ConfigMap,
    pub authz: Authz,
    pub http_addr: SocketAddr,
    pub mqtt: AgentConfig,
    pub sentry: Option<SentryConfig>,
    pub metrics: Option<MetricsConfig>,
    ban_duration_s: Option<u64>,
    #[serde(default)]
    pub vacuum: VacuumConfig,
    pub http_broker_client: HttpBrokerClientConfig,
    pub constraint: Constraint,
    pub adjust: AdjustConfig,
    pub nats: Option<svc_nats_client::Config>,
    pub nats_consumer: Option<NatsConsumer>,
}

impl Config {
    pub fn ban_duration(&self) -> u64 {
        self.ban_duration_s.unwrap_or(DEFAULT_BAN_DUR_SECS)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Constraint {
    pub payload_size: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MetricsConfig {
    pub http: MetricsHttpConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MetricsHttpConfig {
    pub bind_address: SocketAddr,
}

#[derive(Clone, Debug, Deserialize)]
pub struct JwtConfig {
    #[serde(deserialize_with = "svc_authn::serde::algorithm")]
    pub algorithm: Algorithm,
    #[serde(deserialize_with = "svc_authn::serde::file")]
    pub key: Vec<u8>,
}

pub(crate) fn load() -> Result<Config, config::ConfigError> {
    let mut parser = config::Config::default();
    parser.merge(config::File::with_name("App"))?;
    parser.merge(config::Environment::with_prefix("APP").separator("__"))?;
    parser.try_into::<Config>()
}

#[derive(Clone, Debug, Deserialize)]
pub struct VacuumConfig {
    pub max_history_size: usize,
    #[serde(with = "crate::serde::duration_seconds")]
    pub max_history_lifetime: Duration,
    #[serde(with = "crate::serde::duration_seconds")]
    pub max_deleted_lifetime: Duration,
}

impl Default for VacuumConfig {
    fn default() -> Self {
        Self {
            max_history_size: 10,
            max_history_lifetime: Duration::days(1),
            max_deleted_lifetime: Duration::days(1),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct HttpBrokerClientConfig {
    pub host: String,
    #[serde(default, with = "humantime_serde")]
    pub timeout: Option<StdDuration>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AdjustConfig {
    #[serde(with = "humantime_serde")]
    pub min_segment_length: StdDuration,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NatsConsumer {
    #[serde(with = "humantime_serde")]
    pub suspend_interval: StdDuration,
    #[serde(with = "humantime_serde")]
    pub max_suspend_interval: StdDuration,
    #[serde(with = "humantime_serde")]
    pub suspend_sentry_interval: StdDuration,
    #[serde(with = "humantime_serde")]
    pub resubscribe_interval: StdDuration,
}
