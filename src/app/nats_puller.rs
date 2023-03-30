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
use svc_nats_client::{AckKind as NatsAckKind, Message, NatsClient, Subject};
use tokio::{sync::watch, task::JoinHandle};
use tracing::{error, info, warn};

pub async fn run(
    ctx: Arc<dyn GlobalContext>,
    nats_client: Arc<dyn NatsClient>,
    nats_puller_config: config::NatsPuller,
    mut shutdown_rx: watch::Receiver<()>,
) -> Result<JoinHandle<()>> {
    let mut retry_count = 0;
    let mut messages = nats_client.subscribe().await?;

    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(result) = messages.next() => {
                    let message = match result {
                        Ok(msg) => msg,
                        Err(err) => {
                            log_error_and_send_to_sentry(
                                anyhow!(err),
                                "failed to get a message from nats",
                                ErrorKind::NatsGettingMessageFailed,
                            );

                            continue;
                        }
                    };

                    info!(
                        "got a message from nats, subject: {:?}, payload: {:?}, headers: {:?}",
                        message.subject,
                        message.payload,
                        message.headers
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
                            let wait_interval = next_wait_interval(retry_count, &nats_puller_config);
                            warn!("nats puller suspenses the processing of nats messages on {} seconds", wait_interval.as_secs());
                            tokio::time::sleep(wait_interval).await;

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
                // Graceful shutdown
                _ = shutdown_rx.changed() => {
                    break;
                }
            }
        }
    });

    Ok(handle)
}

pub fn next_wait_interval(retry_count: u32, nats_puller_config: &config::NatsPuller) -> Duration {
    let seconds = std::cmp::min(
        nats_puller_config.wait_interval.as_secs() * 2_u64.pow(retry_count),
        nats_puller_config.max_wait_interval.as_secs(),
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
