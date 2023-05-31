use crate::{
    app::{
        context::GlobalContext,
        error::{Error as AppError, ErrorKind, ErrorKindExt},
    },
    config, db,
    metrics::QueryKey,
};
use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use futures_util::StreamExt;
use serde_json::json;
use std::{str::FromStr, sync::Arc, time::Duration};
use svc_events::{ban::BanAcceptedV1, Event, EventV1, VideoGroupEventV1};
use svc_nats_client::{
    AckKind as NatsAckKind, Client, Message, MessageStream, NatsClient, Subject, SubscribeError,
};
use tokio::{sync::watch, task::JoinHandle, time::Instant};
use tracing::{error, info, warn};

pub async fn run(
    ctx: Arc<dyn GlobalContext + Send>,
    nats_client: Client,
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
                        anyhow!(err)
                            .kind(ErrorKind::NatsSubscriptionFailed)
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
                &nats_client,
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
                        error
                            .kind(ErrorKind::NatsSubscriptionFailed)
                            .notify_sentry();
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
    nats_client: &Client,
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
                        anyhow!(err)
                            .context("internal nats error")
                            .kind(ErrorKind::InternalNatsError)
                            .log()
                            .notify_sentry();

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
                            anyhow!(err)
                                .context("nats ack error")
                                .kind(ErrorKind::NatsPublishFailed)
                                .log()
                                .notify_sentry();
                        }
                    }
                    Err(HandleMessageError::DbConnAcquisitionFailed(err)) => {
                        err.log().notify_sentry();

                        if let Err(err) = message.ack_with(NatsAckKind::Nak(None)).await {
                            anyhow!(err)
                                .context("nats nack error")
                                .kind(ErrorKind::NatsPublishFailed)
                                .log()
                                .notify_sentry();
                        }

                        retry_count += 1;
                        let interval = next_suspend_interval(retry_count, nats_consumer_config);
                        suspend_interval = Some(interval);
                    }
                    Err(HandleMessageError::Other(err)) => {
                        err
                            .kind(ErrorKind::NatsMessageHandlingFailed)
                            .log()
                            .notify_sentry();

                        if let Err(err) = nats_client.terminate(message).await {
                            anyhow!(err)
                                .context("failed to handle nats message")
                                .kind(ErrorKind::NatsPublishFailed)
                                .log()
                                .notify_sentry();
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

enum HandleMessageError {
    DbConnAcquisitionFailed(AppError),
    Other(anyhow::Error),
}

impl From<anyhow::Error> for HandleMessageError {
    fn from(error: anyhow::Error) -> Self {
        HandleMessageError::Other(error)
    }
}

async fn handle_message(
    ctx: &dyn GlobalContext,
    message: &Message,
) -> Result<(), HandleMessageError> {
    let subject = Subject::from_str(&message.subject).context("parse nats subject")?;
    let event =
        serde_json::from_slice::<Event>(message.payload.as_ref()).context("parse nats payload")?;

    let classroom_id = subject.classroom_id();
    let room = {
        let mut conn = ctx
            .get_conn()
            .await
            .map_err(HandleMessageError::DbConnAcquisitionFailed)?;

        db::room::FindQuery::by_classroom_id(classroom_id)
            .execute(&mut conn)
            .await
            .context("find room by classroom_id")?
            .ok_or(HandleMessageError::Other(anyhow!(
                "failed to get room by classroom_id: {}",
                classroom_id
            )))?
    };

    let headers = svc_nats_client::Headers::try_from(message.headers.clone().unwrap_or_default())
        .context("parse nats headers")?;

    match event {
        Event::V1(EventV1::VideoGroup(e)) => {
            handle_video_group(ctx, e, &room, subject, &headers).await?;
        }
        Event::V1(EventV1::BanAccepted(e)) => {
            handle_ban_accepted(ctx, e, &room).await?;
        }
        _ => {
            // ignore
        }
    };

    Ok(())
}

async fn handle_video_group(
    ctx: &dyn GlobalContext,
    e: VideoGroupEventV1,
    room: &db::room::Object,
    subject: Subject,
    headers: &svc_nats_client::Headers,
) -> Result<(), HandleMessageError> {
    let (label, created_at) = (e.as_label().to_owned(), e.created_at());
    let entity_type = subject.entity_type();
    let agent_id = headers.sender_id();
    let entity_event_id = headers.event_id().sequence_id();

    let created_at: DateTime<Utc> = Utc.timestamp_nanos(created_at);
    let occurred_at = room
        .time()
        .map(|t| {
            (created_at - t.start().to_owned())
                .num_nanoseconds()
                .unwrap_or(i64::MAX)
        })
        .map_err(|_| HandleMessageError::Other(anyhow!("invalid room time")))?;

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
    .context("invalid event data")?
    .entity_type(entity_type.to_string())
    .entity_event_id(entity_event_id)
    .execute(&mut conn)
    .await;

    if let Err(sqlx::Error::Database(ref err)) = result {
        if let Some("uniq_entity_type_entity_event_id") = err.constraint() {
            warn!(
                "duplicate nats message, entity_type: {:?}, entity_event_id: {:?}",
                entity_type.to_string(),
                entity_event_id
            );

            return Ok(());
        };
    }

    if let Err(err) = result {
        return Err(HandleMessageError::Other(anyhow!(
            "failed to create event from nats: {}",
            err
        )));
    }

    Ok(())
}

async fn handle_ban_accepted(
    ctx: &dyn GlobalContext,
    e: BanAcceptedV1,
    room: &db::room::Object,
) -> Result<(), HandleMessageError> {
    let mut conn = ctx
        .get_conn()
        .await
        .map_err(HandleMessageError::DbConnAcquisitionFailed)?;

    if e.ban {
        let mut query = db::room_ban::InsertQuery::new(e.user_account, room.id());
        query.reason("ban event");

        ctx.metrics()
            .measure_query(QueryKey::BanInsertQuery, query.execute(&mut conn))
            .await
            .context("Failed to insert room ban")
            .map_err(HandleMessageError::Other)?;
    } else {
        let query = db::room_ban::DeleteQuery::new(e.user_account, room.id());

        ctx.metrics()
            .measure_query(QueryKey::BanDeleteQuery, query.execute(&mut conn))
            .await
            .context("Failed to delete room ban")
            .map_err(HandleMessageError::Other)?;
    }

    Ok(())
}
