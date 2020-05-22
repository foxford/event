use std::sync::Arc;
use std::thread;

use anyhow::{Context as AnyhowContext, Result};
use async_std::task;
use chrono::Utc;
use futures::StreamExt;
use log::{error, info, warn};
use serde_json::json;
use svc_agent::mqtt::{
    Agent, AgentBuilder, ConnectionMode, IntoPublishableDump, Notification, OutgoingRequest,
    OutgoingRequestProperties, QoS, ShortTermTimingProperties, SubscriptionTopic,
};
use svc_agent::{AccountId, AgentId, Authenticable, SharedGroup, Subscription};
use svc_authn::token::jws_compact;
use svc_authz::cache::Cache as AuthzCache;
use svc_error::{extension::sentry, Error as SvcError};

use crate::app::context::Context;
use crate::config::{self, Config, KruonisConfig};
use crate::db::ConnectionPool;
use context::AppContext;
use message_handler::MessageHandler;

pub(crate) const API_VERSION: &str = "v1";

////////////////////////////////////////////////////////////////////////////////

pub(crate) async fn run(db: &ConnectionPool, authz_cache: Option<AuthzCache>) -> Result<()> {
    // Config
    let config = config::load().context("Failed to load config")?;
    info!("App config: {:?}", config);

    // Agent
    let agent_id = AgentId::new(&config.agent_label, config.id.clone());
    info!("Agent id: {:?}", &agent_id);

    let token = jws_compact::TokenBuilder::new()
        .issuer(&agent_id.as_account_id().audience().to_string())
        .subject(&agent_id)
        .key(config.id_token.algorithm, config.id_token.key.as_slice())
        .build()
        .context("Error creating an id token")?;

    let mut agent_config = config.mqtt.clone();
    agent_config.set_password(&token);

    let (mut agent, rx) = AgentBuilder::new(agent_id.clone(), API_VERSION)
        .connection_mode(ConnectionMode::Service)
        .start(&agent_config)
        .context("Failed to create an agent")?;

    // Message loop for incoming messages of MQTT Agent
    let (mq_tx, mut mq_rx) = futures_channel::mpsc::unbounded::<Notification>();

    thread::spawn(move || {
        for message in rx {
            if mq_tx.unbounded_send(message).is_err() {
                error!("Error sending message to the internal channel");
            }
        }
    });

    // Authz
    let authz = svc_authz::ClientMap::new(&config.id, authz_cache, config.authz.clone())
        .context("Error converting authz config to clients")?;

    // Sentry
    if let Some(sentry_config) = config.sentry.as_ref() {
        svc_error::extension::sentry::init(sentry_config);
    }

    // Subscribe to topics
    subscribe(&mut agent, &agent_id, &config)?;

    // Context
    let context = AppContext::new(config, authz, db.clone());

    // Message handler
    let message_handler = Arc::new(MessageHandler::new(agent, context));

    // Message loop
    while let Some(message) = mq_rx.next().await {
        let message_handler = message_handler.clone();

        task::spawn(async move {
            match message {
                svc_agent::mqtt::Notification::Publish(message) => {
                    message_handler.handle(&message.payload).await
                }
                svc_agent::mqtt::Notification::Disconnection => {
                    error!("Disconnected from broker");
                }
                svc_agent::mqtt::Notification::Reconnection => {
                    error!("Reconnected to broker");

                    resubscribe(
                        &mut message_handler.agent().to_owned(),
                        message_handler.context().agent_id(),
                        message_handler.context().config(),
                    );
                }
                _ => error!("Unsupported notification type = '{:?}'", message),
            }
        });
    }

    Ok(())
}

fn subscribe(agent: &mut Agent, agent_id: &AgentId, config: &Config) -> Result<()> {
    let group = SharedGroup::new("loadbalancer", agent_id.as_account_id().clone());

    // Multicast requests
    agent
        .subscribe(
            &Subscription::multicast_requests(Some(API_VERSION)),
            QoS::AtMostOnce,
            Some(&group),
        )
        .context("Error subscribing to multicast requests")?;

    // Unicast requests
    agent
        .subscribe(
            &Subscription::unicast_requests(),
            QoS::AtMostOnce,
            Some(&group),
        )
        .context("Error subscribing to unicast requests")?;

    // Kruonis
    if let KruonisConfig {
        id: Some(ref kruonis_id),
    } = config.kruonis
    {
        subscribe_to_kruonis(kruonis_id, agent)?;
    }

    Ok(())
}

fn subscribe_to_kruonis(kruonis_id: &AccountId, agent: &mut Agent) -> Result<()> {
    let timing = ShortTermTimingProperties::new(Utc::now());

    let topic = Subscription::unicast_requests_from(kruonis_id)
        .subscription_topic(agent.id(), API_VERSION)
        .context("Failed to build subscription topic")?;

    let props = OutgoingRequestProperties::new("kruonis.subscribe", &topic, "", timing);
    let event = OutgoingRequest::multicast(json!({}), props, kruonis_id);
    let message = Box::new(event) as Box<dyn IntoPublishableDump + Send>;

    let dump = message
        .into_dump(agent.address())
        .context("Failed to dump message")?;

    info!(
        "Outgoing message = '{}' sending to the topic = '{}'",
        dump.payload(),
        dump.topic(),
    );

    agent
        .publish_dump(dump)
        .context("Failed to publish message")?;

    Ok(())
}

fn resubscribe(agent: &mut Agent, agent_id: &AgentId, config: &Config) {
    if let Err(err) = subscribe(agent, agent_id, config) {
        let err = format!("Failed to resubscribe after reconnection: {}", err);
        error!("{}", err);

        let svc_error = SvcError::builder()
            .kind("resubscription_error", "Resubscription error")
            .detail(&err)
            .build();

        sentry::send(svc_error)
            .unwrap_or_else(|err| warn!("Error sending error to Sentry: {}", err));
    }
}

pub(crate) mod context;
pub(crate) mod endpoint;
pub(crate) mod message_handler;
pub(crate) mod operations;
