use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::{
    net::SocketAddr,
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Context as AnyhowContext, Result};
use async_std::task;
use chrono::Utc;
use futures::StreamExt;
use prometheus::{Encoder, Registry, TextEncoder};
use serde_json::json;
use sqlx::postgres::PgPool as Db;
use svc_agent::mqtt::{
    Agent, AgentBuilder, AgentNotification, ConnectionMode, OutgoingRequest,
    OutgoingRequestProperties, QoS, ShortTermTimingProperties, SubscriptionTopic,
};
use svc_agent::{AccountId, AgentId, Authenticable, SharedGroup, Subscription};
use svc_authn::token::jws_compact;
use svc_authz::cache::{AuthzCache, ConnectionPool as RedisConnectionPool};
use svc_error::{extension::sentry, Error as SvcError};

use crate::{app::context::GlobalContext, metrics::Metrics};
use crate::{
    authz::Authz,
    config::{self, Config, KruonisConfig},
};
use context::AppContextBuilder;
use message_handler::MessageHandler;

pub(crate) const API_VERSION: &str = "v1";

////////////////////////////////////////////////////////////////////////////////

pub(crate) async fn run(
    db: Db,
    ro_db: Option<Db>,
    redis_pool: Option<RedisConnectionPool>,
    authz_cache: Option<Box<dyn AuthzCache>>,
) -> Result<()> {
    // Config
    let config = config::load().context("Failed to load config")?;
    info!(crate::LOG, "App config: {:?}", config);

    // Agent
    let agent_id = AgentId::new(&config.agent_label, config.id.clone());
    info!(crate::LOG, "Agent id: {:?}", &agent_id);

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
    let (mq_tx, mut mq_rx) = futures_channel::mpsc::unbounded::<AgentNotification>();

    thread::Builder::new()
        .name("event-notifications-loop".to_owned())
        .spawn(move || {
            for message in rx {
                if mq_tx.unbounded_send(message).is_err() {
                    error!(crate::LOG, "Error sending message to the internal channel");
                }
            }
        })
        .expect("Failed to start event notifications loop");

    let is_banned_f = crate::app::endpoint::authz::db_ban_callback(db.clone());

    // Authz
    let authz = svc_authz::ClientMap::new(
        &config.id,
        authz_cache,
        config.authz.clone(),
        Some(is_banned_f),
    )
    .context("Error converting authz config to clients")?;

    // Sentry
    if let Some(sentry_config) = config.sentry.as_ref() {
        svc_error::extension::sentry::init(sentry_config);
    }

    // Subscribe to topics
    subscribe(&mut agent, &agent_id, &config)?;

    let registry = Registry::new();
    let metrics = Arc::new(Metrics::new(&registry)?);

    // Context
    let authz = Authz::new(authz, metrics.clone());
    let context_builder = AppContextBuilder::new(config.clone(), authz, db);

    let context_builder = match ro_db {
        Some(db) => context_builder.ro_db(db),
        None => context_builder,
    };

    let context_builder = match redis_pool {
        Some(pool) => context_builder.redis_pool(pool),
        None => context_builder,
    };

    let context = context_builder
        .queue_counter(agent.get_queue_counter())
        .build(metrics);
    if let Some(metrics) = config.metrics.as_ref() {
        task::spawn(start_metrics_collector(registry, metrics.http.bind_address));
    }

    let metrics = context.metrics();

    // Message handler
    let message_handler = Arc::new(MessageHandler::new(agent, context));

    // Message loop
    let term_check_period = Duration::from_secs(1);
    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;

    while !term.load(Ordering::Relaxed) {
        let fut = async_std::future::timeout(term_check_period, mq_rx.next());

        if let Ok(Some(message)) = fut.await {
            let message_handler = message_handler.clone();
            let request_started = metrics.clone().request_started();
            let metrics = metrics.clone();
            task::spawn(async move {
                match message {
                    AgentNotification::Message(ref message, _) => {
                        metrics.total_requests.inc();
                        message_handler.handle(message).await;
                    }
                    AgentNotification::Disconnect => {
                        metrics.mqtt_disconnect.inc();
                        error!(crate::LOG, "Disconnected from broker")
                    }
                    AgentNotification::Reconnection => {
                        metrics.mqtt_reconnection.inc();
                        error!(crate::LOG, "Reconnected to broker");

                        resubscribe(
                            &mut message_handler.agent().to_owned(),
                            message_handler.global_context().agent_id(),
                            message_handler.global_context().config(),
                        );
                    }
                    AgentNotification::Puback(_) => (),
                    AgentNotification::Pubrec(_) => (),
                    AgentNotification::Pubcomp(_) => (),
                    AgentNotification::Suback(_) => (),
                    AgentNotification::Unsuback(_) => (),
                    AgentNotification::ConnectionError => metrics.mqtt_connection_error.inc(),
                    AgentNotification::Connect(_) => (),
                    AgentNotification::Connack(_) => (),
                    AgentNotification::Pubrel(_) => (),
                    AgentNotification::Subscribe(_) => (),
                    AgentNotification::Unsubscribe(_) => (),
                    AgentNotification::PingReq => (),
                    AgentNotification::PingResp => (),
                }
                drop(request_started);
            });
        }
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
        .subscribe(&Subscription::unicast_requests(), QoS::AtMostOnce, None)
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
    let event = OutgoingRequest::multicast(json!({}), props, kruonis_id, API_VERSION);

    agent.publish(event).context("Failed to publish message")?;
    Ok(())
}

fn resubscribe(agent: &mut Agent, agent_id: &AgentId, config: &Config) {
    if let Err(err) = subscribe(agent, agent_id, config) {
        let err = format!("Failed to resubscribe after reconnection: {:?}", err);
        error!(crate::LOG, "{:?}", err);

        let svc_error = SvcError::builder()
            .kind("resubscription_error", "Resubscription error")
            .detail(&err)
            .build();

        sentry::send(svc_error)
            .unwrap_or_else(|err| warn!(crate::LOG, "Error sending error to Sentry: {:?}", err));
    }
}

async fn start_metrics_collector(
    registry: Registry,
    bind_addr: SocketAddr,
) -> async_std::io::Result<()> {
    let mut app = tide::with_state(registry);
    app.at("/metrics")
        .get(|req: tide::Request<Registry>| async move {
            let registry = req.state();
            let mut buffer = vec![];
            let encoder = TextEncoder::new();
            let metric_families = registry.gather();
            match encoder.encode(&metric_families, &mut buffer) {
                Ok(_) => {
                    let mut response = tide::Response::new(200);
                    response.set_body(buffer);
                    Ok(response)
                }
                Err(err) => {
                    warn!(crate::LOG, "Metrics not gathered: {:?}", err);
                    Ok(tide::Response::new(500))
                }
            }
        });
    app.listen(bind_addr).await
}

pub(crate) mod context;
pub(crate) mod endpoint;
pub(crate) mod error;
pub(crate) mod message_handler;
pub(crate) mod operations;
pub(crate) mod s3_client;
