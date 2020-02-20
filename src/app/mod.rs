use std::sync::Arc;
use std::thread;
use std::time::Duration as StdDuration;

use failure::{format_err, Error};
use futures::{executor::ThreadPoolBuilder, task::SpawnExt, StreamExt};
use futures_timer::Delay;
use log::{error, info};
use svc_agent::mqtt::{AgentBuilder, ConnectionMode, Notification, QoS};
use svc_agent::{AgentId, Authenticable, SharedGroup, Subscription};
use svc_authn::token::jws_compact;

use crate::authz_cache::AuthzCache;
use crate::config;
use crate::db::ConnectionPool;
use context::Context;
use message_handler::MessageHandler;

pub(crate) const API_VERSION: &str = "v1";

////////////////////////////////////////////////////////////////////////////////

pub(crate) async fn run(db: &ConnectionPool) -> Result<(), Error> {
    // Config
    let config = config::load().map_err(|err| format_err!("Failed to load config: {}", err))?;
    info!("App config: {:?}", config);

    // Agent
    let agent_id = AgentId::new(&config.agent_label, config.id.clone());
    info!("Agent id: {:?}", &agent_id);

    let token = jws_compact::TokenBuilder::new()
        .issuer(&agent_id.as_account_id().audience().to_string())
        .subject(&agent_id)
        .key(config.id_token.algorithm, config.id_token.key.as_slice())
        .build()
        .map_err(|err| format_err!("Error creating an id token: {}", err))?;

    let mut agent_config = config.mqtt.clone();
    agent_config.set_password(&token);

    let (mut agent, rx) = AgentBuilder::new(agent_id.clone(), API_VERSION)
        .connection_mode(ConnectionMode::Service)
        .start(&agent_config)
        .map_err(|err| format_err!("Failed to create an agent: {}", err))?;

    // Message loop for incoming messages of MQTT Agent
    let (mq_tx, mut mq_rx) = futures_channel::mpsc::unbounded::<Notification>();
    let thread_pool = Arc::new(ThreadPoolBuilder::new().create()?);

    thread::spawn(move || {
        for message in rx {
            if let Err(_) = mq_tx.unbounded_send(message) {
                error!("Error sending message to the internal channel");
            }
        }
    });

    // Authz
    let authz = svc_authz::ClientMap::new(&config.id, None, config.authz.clone())
        .map_err(|err| format_err!("Error converting authz config to clients: {}", err))?;

    // Authz cache
    let authz_cache = Arc::new(AuthzCache::new(&config.authz_cache));
    let authz_cache_clone = authz_cache.clone();
    let authz_cache_vacuum_period = StdDuration::from_secs(config.authz_cache.vacuum_period);

    thread_pool
        .spawn(async move {
            loop {
                Delay::new(authz_cache_vacuum_period).await;

                if let Err(err) = authz_cache_clone.vacuum() {
                    error!("Authz cache vacuum failed: {}", err);
                }
            }
        })
        .map_err(|err| format_err!("Failed to schedule authz cache vacuum job: {}", err))?;

    // Sentry
    if let Some(sentry_config) = config.sentry.as_ref() {
        svc_error::extension::sentry::init(sentry_config);
    }

    // Subscribe to requests
    agent
        .subscribe(
            &Subscription::multicast_requests(Some(API_VERSION)),
            QoS::AtMostOnce,
            Some(&SharedGroup::new(
                "loadbalancer",
                agent_id.as_account_id().clone(),
            )),
        )
        .map_err(|err| format_err!("Error subscribing to multicast requests: {}", err))?;

    // Message handler
    let context = Context::new(
        agent.to_owned(),
        config,
        authz,
        authz_cache,
        db.clone(),
        thread_pool.clone(),
    );

    let message_handler = Arc::new(MessageHandler::new(agent, context));

    // Message loop
    while let Some(message) = mq_rx.next().await {
        let message_handler = message_handler.clone();

        thread_pool
            .spawn(async move {
                match message {
                    svc_agent::mqtt::Notification::Publish(message) => {
                        message_handler.handle(&message.payload).await
                    }
                    _ => error!("Unsupported notification type = '{:?}'", message),
                }
            })
            .map_err(|err| format_err!("Failed to spawn message handling task: {}", err))?;
    }

    Ok(())
}

mod context;
mod endpoint;
mod message_handler;
pub(crate) mod operations;
