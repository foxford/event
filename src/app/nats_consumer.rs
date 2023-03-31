use crate::{
    app::{
        context::GlobalContext,
        error::{Error as AppError, ErrorKind},
    },
    config, db,
};
use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use futures_util::StreamExt;
use serde_json::json;
use sqlx::{pool::PoolConnection, Postgres};
use std::{str::FromStr, sync::Arc, time::Duration};
use svc_conference_events::{Event, EventV1};
use svc_nats_client::{
    AckKind as NatsAckKind, Message, MessageStream, NatsClient, Subject, SubscribeError,
};
use tokio::{sync::watch, task::JoinHandle};
use tracing::{error, info, warn};

pub async fn run(
    ctx: Arc<dyn GlobalContext + Send>,
    nats_client: Arc<dyn NatsClient>,
    nats_consumer_config: config::NatsConsumer,
    mut shutdown_rx: watch::Receiver<()>,
) -> Result<JoinHandle<Result<(), SubscribeError>>> {
    let handle = tokio::spawn(async move {
        // In case of subscription errors we don't want to spam sentry
        let mut may_send_to_sentry = true;
        let mut not_spam_sentry_interval =
            tokio::time::interval(nats_consumer_config.suspend_sentry_interval);

        loop {
            let result = nats_client.subscribe().await;
            let messages = match result {
                Ok(messages) => messages,
                Err(err) => {
                    if may_send_to_sentry {
                        log_error_and_send_to_sentry(
                            anyhow!(err),
                            "failed to subscribe to the nats stream",
                            ErrorKind::NatsSubscriptionFailed,
                        );
                    }
                    may_send_to_sentry = false;

                    tokio::time::sleep(nats_consumer_config.resubscribe_interval).await;
                    continue;
                }
            };

            let handle_stream_future = handle_stream(
                ctx.as_ref(),
                nats_client.as_ref(),
                &nats_consumer_config,
                messages,
            );

            tokio::select! {
                _ = handle_stream_future => {
                    // Stream was closed. Send an error to sentry and try to resubscribe.
                    if may_send_to_sentry {
                        log_error_and_send_to_sentry(
                            anyhow!("nats stream was closed"),
                            "failed to handle the nats stream",
                            ErrorKind::NatsSubscriptionFailed,
                        );
                    }
                    may_send_to_sentry = false;

                    tokio::time::sleep(nats_consumer_config.resubscribe_interval).await;
                    continue;
                }
                _ = not_spam_sentry_interval.tick() => {
                    // Allow sending errors to sentry in case of subscription errors
                    may_send_to_sentry = true;
                }
                // Graceful shutdown
                _ = shutdown_rx.changed() => {
                    break;
                }
            }
        }

        Ok::<_, SubscribeError>(())
    });

    Ok(handle)
}

