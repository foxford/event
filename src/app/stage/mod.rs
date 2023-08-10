use std::{convert::TryFrom, str::FromStr, sync::Arc};

use anyhow::Context;
use chrono::{DateTime, TimeZone, Utc};
use svc_events::{
    ban::{BanAcceptedV1, BanCollaborationCompletedV1},
    Event, EventId, EventV1, VideoGroupEventV1,
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

    tracing::info!(?event, class_id = %classroom_id);

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

    let event_id = EventId::from((
        event_id.entity_type().to_owned(),
        "collaboration_completed".to_owned(),
        event_id.sequence_id(),
    ));

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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::prelude::*;

    #[tokio::test]
    async fn ban_accepted_handler_enables_disables_ban() {
        let db = TestDb::new().await;
        let agent = TestAgent::new("web", "user", USR_AUDIENCE);

        let mut conn = db.get_conn().await;
        let room = shared_helpers::insert_room(&mut conn).await;

        let subject = Subject::new("test".to_string(), room.classroom_id(), "ban".to_string());
        let event_id: EventId = ("ban".to_string(), "accepted".to_string(), 0).into();
        let headers =
            svc_nats_client::test_helpers::HeadersBuilder::new(event_id, agent.agent_id().clone())
                .build();
        let e = BanAcceptedV1 {
            ban: true,
            classroom_id: room.classroom_id(),
            target_account: agent.account_id().clone(),
            operation_id: 0,
        };

        let ctx = TestContext::new(db, TestAuthz::new());

        handle_ban_accepted(&ctx, e, &room, subject, &headers)
            .await
            .expect("handler failed");

        // to drop pub_reqs after all checks
        {
            let pub_reqs = ctx.inspect_nats_client().get_publish_requests();
            assert_eq!(pub_reqs.len(), 1);

            let payload = serde_json::from_slice::<Event>(&pub_reqs[0].payload)
                .expect("failed to parse event");
            assert!(matches!(
                payload,
                Event::V1(EventV1::BanCollaborationCompleted(
                    BanCollaborationCompletedV1 { ban: true, .. }
                ))
            ));
        }

        let room_bans = db::room_ban::ListQuery::new(room.id())
            .execute(&mut conn)
            .await
            .expect("failed to list room bans");

        assert_eq!(room_bans.len(), 1);
        assert_eq!(room_bans[0].account_id(), agent.account_id());

        let subject = Subject::new("test".to_string(), room.classroom_id(), "ban".to_string());
        let e: BanAcceptedV1 = BanAcceptedV1 {
            ban: false,
            classroom_id: room.classroom_id(),
            target_account: agent.account_id().clone(),
            operation_id: 0,
        };

        handle_ban_accepted(&ctx, e, &room, subject, &headers)
            .await
            .expect("handler failed");

        // to drop pub_reqs after all checks
        {
            let pub_reqs = ctx.inspect_nats_client().get_publish_requests();
            assert_eq!(pub_reqs.len(), 2);

            let payload = serde_json::from_slice::<Event>(&pub_reqs[1].payload)
                .expect("failed to parse event");
            assert!(matches!(
                payload,
                Event::V1(EventV1::BanCollaborationCompleted(
                    BanCollaborationCompletedV1 { ban: false, .. }
                ))
            ));
        }

        let room_bans = db::room_ban::ListQuery::new(room.id())
            .execute(&mut conn)
            .await
            .expect("failed to list room bans");

        assert_eq!(room_bans.len(), 0);
    }
}
