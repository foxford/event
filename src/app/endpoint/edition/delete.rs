
use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::extract::{Extension, Path};
use serde_derive::Deserialize;
use svc_agent::mqtt::ResponseStatus;
use svc_authn::Authenticable;
use svc_utils::extractors::AuthnExtractor;
use tracing::instrument;
use uuid::Uuid;

use crate::app::endpoint::prelude::*;
use crate::app::context::Context;
use crate::db;

pub(crate) struct DeleteHandler;

#[derive(Debug, Deserialize)]
pub(crate) struct DeleteRequest {
    pub id: Uuid,
}

pub async fn delete(
    Extension(ctx): Extension<Arc<AppContext>>,
    AuthnExtractor(agent_id): AuthnExtractor,
    Path(id): Path<Uuid>,
) -> RequestResult {
    let request = DeleteRequest { id };
    DeleteHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

#[async_trait]
impl RequestHandler for DeleteHandler {
    type Payload = DeleteRequest;

    #[instrument(
        skip_all,
        fields(
            edition_id = %payload.id,
            room_id, scope, classroom_id
        )
    )]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let (edition, room) = {
            let query = db::edition::FindWithRoomQuery::new(payload.id);
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

        {
            let query = db::edition::DeleteQuery::new(edition.id());
            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::EditionDeleteQuery, query.execute(&mut conn))
                .await
                .context("Failed to delete edition")
                .error(AppErrorKind::DbQueryFailed)?;
        }

        Ok(AppResponse::new(
            ResponseStatus::OK,
            edition,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}