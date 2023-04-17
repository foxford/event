use std::sync::Arc;

use anyhow::Context as AnyhowContext;
use async_trait::async_trait;
use axum::{
    extract::{Extension, Path, Query},
};
use chrono::Utc;
use serde_derive::{Deserialize};
use svc_agent::{mqtt::ResponseStatus, Addressable};
use svc_utils::extractors::AuthnExtractor;
use tracing::{Span, instrument};
use uuid::Uuid;

use crate::app::endpoint::{prelude::*};
use crate::db;


#[derive(Debug, Clone, Deserialize)]
pub struct DeletePayload {
    set: Option<String>,
    edition_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteRequest {
    pub room_id: Uuid,
    #[serde(flatten)]
    pub payload: DeletePayload,
}

pub async fn delete_by_room(
    Extension(ctx): Extension<Arc<AppContext>>,
    AuthnExtractor(agent_id): AuthnExtractor,
    Path(room_id): Path<Uuid>,
    Query(payload): Query<DeletePayload>,
) -> RequestResult {
    let request = DeleteRequest { room_id, payload };
    DeleteHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub(crate) struct DeleteHandler;


#[async_trait]
impl RequestHandler for DeleteHandler {
    type Payload = DeleteRequest;

    #[instrument(skip_all, fields(room_id, scope, classroom_id))]
    async fn handle<C: Context>(
        context: &mut C,
        Self::Payload { room_id, payload }: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {

        let room = helpers::find_room(context, room_id, helpers::RoomTimeRequirement::Open).await?;
        let set = payload.set.as_ref().map(String::as_str);
        let edition_id = payload.edition_id;
        if let Some(ref set) = set {
            Span::current().record("set", &set);
        }
        if let Some(ref edition_id) = edition_id {
            let edition_id_string = edition_id.to_string();
            Span::current().record("edition_id", &edition_id_string.as_str());
        }

        let query = db::event::mass_delete_by_room::MassDeleteQuery::new(
            room.id(),
            set,
            edition_id,
        );

        let mut conn = context.get_ro_conn().await?;

        context
            .metrics()
            .measure_query(QueryKey::EventOriginalEventQuery, query.execute(&mut conn))
            .await
            .context("Failed to find original event")
            .error(AppErrorKind::DbQueryFailed)?;

        // NOTE:
        // Currently we simply override authz object to room update if event type is in locked_types
        // or if room mandates whiteboard access validation and the user is not allowed access through whiteboard access map
        //
        // Assumption here is that admins can always update rooms and its ok for them to post messages in locked chat
        // So room update authz check works for them
        // But the same check always fails for common users
        //
        // This is probably a temporary solution, relying on room update being allowed only to those who can post in locked chat
        let (object, action) = {
            let object = room.authz_object();
            let object = object.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
            (AuthzObject::new(&object).into(), "delete")
        };

        let authz_time = context
            .authz()
            .authorize(
                room.audience().into(),
                reqp.as_account_id().to_owned(),
                object,
                action.into(),
            )
            .await?;

        // Calculate occurrence date.
        let occurred_at = match room.time().map(|t| t.start().to_owned()) {
            Ok(opened_at) => (Utc::now() - opened_at)
                .num_nanoseconds()
                .unwrap_or(std::i64::MAX),
            _ => {
                return Err(anyhow!("Invalid room time")).error(AppErrorKind::InvalidRoomTime);
            }
        };

        let event = {
            // Build transient event.
            let mut builder = db::event::Builder::new()
                .room_id(room_id)
                .occurred_at(occurred_at)
                .created_by(reqp.as_agent_id());

            if let Some(set) = &set {
                builder = builder.set(*set)
            }

            builder
                .build()
                .map_err(|err| anyhow!("Error building transient event: {:?}", err))
                .error(AppErrorKind::TransientEventCreationFailed)?
        };

        // Respond to the agent.
        let mut response = AppResponse::new(
            ResponseStatus::OK,
            "",
            context.start_timestamp(),
            Some(authz_time),
        );

        // Notify room subscribers.
        response.add_notification(
            "event.delete",
            &format!("rooms/{}/events", room.id()),
            event,
            context.start_timestamp(),
        );

        Ok(response)
    }
}
