use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use serde_derive::Deserialize;
use svc_agent::mqtt::ResponseStatus;
use svc_authn::Authenticable;
use svc_utils::extractors::AgentIdExtractor;
use tracing::instrument;
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::db;

pub(crate) struct ListHandler;

#[derive(Debug, Deserialize)]
pub struct ListPayload {
    pub last_created_at: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ListRequest {
    pub room_id: Uuid,
    #[serde(flatten)]
    pub payload: ListPayload,
}

pub async fn list(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
    Query(payload): Query<ListPayload>,
) -> RequestResult {
    let request = ListRequest { room_id, payload };
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

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<C: Context>(
        context: &mut C,
        Self::Payload { room_id, payload }: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let room = helpers::find_room(context, room_id, helpers::RoomTimeRequirement::Any).await?;

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

        let mut query = db::edition::ListQuery::new(room.id());

        if let Some(last_created_at) = payload.last_created_at {
            query = query.last_created_at(last_created_at);
        }

        if let Some(limit) = payload.limit {
            query = query.limit(limit);
        }

        let editions = {
            let mut conn = context.get_ro_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::EditionListQuery, query.execute(&mut conn))
                .await
                .context("Failed to list editions")
                .error(AppErrorKind::DbQueryFailed)?
        };

        // Respond with events list.
        Ok(AppResponse::new(
            ResponseStatus::OK,
            editions,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}

////////////////////////////////////////////////////////////////////////////////
