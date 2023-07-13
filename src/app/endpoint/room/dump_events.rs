use async_trait::async_trait;
use chrono::Utc;
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use svc_agent::mqtt::{
    OutgoingEvent, OutgoingEventProperties, ResponseStatus, ShortTermTimingProperties,
};
use svc_error::Error as SvcError;
use tracing::error;
use uuid::Uuid;

use super::*;
use crate::app::context::Context;
use crate::app::message_handler::Message;
use crate::app::operations::dump_events_to_s3;

#[derive(Debug, Deserialize)]
pub struct EventsDumpRequest {
    id: Uuid,
}

#[derive(Serialize)]
struct EventsDumpNotification {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<JsonValue>,
    result: EventsDumpResult,
}

#[derive(Serialize)]
#[serde(untagged)]
enum EventsDumpResult {
    Success { room_id: Uuid, s3_uri: String },
    Error { error: SvcError },
}

impl EventsDumpResult {
    fn status(&self) -> &'static str {
        match self {
            Self::Success { .. } => "success",
            Self::Error { .. } => "error",
        }
    }
}

pub async fn dump_events(
    ctx: extract::Extension<Arc<AppContext>>,
    AgentIdExtractor(agent_id): AgentIdExtractor,
    Path(room_id): Path<Uuid>,
) -> RequestResult {
    let request = EventsDumpRequest { id: room_id };
    EventsDumpHandler::handle(
        &mut ctx.start_message(),
        request,
        RequestParams::Http {
            agent_id: &agent_id,
        },
    )
    .await
}

pub struct EventsDumpHandler;

#[async_trait]
impl RequestHandler for EventsDumpHandler {
    type Payload = EventsDumpRequest;

    async fn handle<C: Context>(
        context: &mut C,
        payload: Self::Payload,
        reqp: RequestParams<'_>,
    ) -> RequestResult {
        let room =
            helpers::find_room(context, payload.id, helpers::RoomTimeRequirement::Any).await?;

        let object = AuthzObject::new(&["classrooms"]).into();

        // Authorize room.
        let authz_time = context
            .authz()
            .authorize(
                room.audience().to_owned(),
                reqp.as_account_id().to_owned(),
                object,
                "dump_events".into(),
            )
            .await?;

        let db = context.db().to_owned();
        let metrics = context.metrics();

        let s3_client = context
            .s3_client()
            .ok_or_else(|| {
                error!("DumpEvents called with no s3client in context");
                anyhow!("No S3Client")
            })
            .error(AppErrorKind::NoS3Client)?;

        let notification_future = tokio::task::spawn(async move {
            let result = dump_events_to_s3(&db, &metrics, s3_client, &room).await;

            // Handle result.
            let result = match result {
                Ok(s3_uri) => EventsDumpResult::Success {
                    room_id: room.id(),
                    s3_uri,
                },
                Err(err) => {
                    error!("Events dump job failed: {:?}", err);
                    let app_error = AppError::new(AppErrorKind::EditionCommitTaskFailed, err);
                    app_error.notify_sentry();
                    EventsDumpResult::Error {
                        error: app_error.to_svc_error(),
                    }
                }
            };

            // Publish success/failure notification.
            let notification = EventsDumpNotification {
                status: result.status(),
                tags: room.tags().map(|t| t.to_owned()),
                result,
            };

            let timing = ShortTermTimingProperties::new(Utc::now());
            let props = OutgoingEventProperties::new("room.dump_events", timing);
            let path = format!("audiences/{}/events", room.audience());
            let event = OutgoingEvent::broadcast(notification, props, &path);

            Box::new(event) as Message
        });

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::prelude::*;

    #[tokio::test]
    async fn dump_events_not_authorized() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let db = TestDb::new().await;

        let room = {
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        let mut context = TestContext::new(db, TestAuthz::new());

        let payload = EventsDumpRequest { id: room.id() };

        let err = handle_request::<EventsDumpHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room dump");

        assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
    }

    #[tokio::test]
    async fn dump_events_room_missing() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());

        let payload = EventsDumpRequest { id: Uuid::new_v4() };

        let err = handle_request::<EventsDumpHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room dump");

        assert_eq!(err.status(), ResponseStatus::NOT_FOUND);
        assert_eq!(err.kind(), "room_not_found");
    }

    #[tokio::test]
    async fn dump_events_no_s3_client() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let db = TestDb::new().await;
        let mut authz = TestAuthz::new();
        authz.allow(agent.account_id(), vec!["classrooms"], "dump_events");

        let room = {
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        let mut context = TestContext::new(TestDb::new().await, authz);

        let payload = EventsDumpRequest { id: room.id() };

        let err = handle_request::<EventsDumpHandler>(&mut context, &agent, payload)
            .await
            .expect_err("Unexpected success on room dump");

        assert_eq!(err.status(), ResponseStatus::NOT_IMPLEMENTED);
        assert_eq!(err.kind(), "no_s3_client");
    }

    #[tokio::test]
    async fn dump_events() {
        let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
        let db = TestDb::new().await;
        let mut authz = TestAuthz::new();
        authz.allow(agent.account_id(), vec!["classrooms"], "dump_events");

        let room = {
            let mut conn = db.get_conn().await;
            shared_helpers::insert_room(&mut conn).await
        };

        let mut context = TestContext::new(TestDb::new().await, authz);
        context.set_s3(shared_helpers::mock_s3());

        let payload = EventsDumpRequest { id: room.id() };

        let messages = handle_request::<EventsDumpHandler>(&mut context, &agent, payload)
            .await
            .expect("Failed to dump room events");

        assert_eq!(messages.len(), 2);
        let (_, respp, _) = find_response::<JsonValue>(messages.as_slice());
        let (ev, evp, _) = find_event::<JsonValue>(messages.as_slice());
        assert_eq!(respp.status(), ResponseStatus::ACCEPTED);
        assert_eq!(evp.label(), "room.dump_events");
        assert_eq!(
            ev.get("result")
                .and_then(|v| v.get("room_id"))
                .and_then(|v| v.as_str()),
            Some(room.id().to_string()).as_deref()
        );
        assert_eq!(
            ev.get("result")
                .and_then(|v| v.get("s3_uri"))
                .and_then(|v| v.as_str()),
            Some(format!(
                "s3://eventsdump.{}/{}.json",
                room.audience(),
                room.id()
            ))
            .as_deref()
        );
    }
}
