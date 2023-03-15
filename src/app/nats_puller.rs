use crate::{app::context::GlobalContext, db};
use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use futures_util::StreamExt;
use serde_json::json;
use std::{str::FromStr, sync::Arc, time::Duration};
use svc_conference_events::{Event, EventV1};
use svc_nats_client::{AckKind, Message, NatsClient, Subject};
use tokio::{sync::watch, task::JoinHandle};
use tracing::{error, info, warn};

pub fn run(
    ctx: Arc<dyn GlobalContext>,
    nats_client: Arc<dyn NatsClient>,
    stream: String,
    consumer: String,
    mut shutdown_rx: watch::Receiver<()>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let mut messages = match nats_client.subscribe(&stream, &consumer).await {
                Ok(messages) => messages,
                Err(err) => {
                    error!(%err, "failed to get the stream of messages from nats");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    continue;
                }
            };

            tokio::select! {
                Some(result) = messages.next() => {
                    let message = match result {
                        Ok(msg) => msg,
                        Err(err) => {
                            error!(%err, "failed to get a message from nats");
                            continue;
                        }
                    };

                    info!(
                        "got a message from nats, subject: {:?}, payload: {:?}, headers: {:?}",
                        message.subject,
                        message.payload,
                        message.headers
                    );

                    if let Err(err) = handle_message(ctx.clone(), &message).await {
                        error!(%err, "failed to handle nats message");

                        // todo: replace with nack + duration
                        if let Err(err) = message.ack_with(AckKind::Term).await {
                            error!(%err, "failed to term nats message");
                        }

                        continue;
                    }

                    if let Err(err) = message.ack().await {
                        error!(%err, "failed to ack nats message");
                        continue;
                    }
                }
                // Graceful shutdown
                _ = shutdown_rx.changed() => {
                    break;
                }
            }
        }
    })
}

async fn handle_message(ctx: Arc<dyn GlobalContext>, message: &Message) -> Result<()> {
    let subject = Subject::from_str(&message.subject)?;
    let entity_type = subject.entity_type.as_str();

    let event = match serde_json::from_slice::<Event>(message.payload.as_ref()) {
        Ok(event) => event,
        Err(err) => {
            warn!(%err, "The version of the event is not supported");
            return Ok(());
        }
    };

    let (label, created_at) = match event {
        Event::V1(EventV1::VideoGroup(e)) => (e.as_label(), e.created_at()),
    };

    let mut conn = ctx
        .get_conn()
        .await
        .map_err(|e| anyhow!("failed to get DB connection: {:?}", e))?;

    let classroom_id = subject.classroom_id;
    let room = db::room::FindQuery::new()
        .by_classroom_id(classroom_id)
        .execute(&mut conn)
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
                bail!("failed to create event from nats: {}", err);
            }
        }
        Err(err) => {
            bail!("failed to create event from nats: {}", err);
        }
        Ok(_) => Ok(()),
    }
}
