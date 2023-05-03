use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::{
    extract::{Extension, Path},
    Json,
};
use svc_agent::mqtt::ResponseStatus;
use svc_authn::Authenticable;
use svc_utils::extractors::AuthnExtractor;
use tracing::{field::display, instrument, Span};
use uuid::Uuid;

use crate::app::context::Context;
use crate::app::endpoint::change::create_request::{Changeset, CreateRequest};
use crate::app::endpoint::prelude::*;
use crate::db;

pub(crate) struct CreateHandler;

pub async fn create(
    Extension(ctx): Extension<Arc<AppContext>>,
    AuthnExtractor(agent_id): AuthnExtractor,
    Path(edition_id): Path<Uuid>,
    Json(changeset): Json<Changeset>,
) -> RequestResult {
    let request = CreateRequest {
        edition_id,
        changeset,
    };
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
            edition_id = %payload.edition_id,
            scope, room_id, classroom_id, change_id
        )
    )]
    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let (_edition, room) = {
            let query = db::edition::FindWithRoomQuery::new(payload.edition_id);
            let mut conn = context.get_ro_conn().await?;

            let maybe_edition_with_room = context
                .metrics()
                .measure_query(QueryKey::EditionFindWithRoomQuery, query.execute(&mut conn))
                .await
                .context("Failed to find edition with room")
                .error(AppErrorKind::DbQueryFailed)?;

            match maybe_edition_with_room {
                Some(edition_with_room) => edition_with_room,
                None => {
                    return Err(anyhow!("Edition not found"))
                        .error(AppErrorKind::EditionNotFound)?
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

        let query =
            db::change::InsertQuery::new(payload.edition_id, payload.changeset.as_changetype());

        let query = match payload.changeset {
            Changeset::Addition(event) => query
                .event_kind(event.kind)
                .event_set(event.set)
                .event_label(event.label)
                .event_data(event.data)
                .event_occurred_at(event.occurred_at)
                .event_created_by(event.created_by),
            Changeset::Modification(event) => {
                let query = query.event_id(event.event_id);

                let query = match event.kind {
                    Some(kind) => query.event_kind(kind),
                    None => query,
                };

                let query = query.event_set(event.set);
                let query = query.event_label(event.label);

                let query = match event.data {
                    Some(data) => query.event_data(data),
                    None => query,
                };

                let query = match event.occurred_at {
                    Some(v) => query.event_occurred_at(v),
                    None => query,
                };

                match event.created_by {
                    Some(agent_id) => query.event_created_by(agent_id),
                    None => query,
                }
            }
            Changeset::Removal(event) => query.event_id(event.event_id),
            Changeset::BulkRemoval(event) => query.event_set(Some(event.set)),
        };

        let change = {
            let mut conn = context.get_conn().await?;

            context
                .metrics()
                .measure_query(QueryKey::ChangeInsertQuery, query.execute(&mut conn))
                .await
                .context("Failed to insert change")
                .error(AppErrorKind::DbQueryFailed)?
        };

        Span::current().record("change_id", &display(change.id()));

        Ok(AppResponse::new(
            ResponseStatus::CREATED,
            change,
            context.start_timestamp(),
            Some(authz_time),
        ))
    }
}
