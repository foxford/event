use std::{convert::TryFrom, str::FromStr, sync::Arc};

use anyhow::Context;
use chrono::{DateTime, TimeZone, Utc};
use svc_events::{
    ban::{BanAcceptedV1, BanCollaborationCompletedV1},
    Event, EventV1, VideoGroupEventV1,
};
use svc_nats_client::{
    consumer::{FailureKind, FailureKindExt, HandleMessageFailure},
    Subject,
};

use crate::{db, metrics::QueryKey};

use super::{
    error::{Error, ErrorExt, ErrorKind, ErrorKindExt},
    GlobalContext,
};

pub async fn route_message(
    ctx: Arc<dyn GlobalContext + Sync + Send>,
    msg: Arc<svc_nats_client::Message>,
) -> Result<(), HandleMessageFailure<anyhow::Error>> {
    let subject = Subject::from_str(&msg.subject)
        .context("parse nats subject")
        .permanent()?;

    let event = serde_json::from_slice::<Event>(msg.payload.as_ref())
        .context("parse nats payload")
        .permanent()?;

    let classroom_id = subject.classroom_id();
    let room = {
        let mut conn = ctx
            .get_conn()
            .await
            .map_err(anyhow::Error::from)
            .transient()?;

        db::room::FindQuery::by_classroom_id(classroom_id)
            .execute(&mut conn)
            .await
            .context("find room by classroom_id")
            .transient()?
            .ok_or(anyhow!(
                "failed to get room by classroom_id: {}",
                classroom_id
            ))
            .permanent()?
    };

    let headers = svc_nats_client::Headers::try_from(msg.headers.clone().unwrap_or_default())
        .context("parse nats headers")
        .permanent()?;
    let _agent_id = headers.sender_id();

    let r = match event {
        Event::V1(EventV1::VideoGroup(e)) => {
            handle_video_group(ctx.as_ref(), e, &room, subject, &headers).await
        }
        Event::V1(EventV1::BanAccepted(e)) => {
            handle_ban_accepted(ctx.as_ref(), e, &room, subject, &headers).await
        }
        _ => {
            // ignore
            Ok(())
        }
    };

    FailureKindExt::map_err(r, |e| anyhow!(e))
}

async fn handle_video_group(
    ctx: &(dyn GlobalContext + Sync),
    e: VideoGroupEventV1,
    room: &db::room::Object,
    subject: Subject,
    headers: &svc_nats_client::Headers,
) -> Result<(), HandleMessageFailure<Error>> {
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
        .map_err(|_| Error::from(ErrorKind::InvalidRoomTime))
        .permanent()?;

    let mut conn = ctx
        .get_conn()
        .await
        .map_err(|_| Error::from(ErrorKind::DbConnAcquisitionFailed))
        .transient()?;

    let query = db::event::InsertQuery::new(
        room.id(),
        entity_type.to_string(),
        serde_json::json!({ entity_type: label }),
        occurred_at,
        agent_id.to_owned(),
    )
    .context("invalid event data")
    .map_err(|e| e.kind(ErrorKind::InvalidEvent))
    .permanent()?
    .entity_type(entity_type.to_string())
    .entity_event_id(entity_event_id);

    let result = ctx
        .metrics()
        .measure_query(QueryKey::EventInsertQuery, query.execute(&mut conn))
        .await;

    if let Err(sqlx::Error::Database(ref err)) = result {
        if let Some("uniq_entity_type_entity_event_id") = err.constraint() {
            tracing::warn!(
                "duplicate nats message, entity_type: {:?}, entity_event_id: {:?}",
                entity_type.to_string(),
                entity_event_id
            );

            return Ok(());
        };
    }

    if let Err(err) = result {
        return Err(HandleMessageFailure::Transient(Error::new(
            super::error::ErrorKind::DbQueryFailed,
            anyhow!("failed to create event from nats: {}", err),
        )));
    }

    Ok(())
}

async fn handle_ban_accepted(
    ctx: &(dyn GlobalContext + Sync),
    e: BanAcceptedV1,
    room: &db::room::Object,
    subject: Subject,
    headers: &svc_nats_client::Headers,
) -> Result<(), HandleMessageFailure<Error>> {
    let mut conn = ctx.get_conn().await.transient()?;

    if e.ban {
        let mut query = db::room_ban::InsertQuery::new(e.target_account.clone(), room.id());
        query.reason("ban event");

        ctx.metrics()
            .measure_query(QueryKey::BanInsertQuery, query.execute(&mut conn))
            .await
            .context("Failed to insert room ban")
            .map_err(|e| Error::new(ErrorKind::DbQueryFailed, e))
            .transient()?;
    } else {
        let query = db::room_ban::DeleteQuery::new(e.target_account.clone(), room.id());

        ctx.metrics()
            .measure_query(QueryKey::BanDeleteQuery, query.execute(&mut conn))
            .await
            .context("Failed to delete room ban")
            .map_err(|e| Error::new(ErrorKind::DbQueryFailed, e))
            .transient()?;
    }

    let event_id = headers.event_id();
    let event = BanCollaborationCompletedV1::new_from_accepted(e, event_id.clone());
    let event = Event::from(event);

    let payload = serde_json::to_vec(&event)
        .error(ErrorKind::InvalidPayload)
        .permanent()?;

    let event = svc_nats_client::event::Builder::new(
        subject,
        payload,
        event_id.to_owned(),
        ctx.agent_id().to_owned(),
    )
    .build();

    ctx.nats_client()
        .ok_or_else(|| anyhow!("nats client not found"))
        .error(ErrorKind::NatsClientNotFound)
        .transient()?
        .publish(&event)
        .await
        .error(ErrorKind::NatsPublishFailed)
        .transient()?;

    Ok(())
}
