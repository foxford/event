use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::extract::{Path, State};
use serde_derive::Deserialize;
use svc_agent::mqtt::ResponseStatus;
use svc_authn::Authenticable;
use svc_utils::extractors::AgentIdExtractor;
use tracing::{field::display, instrument, Span};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::db;

////////////////////////////////////////////////////////////////////////////////

pub struct DeleteHandler;

#[derive(Debug, Deserialize)]
pub struct DeleteRequest {
    pub id: Uuid,
}

pub async fn delete(
    State(ctx): State<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
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
            change_id = %payload.id,
            scope, room_id, classroom_id, edition_id
        )
    )]
    async fn handle<'a, C: Context + Sync + Send>(
        context: &'a mut C,
        payload: Self::Payload,
        reqp: RequestParams<'a>,
    ) -> RequestResult {
        let (change, room) = {
            let query = db::change::FindWithRoomQuery::new(payload.id);
            let mut conn = context.get_ro_conn().await?;

            let maybe_change_with_room = context
                .metrics()
                .measure_query(QueryKey::ChangeFindWithRoomQuery, query.execute(&mut conn))
                .await
                .context("Failed to find change with room")
                .error(AppErrorKind::DbQueryFailed)?;

            match maybe_change_with_room {
                Some(change_with_room) => change_with_room,
                None => {
                    return Err(anyhow!("Change not found")).error(AppErrorKind::ChangeNotFound)?;
                }
            }
        };

        helpers::add_room_logger_tags(&room);
        Span::current().record("edition_id", &display(change.edition_id()));

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
            let query = db::change::DeleteQuery::new(change.id());
            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::ChangeDeleteQuery, query.execute(&mut conn))
                .await
                .context("Failed to delete change")
                .error(AppErrorKind::DbQueryFailed)?;
        }

        Ok(AppResponse::new(
            ResponseStatus::OK,
            change,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}
