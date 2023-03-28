use crate::{
    app::{
        context::GlobalContext,
        error::{Error as AppError, ErrorKind},
    },
    db,
};
use chrono::{DateTime, TimeZone, Utc};
use futures_util::StreamExt;
use serde_json::json;
use std::{str::FromStr, sync::Arc, time::Duration};
use svc_conference_events::{Event, EventV1};
use svc_nats_client::{
    error::{AckKind, Error as NatsError, ErrorExt as NatsErrorExt},
    AckKind as NatsAckKind, Message, NatsClient, Subject,
};
use tokio::{sync::watch, task::JoinHandle};
use tracing::{error, info, warn};

pub async fn run(
    ctx: Arc<dyn GlobalContext>,
    nats_client: Arc<dyn NatsClient>,
    config: &svc_nats_client::Config,
    mut shutdown_rx: watch::Receiver<()>,
) -> anyhow::Result<JoinHandle<()>> {
    let stream = config
        .stream
        .as_ref()
        .ok_or_else(|| anyhow!("missing nats stream in config"))?;
    let consumer = config
        .consumer
        .as_ref()
        .ok_or_else(|| anyhow!("missing nats consumer in config"))?;
    let redelivery_interval = config
        .redelivery_interval
        .ok_or_else(|| anyhow!("missing nats redelivery_interval in config"))?;

    let mut messages = nats_client.subscribe(stream, consumer).await?;

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

                    match handle_message(ctx.clone(), &message, redelivery_interval).await {
                        Ok(_) => {
                            if let Err(err) = message.ack().await {
                                log_error_and_send_to_sentry(
                                    anyhow!(err),
                                    "failed to ack nats message",
                                    ErrorKind::NatsAckFailed,
                                );
                            }
                        }
                        Err(NatsError { error, ack_kind }) => {
                            log_error_and_send_to_sentry(
                                error,
                                "failed to handle nats message",
                                ErrorKind::NatsMessageHandlingFailed,
                            );

                            match ack_kind {
                                AckKind::Nak(duration) => {
                                    if let Err(err) = message.ack_with(NatsAckKind::Nak(Some(duration))).await {
                                        log_error_and_send_to_sentry(
                                            anyhow!(err),
                                            "failed to nack nats message",
                                            ErrorKind::NatsTermFailed,
                                        );
                                    }
                                }
                                AckKind::Term => {
                                    if let Err(err) = message.ack_with(NatsAckKind::Term).await {
                                        log_error_and_send_to_sentry(
                                            anyhow!(err),
                                            "failed to term nats message",
                                            ErrorKind::NatsTermFailed,
                                        );
                                    }
                                },
                            }
                        }
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

fn log_error_and_send_to_sentry(error: anyhow::Error, message: &str, kind: ErrorKind) {
    error!(%error, message);
    AppError::new(kind, error).notify_sentry();
}

async fn handle_message(
    ctx: Arc<dyn GlobalContext>,
    message: &Message,
    redelivery_interval: Duration,
) -> Result<(), NatsError> {
    let subject = Subject::from_str(&message.subject).ack_with(AckKind::Term)?;
    let entity_type = subject.entity_type.as_str();

    let event =
        serde_json::from_slice::<Event>(message.payload.as_ref()).ack_with(AckKind::Term)?;

    let (label, created_at) = match event {
        Event::V1(EventV1::VideoGroup(e)) => (e.as_label(), e.created_at()),
    };

    let mut conn = ctx
        .get_conn()
        .await
        .map_err(|e| anyhow!("failed to get DB connection: {:?}", e))
        .ack_with(AckKind::Nak(redelivery_interval))?;

    let classroom_id = subject.classroom_id;
    let room = db::room::FindQuery::new()
        .by_classroom_id(classroom_id)
        .execute(&mut conn)
        .await
        .ack_with(AckKind::Term)?
        .ok_or(anyhow!(
            "failed to get room by classroom_id: {}",
            classroom_id
        ))
        .ack_with(AckKind::Term)?;

    let headers = svc_nats_client::Headers::try_from(message.headers.clone().unwrap_or_default())
        .ack_with(AckKind::Term)?;
    let agent_id = headers.sender_id();
    let entity_event_id = headers.event_id().sequence_id();

    let created_at: DateTime<Utc> = Utc.timestamp_nanos(created_at);
    let occurred_at = match room.time().map(|t| t.start().to_owned()) {
        Ok(opened_at) => (created_at - opened_at)
            .num_nanoseconds()
            .unwrap_or(std::i64::MAX),
        _ => {
            return Err(anyhow!("Invalid room time")).ack_with(AckKind::Term);
        }
    };

    match db::event::InsertQuery::new(
        room.id(),
        entity_type.to_string(),
        json!({ entity_type: label }),
        occurred_at,
        agent_id.to_owned(),
    )
    .map_err(|e| anyhow!("invalid data: {}", e))
    .ack_with(AckKind::Term)?
    .entity_type(entity_type.to_string())
    .entity_event_id(entity_event_id)
    .execute(&mut conn)
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
                Err(anyhow!("failed to create event from nats: {}", err)).ack_with(AckKind::Term)
            }
        }
        Err(err) => {
            Err(anyhow!("failed to create event from nats: {}", err)).ack_with(AckKind::Term)
        }
        Ok(_) => Ok(()),
    }
}
