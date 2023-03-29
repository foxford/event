use std::{sync::Arc, time::Duration};

use anyhow::{Context as AnyhowContext, Result};
use futures::StreamExt;
use prometheus::Registry;
use signal_hook::consts::TERM_SIGNALS;
use sqlx::postgres::PgPool as Db;
use svc_agent::mqtt::{Agent, AgentBuilder, AgentNotification, ConnectionMode, QoS};
use svc_agent::{request::Dispatcher, AgentId, Authenticable, SharedGroup, Subscription};
use svc_authn::token::jws_compact;
use svc_authz::cache::{AuthzCache, ConnectionPool as RedisConnectionPool};
use svc_error::extension::sentry as svc_sentry;
use svc_nats_client::NatsClient;
use tokio::{sync::mpsc, task};
use tracing::{error, info, warn};

use crate::app::broker_client::{BrokerClient, HttpBrokerClient};
use crate::app::http::build_router;
use crate::{app::context::GlobalContext, metrics::Metrics};
use crate::{
    authz::Authz,
    config::{self, Config},
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
    info!("App config: {:?}", config);

    // Agent
    let agent_id = AgentId::new(&config.agent_label, config.id.clone());
    info!("Agent id: {:?}", &agent_id);

    let token = jws_compact::TokenBuilder::new()
        .issuer(agent_id.as_account_id().audience())
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

    let is_banned_f = endpoint::authz::db_ban_callback(db.clone());

    // Authz
    let authz = svc_authz::ClientMap::new(
        &config.id,
        authz_cache,
        config.authz.clone(),
        Some(is_banned_f),
    )
    .context("Error converting authz config to clients")?;

    // Sentry
    let _j = if let Some(ref sentry_config) = config.sentry {
        let mut sentry_config = sentry_config.clone();
        sentry_config.release = sentry::release_name!();
        Some(svc_error::extension::sentry::init(&sentry_config))
    } else {
        None
    };

    // Subscribe to topics
    subscribe(&mut agent, &agent_id)?;

    let registry = Registry::new();
    let metrics = Arc::new(Metrics::new(&registry)?);

    // Context
    let authz = Authz::new(authz, metrics.clone());
    let queue_counter = agent.get_queue_counter();
    let dispatcher = Arc::new(Dispatcher::new(&agent));
    let broker_client = build_broker_client(&config, &token);

    let context_builder = AppContextBuilder::new(config.clone(), authz, db, broker_client);

    let context_builder = match ro_db {
        Some(db) => context_builder.ro_db(db),
        None => context_builder,
    };

    let context_builder = match redis_pool {
        Some(pool) => context_builder.redis_pool(pool),
        None => context_builder,
    };

    let context = context_builder.queue_counter(queue_counter).build(metrics);

    let metrics_task = config.metrics.as_ref().map(|metrics| {
        svc_utils::metrics::MetricsServer::new_with_registry(registry, metrics.http.bind_address)
    });

    let metrics = context.metrics();

    let ctx = Arc::new(context.clone());
    let (graceful_tx, graceful_rx) = tokio::sync::watch::channel(());
    let mut shutdown_server_rx = graceful_rx.clone();
    let http_task = tokio::spawn(
        axum::Server::bind(&config.http_addr)
            .serve(
                build_router(ctx.clone(), agent.clone(), config.authn.clone()).into_make_service(),
            )
            .with_graceful_shutdown(async move {
                shutdown_server_rx.changed().await.ok();
            }),
    );

    let nats_puller = match config.nats.zip(config.nats_puller) {
        Some((nats_cfg, nats_puller_cfg)) => {
            let nats_client = build_nats_client(&nats_cfg).await;
            info!("Connected to nats");

            let nats_puller = nats_puller::run(
                ctx.clone(),
                nats_client,
                &nats_cfg,
                nats_puller_cfg,
                graceful_rx.clone(),
            )
            .await
            .context("nats puller")?;
            info!("Nats puller started");

            Some(nats_puller)
        }
        None => None,
    };

    // Message handler
    let message_handler = Arc::new(MessageHandler::new(agent.clone(), context, dispatcher));

    // Message loop
    let mut signals_stream = signal_hook_tokio::Signals::new(TERM_SIGNALS)?.fuse();
    let signals = signals_stream.next();

    let main_loop_task = task::spawn(main_loop(rx, message_handler.clone(), metrics.clone()));
    let _ = futures::future::select(signals, main_loop_task).await;
    unsubscribe(&mut agent, &agent_id)?;

    let _ = graceful_tx.send(());

    if let Some(puller) = nats_puller {
        if let Err(err) = puller.await {
            error!(%err, "failed to await nats puller completion");
        }
    }

    if let Some(metrics_task) = metrics_task {
        metrics_task.shutdown().await;
    }

    if let Err(e) = http_task.await {
        error!("Failed to await http server completion, err = {:?}", e);
    }

    tokio::time::sleep(Duration::from_secs(3)).await;
    info!(
        "Running requests left: {}",
        metrics.running_requests_total.get()
    );

    Ok(())
}

async fn main_loop(
    mut mq_rx: mpsc::UnboundedReceiver<AgentNotification>,
    message_handler: Arc<MessageHandler<context::AppContext>>,
    metrics: Arc<Metrics>,
) {
    loop {
        if let Some(message) = mq_rx.recv().await {
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
                        error!("Disconnected from broker")
                    }
                    AgentNotification::Reconnection => {
                        metrics.mqtt_reconnection.inc();
                        error!("Reconnected to broker");

                        resubscribe(
                            &mut message_handler.agent().to_owned(),
                            message_handler.global_context().agent_id(),
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
}

fn subscribe(agent: &mut Agent, agent_id: &AgentId) -> Result<()> {
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

    Ok(())
}

fn unsubscribe(agent: &mut Agent, agent_id: &AgentId) -> Result<()> {
    let group = SharedGroup::new("loadbalancer", agent_id.as_account_id().clone());

    // Multicast requests
    agent
        .unsubscribe(
            &Subscription::multicast_requests(Some(API_VERSION)),
            Some(&group),
        )
        .context("Error subscribing to multicast requests")?;

    // Unicast requests
    agent
        .unsubscribe(&Subscription::unicast_requests(), None)
        .context("Error subscribing to unicast requests")?;

    Ok(())
}

fn resubscribe(agent: &mut Agent, agent_id: &AgentId) {
    if let Err(err) = subscribe(agent, agent_id) {
        let err = err.context("Failed to resubscribe after reconnection");
        error!("{:?}", err);

        svc_sentry::send(Arc::new(err))
            .unwrap_or_else(|err| warn!("Error sending error to Sentry: {:?}", err));
    }
}

fn build_broker_client(config: &Config, token: &str) -> Arc<dyn BrokerClient> {
    Arc::new(
        HttpBrokerClient::new(
            &config.http_broker_client.host,
            token,
            config.http_broker_client.timeout,
        )
        .expect("Failed to create Http Broker Client"),
    )
}

async fn build_nats_client(config: &svc_nats_client::Config) -> Arc<dyn NatsClient> {
    Arc::new(
        svc_nats_client::new(&config.url, &config.creds)
            .await
            .expect("failed to create nats client"),
    )
}

pub(crate) mod broker_client;
pub(crate) mod context;
pub(crate) mod endpoint;
pub(crate) mod error;
pub(crate) mod http;
pub(crate) mod message_handler;
pub(crate) mod nats_puller;
pub(crate) mod operations;
pub(crate) mod s3_client;
pub(crate) mod service_utils;
