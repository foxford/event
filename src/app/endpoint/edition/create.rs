use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::extract::{self, Path};
use serde_derive::Deserialize;
use svc_agent::{mqtt::ResponseStatus, Addressable};
use svc_authn::Authenticable;
use svc_utils::extractors::AgentIdExtractor;
use tracing::{field::display, instrument, Span};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::db;

pub struct CreateHandler;

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub room_id: Uuid,
}

pub async fn create(
    ctx: extract::Extension<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
) -> RequestResult {
    let request = CreateRequest { room_id };
    CreateHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

#[async_trait]
impl RequestHandler for CreateHandler {
    type Payload = CreateRequest;

    #[instrument(
        skip_all,
        fields(
            room_id = %payload.room_id,
            scope, classroom_id, edition_id
        )
    )]
    async fn handle<C: Context + Sync + Send>(
        context: &mut C,
        payload: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let room =
            helpers::find_room(context, payload.room_id, helpers::RoomTimeRequirement::Any).await?;

        let object = {
            let object = room.authz_object();
            let object = object.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
            AuthzObject::new(&object).into()
        };

        let authz_time = context
            .authz()
            .authorize(
                room.audience().to_owned(),
                reqp.as_account_id().to_owned(),
                object,
                "update".into(),
            )
            .await?;

        let edition = {
            let query = db::edition::InsertQuery::new(payload.room_id, reqp.as_agent_id());
            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::EditionInsertQuery, query.execute(&mut conn))
                .await
                .context("Failed to insert edition")
                .error(AppErrorKind::DbQueryFailed)?
        };

        Span::current().record("edition_id", &display(edition.id()));

        let mut response = AppResponse::new(
            ResponseStatus::CREATED,
            edition.clone(),
            context.start_timestamp(),
            Some(authz_time),
        );

        response.add_notification(
            "edition.create",
            &format!("rooms/{}/editions", payload.room_id),
            edition,
            context.start_timestamp(),
        );

        Ok(response)
    }
}
