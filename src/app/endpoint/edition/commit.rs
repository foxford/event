use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::extract::{Json, Path, State};
use chrono::Utc;
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use svc_agent::{
    mqtt::{OutgoingEvent, OutgoingEventProperties, ResponseStatus, ShortTermTimingProperties}
};
use svc_authn::Authenticable;
use svc_error::Error as SvcError;
use svc_utils::extractors::AgentIdExtractor;
use tracing::{error, field::display, instrument, Span};
use uuid::Uuid;

use crate::app::endpoint::prelude::*;
use crate::app::operations::commit_edition;
use crate::app::{context::Context, message_handler::Message};
use crate::db;
use crate::db::adjustment::Segments;

pub(crate) struct CommitHandler;

#[derive(Debug, Deserialize)]
pub struct CommitPayload {
    #[serde(default)]
    offset: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CommitRequest {
    id: Uuid,
    #[serde(flatten)]
    payload: CommitPayload,
}

pub async fn commit(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(id): Path<Uuid>,
    Json(payload): Json<CommitPayload>,
) -> RequestResult {
    let request = CommitRequest { id, payload };
    CommitHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

#[async_trait]
impl RequestHandler for CommitHandler {
    type Payload = CommitRequest;

    #[instrument(skip_all, fields(edition_id, offset, room_id, scope, classroom_id,))]
    async fn handle<C: Context>(
        context: &mut C,
        CommitRequest {
            id,
            payload: CommitPayload { offset },
        }: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        Span::current().record("edition_id", &display(id));
        Span::current().record("offset", &display(offset));
        // Find edition with its source room.
        let (edition, room) = {
            let query = db::edition::FindWithRoomQuery::new(id);
            let mut conn = context.get_ro_conn().await?;

            let maybe_edition = context
                .metrics()
                .measure_query(QueryKey::EditionFindWithRoomQuery, query.execute(&mut conn))
                .await
                .context("Failed to find edition with room")
                .error(AppErrorKind::DbQueryFailed)?;

            match maybe_edition {
                Some(edition_with_room) => edition_with_room,
                None => {
                    return Err(anyhow!("Edition not found")).error(AppErrorKind::EditionNotFound);
                }
            }
        };

        helpers::add_room_logger_tags(&room);

        // Authorize room update.
        let object = AuthzObject::room(&room).into();

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                "update".into(),
            )
            .await?;

        // Run commit task asynchronously.
        let db = context.db().to_owned();
        let metrics = context.metrics();
        let cfg = context.config().to_owned();

        let notification_future = tokio::task::spawn(async move {
            let result = commit_edition(&db, &metrics, &edition, &room, offset, cfg.adjust).await;

            // Handle result.
            let result = match result {
                Ok((destination, modified_segments)) => EditionCommitResult::Success {
                    source_room_id: edition.source_room_id(),
                    committed_room_id: destination.id(),
                    modified_segments,
                },
                Err(err) => {
                    error!("Room adjustment job failed: {:?}", err);
                    let app_error = AppError::new(AppErrorKind::EditionCommitTaskFailed, err);
                    app_error.notify_sentry();
                    EditionCommitResult::Error {
                        error: app_error.to_svc_error(),
                    }
                }
            };

            // Publish success/failure notification.
            let notification = EditionCommitNotification {
                status: result.status(),
                tags: room.tags().map(|t| t.to_owned()),
                result,
            };

            let timing = ShortTermTimingProperties::new(Utc::now());
            let props = OutgoingEventProperties::new("edition.commit", timing);
            let path = format!("audiences/{}/events", room.audience());
            let event = OutgoingEvent::broadcast(notification, props, &path);

            Box::new(event) as Message
        });

        // Respond with 202.
        // The actual task result will be broadcasted to the room topic when finished.
        let mut response = AppResponse::new(
            ResponseStatus::ACCEPTED,
            json!({}),
            context.start_timestamp(),
            Some(authz_time),
        );

        response.add_async_task(notification_future);

        Ok(response)
    }
}

#[derive(Serialize)]
struct EditionCommitNotification {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<JsonValue>,
    #[serde(flatten)]
    result: EditionCommitResult,
}

#[derive(Serialize)]
#[serde(untagged)]
enum EditionCommitResult {
    Success {
        source_room_id: Uuid,
        committed_room_id: Uuid,
        #[serde(with = "crate::db::adjustment::serde::segments")]
        modified_segments: Segments,
    },
    Error {
        error: SvcError,
    },
}

impl EditionCommitResult {
    fn status(&self) -> &'static str {
        match self {
            Self::Success { .. } => "success",
            Self::Error { .. } => "error",
        }
    }
}