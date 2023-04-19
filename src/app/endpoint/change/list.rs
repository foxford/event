use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::extract::{Extension, Path, Query};
use chrono::{DateTime, Utc};
use serde_derive::Deserialize;
use svc_agent::mqtt::ResponseStatus;
use svc_authn::Authenticable;
use svc_utils::extractors::AuthnExtractor;
use tracing::{field::display, instrument, Span};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::db;

pub(crate) struct ListHandler;

#[derive(Debug, Deserialize)]
pub struct ListPayload {
    pub last_created_at: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ListRequest {
    pub id: Uuid,
    #[serde(flatten)]
    pub payload: ListPayload,
}

pub async fn list(
    Extension(ctx): Extension<Arc<AppContext>>,
    AuthnExtractor(agent_id): AuthnExtractor,
    Path(id): Path<Uuid>,
    Query(payload): Query<ListPayload>,
) -> RequestResult {
    let request = ListRequest { id, payload };
    ListHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

#[async_trait]
impl RequestHandler for ListHandler {
    type Payload = ListRequest;

    #[instrument(skip_all, fields(edition_id, scope, room_id, classroom_id, change_id))]
    async fn handle<C: Context>(
        context: &mut C,
        Self::Payload { id, payload }: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        Span::current().record("edition_id", &display(id));
        let (edition, room) = {
            let query = db::edition::FindWithRoomQuery::new(id);
            let mut conn = context.get_ro_conn().await?;

            let maybe_edition_with_room = context
                .metrics()
                .measure_query(QueryKey::EditionFindWithRoomQuery, query.execute(&mut conn))
                .await
                .context("Failed to find edition")
                .error(AppErrorKind::DbQueryFailed)?;

            match maybe_edition_with_room {
                Some(edition_with_room) => edition_with_room,
                None => {
                    return Err(anyhow!("Edition not found"))
                        .error(AppErrorKind::EditionNotFound)?;
                }
            }
        };

        helpers::add_room_logger_tags(&room);

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

        let mut query = db::change::ListQuery::new(edition.id());

        if let Some(last_created_at) = payload.last_created_at {
            query = query.last_created_at(last_created_at);
        }

        if let Some(limit) = payload.limit {
            query = query.limit(limit);
        }

        let changes = {
            let mut conn = context.get_ro_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::ChangeListQuery, query.execute(&mut conn))
                .await
                .context("Failed to list changes")
                .error(AppErrorKind::DbQueryFailed)?
        };

        Ok(AppResponse::new(
            ResponseStatus::OK,
            changes,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}
