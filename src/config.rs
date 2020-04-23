use config;
use serde_derive::Deserialize;
use svc_agent::{mqtt::AgentConfig, AccountId};
use svc_authn::jose::Algorithm;
use svc_authz::ConfigMap as Authz;
use svc_error::extension::sentry::Config as SentryConfig;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Config {
    pub(crate) id: AccountId,
    pub(crate) id_token: JwtConfig,
    pub(crate) agent_label: String,
    pub(crate) broker_id: AccountId,
    pub(crate) authz: Authz,
    pub(crate) mqtt: AgentConfig,
    pub(crate) sentry: Option<SentryConfig>,
    pub(crate) telemetry: Option<TelemetryConfig>,
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

#[derive(Clone, Debug)]
pub(crate) enum TelemetryConfig {
    Enabled(AccountId),
    Disabled,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self::Disabled
    }
}

use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
impl<'de> Deserialize<'de> for TelemetryConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Enabled,
            TelemetryId,
        }

        struct TelemetryConfigVisitor;

        impl<'de> Visitor<'de> for TelemetryConfigVisitor {
            type Value = TelemetryConfig;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct TelemetryConfig")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut enabled = None;
                let mut telemetry_id = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Enabled => {
                            if enabled.is_some() {
                                return Err(de::Error::duplicate_field("enabled"));
                            }
                            enabled = Some(map.next_value()?);
                        }
                        Field::TelemetryId => {
                            if telemetry_id.is_some() {
                                return Err(de::Error::duplicate_field("telemetry_id"));
                            }
                            telemetry_id = Some(map.next_value()?);
                        }
                    }
                }
                if let Some(enabled) = enabled {
                    if enabled {
                        let telemetry_id =
                            telemetry_id.ok_or_else(|| de::Error::missing_field("telemetry_id"))?;
                        Ok(TelemetryConfig::Enabled(telemetry_id))
                    } else {
                        Ok(TelemetryConfig::Disabled)
                    }
                } else {
                    Ok(TelemetryConfig::Disabled)
                }
            }
        }

        const FIELDS: &'static [&'static str] = &["enabled", "telemetry_id"];
        deserializer.deserialize_struct("TelemetryConfig", FIELDS, TelemetryConfigVisitor)
    }
}
