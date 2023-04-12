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
use std::{str::FromStr, sync::Arc, time::Duration};
use svc_conference_events::{Event, EventV1};
use svc_nats_client::{
    AckKind as NatsAckKind, Message, MessageStream, NatsClient, Subject, SubscribeError,
};
use tokio::{sync::watch, task::JoinHandle, time::Instant};
use tracing::{error, info, warn};

pub async fn run(
    ctx: Arc<dyn GlobalContext + Send>,
    nats_client: Arc<dyn NatsClient>,
    nats_consumer_config: config::NatsConsumer,
    shutdown_rx: watch::Receiver<()>,
) -> Result<JoinHandle<Result<(), SubscribeError>>> {
    let handle = tokio::spawn(async move {
        // In case of subscription errors we don't want to spam sentry
        let mut sentry_last_sent = Instant::now() - nats_consumer_config.suspend_sentry_interval;

        loop {
            let result = nats_client.subscribe().await;
            let messages = match result {
                Ok(messages) => messages,
                Err(err) => {
                    error!(%err);

                    if sentry_last_sent.elapsed() >= nats_consumer_config.suspend_sentry_interval {
                        AppError::new(ErrorKind::NatsSubscriptionFailed, anyhow!(err))
                            .notify_sentry();
                        sentry_last_sent = Instant::now();
                    }

                    tokio::time::sleep(nats_consumer_config.resubscribe_interval).await;
                    continue;
                }
            };

            // Run the loop of getting messages from the stream
            let reason = handle_stream(
                ctx.as_ref(),
                nats_client.as_ref(),
                &nats_consumer_config,
                messages,
                shutdown_rx.clone(),
            )
            .await;

            match reason {
                CompletionReason::Shutdown => {
                    warn!("Nats consumer completes its work");
                    break;
                }
                CompletionReason::StreamClosed => {
                    // If the `handle_stream` function ends, then the stream was closed.
                    // Send an error to sentry and try to resubscribe.
                    let error = anyhow!("nats stream was closed");
                    error!(%error);

                    if sentry_last_sent.elapsed() >= nats_consumer_config.suspend_sentry_interval {
                        AppError::new(ErrorKind::NatsSubscriptionFailed, error).notify_sentry();
                        sentry_last_sent = Instant::now();
                    }

                    tokio::time::sleep(nats_consumer_config.resubscribe_interval).await;
                    continue;
                }
            }
        }

        Ok::<_, SubscribeError>(())
    });

    Ok(handle)
}

enum CompletionReason {
    Shutdown,
    StreamClosed,
}

async fn handle_stream(
    ctx: &dyn GlobalContext,
    nats_client: &dyn NatsClient,
    nats_consumer_config: &config::NatsConsumer,
    mut messages: MessageStream,
    mut shutdown_rx: watch::Receiver<()>,
) -> CompletionReason {
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

        tokio::select! {
            result = messages.next() => {
                let message = match result {
                    Some(Ok(msg)) => msg,
                    Some(Err(err)) => {
                        // Types of internal nats errors that may arise here:
                        // * Heartbeat errors
                        // * Failed to send request
                        // * Consumer deleted
                        // * Received unknown message
                        log_error_and_send_to_sentry(
                            anyhow!(err),
                            ErrorKind::InternalNatsError,
                        );

                        continue;
                    }
                    None => {
                        // Stream was closed. Send an error to sentry and try to resubscribe.
                        return CompletionReason::StreamClosed;
                    }
                };

                info!(
                    "got a message from nats, subject: {:?}, payload: {:?}, headers: {:?}",
                    message.subject, message.payload, message.headers
                );

                let result = handle_message(ctx, &message).await;
                match result {
                    Ok(_) => {
                        retry_count = 0;

                        if let Err(err) = message.ack().await {
                            log_error_and_send_to_sentry(anyhow!(err), ErrorKind::NatsPublishFailed);
                        }
                    }
                    Err(HandleMessageError::DbConnAcquisitionFailed(err)) => {
                        error!(?err);
                        err.notify_sentry();

                        if let Err(err) = message.ack_with(NatsAckKind::Nak(None)).await {
                            log_error_and_send_to_sentry(anyhow!(err), ErrorKind::NatsPublishFailed);
                        }

                        retry_count += 1;
                        let interval = next_suspend_interval(retry_count, nats_consumer_config);
                        suspend_interval = Some(interval);
                    }
                    Err(HandleMessageError::Other(err)) => {
                        log_error_and_send_to_sentry(err, ErrorKind::NatsMessageHandlingFailed);

                        if let Err(err) = nats_client.terminate(message).await {
                            log_error_and_send_to_sentry(anyhow!(err), ErrorKind::NatsPublishFailed);
                        }
                    }
                }
            }
            // Graceful shutdown
            _ = shutdown_rx.changed() => {
                return CompletionReason::Shutdown;
            }
        }
    }
}

