use async_trait::async_trait;
use futures::{future, stream};
use serde_derive::Deserialize;
use serde_json::json;
use svc_agent::mqtt::{IncomingRequestProperties, ResponseStatus};
use svc_error::{extension::sentry, Error as SvcError};

use crate::app::context::Context;
use crate::app::endpoint::prelude::*;
use crate::app::operations::vacuum;

#[derive(Debug, Deserialize)]
pub(crate) struct VacuumRequest {}

pub(crate) struct VacuumHandler;

#[async_trait]
impl RequestHandler for VacuumHandler {
    type Payload = VacuumRequest;

    async fn handle<C: Context>(
        context: &mut C,
        _payload: Self::Payload,
        reqp: &IncomingRequestProperties,
    ) -> Result {
        // Authz: only trusted subjects.
        let authz_time = context
            .authz()
            .authorize(
                context.agent_id().as_account_id().audience().into(),
                reqp.as_account_id().to_owned(),
                AuthzObject::new(&["system"]).into(),
                "update".into(),
            )
            .await?;

        // Run vacuum operation asynchronously.
        let db = context.db().to_owned();
        let metrics = context.metrics();
        let logger = context.logger().new(o!());
        let config = context.config().vacuum.to_owned();

        tokio::task::spawn(async move {
            if let Err(err) = vacuum(&db, &metrics, &config).await {
                error!(logger, "Vacuum failed: {:?}", err);

                let svc_error = SvcError::builder()
                    .status(ResponseStatus::INTERNAL_SERVER_ERROR)
                    .kind("vacuum_failed", "Vacuum failed")
                    .detail(&err.to_string())
                    .build();

                sentry::send(svc_error).unwrap_or_else(|err| {
                    warn!(logger, "Error sending error to Sentry: {:?}", err);
                });
            }
        });

        // Return empty 202 response.
        Ok(Box::new(stream::once(future::ready(
            helpers::build_response(
                ResponseStatus::ACCEPTED,
                json!({}),
                reqp,
                context.start_timestamp(),
                Some(authz_time),
            ),
        ))))
    }
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    mod vacuum {
        use serde_json::Value as JsonValue;

        use crate::test_helpers::prelude::*;

        use super::super::*;

        #[tokio::test]
        async fn vacuum() {
            let mut authz = TestAuthz::new();
            authz.set_audience(SVC_AUDIENCE);

            // Allow cron to perform vacuum.
            let agent = TestAgent::new("alpha", "cron", SVC_AUDIENCE);
            authz.allow(agent.account_id(), vec!["system"], "update");

            // Make system.vacuum request.
            let mut context = TestContext::new(TestDb::new().await, authz);
            let payload = VacuumRequest {};

            let messages = handle_request::<VacuumHandler>(&mut context, &agent, payload)
                .await
                .expect("System vacuum failed");

            let (payload, respp, _) = find_response::<JsonValue>(messages.as_slice());
            assert_eq!(respp.status(), ResponseStatus::ACCEPTED);
            assert_eq!(payload, json!({}));
        }

        #[tokio::test]
        async fn vacuum_unauthorized() {
            let agent = TestAgent::new("web", "user123", USR_AUDIENCE);
            let mut context = TestContext::new(TestDb::new().await, TestAuthz::new());
            let payload = VacuumRequest {};

            let err = handle_request::<VacuumHandler>(&mut context, &agent, payload)
                .await
                .expect_err("Unexpected success on system vacuum");

            assert_eq!(err.status(), ResponseStatus::FORBIDDEN);
            assert_eq!(err.kind(), "access_denied");
        }
    }
}