async fn handle_stream(
    ctx: &dyn GlobalContext,
    nats_client: &dyn NatsClient,
    nats_consumer_config: &config::NatsConsumer,
    mut messages: MessageStream,
) {
    let mut retry_count = 0;
    let mut suspend_interval: Option<Duration> = None;

    loop {
        if let Some(interval) = suspend_interval.take() {
            warn!(
                "nats consumer suspenses the processing of nats messages on {} seconds",
                interval.as_secs()
            );
            tokio::time::sleep(interval).await;
        }

        let message = match messages.next().await {
            Some(Ok(msg)) => msg,
            Some(Err(err)) => {
                log_error_and_send_to_sentry(
                    anyhow!(err),
                    "failed to get a message from nats",
                    ErrorKind::NatsGettingMessageFailed,
                );

                continue;
            }
            None => {
                // Stream was closed. Send an error to sentry and try to resubscribe.
                return;
            }
        };

        info!(
            "got a message from nats, subject: {:?}, payload: {:?}, headers: {:?}",
            message.subject, message.payload, message.headers
        );

        let mut conn = match ctx.get_conn().await {
            Ok(conn) => conn,
            Err(err) => {
                error!(?err, "failed to get DB connection");
                err.notify_sentry();

                if let Err(err) = message.ack_with(NatsAckKind::Nak(None)).await {
                    log_error_and_send_to_sentry(
                        anyhow!(err),
                        "failed to nack nats message",
                        ErrorKind::NatsNackFailed,
                    );
                }

                retry_count += 1;
                let interval = next_suspend_interval(retry_count, nats_consumer_config);
                suspend_interval = Some(interval);

                continue;
            }
        };

        if let Err(err) = handle_message(&mut conn, &message).await {
            log_error_and_send_to_sentry(
                err,
                "failed to handle nats message",
                ErrorKind::NatsMessageHandlingFailed,
            );

            if let Err(err) = nats_client.term_message(message).await {
                log_error_and_send_to_sentry(
                    anyhow!(err),
                    "failed to term nats message",
                    ErrorKind::NatsTermFailed,
                );
            }

            continue;
        }

        if let Err(err) = message.ack().await {
            log_error_and_send_to_sentry(
                anyhow!(err),
                "failed to ack nats message",
                ErrorKind::NatsAckFailed,
            );
        }
    }
}

pub fn next_suspend_interval(
    retry_count: u32,
    nats_consumer_config: &config::NatsConsumer,
) -> Duration {
    let seconds = std::cmp::min(
        nats_consumer_config.suspend_interval.as_secs() * 2_u64.pow(retry_count),
        nats_consumer_config.max_suspend_interval.as_secs(),
    );

    Duration::from_secs(seconds)
}

fn log_error_and_send_to_sentry(error: anyhow::Error, message: &str, kind: ErrorKind) {
    error!(%error, message);
    AppError::new(kind, error).notify_sentry();
}

async fn handle_message(conn: &mut PoolConnection<Postgres>, message: &Message) -> Result<()> {
    let subject = Subject::from_str(&message.subject)?;
    let entity_type = subject.entity_type.as_str();

    let event = serde_json::from_slice::<Event>(message.payload.as_ref())?;

    let (label, created_at) = match event {
        Event::V1(EventV1::VideoGroup(e)) => (e.as_label(), e.created_at()),
    };

    let classroom_id = subject.classroom_id;
    let room = db::room::FindQuery::new()
        .by_classroom_id(classroom_id)
        .execute(conn)
        .await?
        .ok_or(anyhow!(
            "failed to get room by classroom_id: {}",
            classroom_id
        ))?;

    let headers = svc_nats_client::Headers::try_from(message.headers.clone().unwrap_or_default())?;
    let agent_id = headers.sender_id();
    let entity_event_id = headers.event_id().sequence_id();

    let created_at: DateTime<Utc> = Utc.timestamp_nanos(created_at);
    let occurred_at = match room.time().map(|t| t.start().to_owned()) {
        Ok(opened_at) => (created_at - opened_at)
            .num_nanoseconds()
            .unwrap_or(std::i64::MAX),
        _ => {
            return Err(anyhow!("Invalid room time"));
        }
    };

    match db::event::InsertQuery::new(
        room.id(),
        entity_type.to_string(),
        json!({ entity_type: label }),
        occurred_at,
        agent_id.to_owned(),
    )
    .map_err(|e| anyhow!("invalid data: {}", e))?
    .entity_type(entity_type.to_string())
    .entity_event_id(entity_event_id)
    .execute(conn)
    .await
    {
        Err(sqlx::Error::Database(err)) => {
            if let Some("uniq_entity_type_entity_event_id") = err.constraint() {
                warn!(
                    "duplicate nats message, entity_type: {:?}, entity_event_id: {:?}",
                    entity_type.to_string(),
                    entity_event_id
                );

                Ok(())
            } else {
                Err(anyhow!("failed to create event from nats: {}", err))
            }
        }
        Err(err) => Err(anyhow!("failed to create event from nats: {}", err)),
        Ok(_) => Ok(()),
    }
}