fn next_suspend_interval(
    retry_count: u32,
    nats_consumer_config: &config::NatsConsumer,
) -> Duration {
    let seconds = std::cmp::min(
        nats_consumer_config.suspend_interval.as_secs() * 2_u64.pow(retry_count),
        nats_consumer_config.max_suspend_interval.as_secs(),
    );

    Duration::from_secs(seconds)
}

fn log_error_and_send_to_sentry(error: anyhow::Error, kind: ErrorKind) {
    error!(%error, "nats consumer");
    AppError::new(kind, error).notify_sentry();
}

enum HandleMessageError {
    DbConnAcquisitionFailed(AppError),
    Other(anyhow::Error),
}

async fn handle_message(
    ctx: &dyn GlobalContext,
    message: &Message,
) -> Result<(), HandleMessageError> {
    let subject =
        Subject::from_str(&message.subject).map_err(|e| HandleMessageError::Other(e.into()))?;
    let entity_type = subject.entity_type();

    let event = serde_json::from_slice::<Event>(message.payload.as_ref())
        .map_err(|e| HandleMessageError::Other(e.into()))?;

    let (label, created_at) = match event {
        Event::V1(EventV1::VideoGroup(e)) => (e.as_label().to_owned(), e.created_at()),
    };

    let classroom_id = subject.classroom_id();
    let room = {
        let mut conn = ctx
            .get_conn()
            .await
            .map_err(HandleMessageError::DbConnAcquisitionFailed)?;

        db::room::FindQuery::new()
            .by_classroom_id(classroom_id)
            .execute(&mut conn)
            .await
            .map_err(|e| HandleMessageError::Other(e.into()))?
            .ok_or(HandleMessageError::Other(anyhow!(
                "failed to get room by classroom_id: {}",
                classroom_id
            )))?
    };

    let headers = svc_nats_client::Headers::try_from(message.headers.clone().unwrap_or_default())
        .map_err(|e| HandleMessageError::Other(e.into()))?;
    let agent_id = headers.sender_id();
    let entity_event_id = headers.event_id().sequence_id();

    let created_at: DateTime<Utc> = Utc.timestamp_nanos(created_at);
    let occurred_at = match room.time().map(|t| t.start().to_owned()) {
        Ok(opened_at) => (created_at - opened_at)
            .num_nanoseconds()
            .unwrap_or(i64::MAX),
        _ => {
            return Err(HandleMessageError::Other(anyhow!("invalid room time")));
        }
    };

    let mut conn = ctx
        .get_conn()
        .await
        .map_err(HandleMessageError::DbConnAcquisitionFailed)?;

    let result = db::event::InsertQuery::new(
        room.id(),
        entity_type.to_string(),
        json!({ entity_type: label }),
        occurred_at,
        agent_id.to_owned(),
    )
    .map_err(|e| HandleMessageError::Other(anyhow!("invalid data: {}", e)))?
    .entity_type(entity_type.to_string())
    .entity_event_id(entity_event_id)
    .execute(&mut conn)
    .await;

    if let Err(sqlx::Error::Database(err)) = &result {
        if let Some("uniq_entity_type_entity_event_id") = err.constraint() {
            warn!(
                "duplicate nats message, entity_type: {:?}, entity_event_id: {:?}",
                entity_type.to_string(),
                entity_event_id
            );
        }
    }

    if let Err(err) = result {
        return Err(HandleMessageError::Other(anyhow!(
            "failed to create event from nats: {}",
            err
        )));
    }

    Ok(())
}
