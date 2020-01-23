use chrono::{DateTime, Utc};
use failure::{format_err, Error};
use log::{error, info};
use svc_agent::mqtt::{
    compat, Agent, IncomingRequestProperties, IntoPublishableDump, OutgoingResponse,
    ResponseStatus, ShortTermTimingProperties,
};

use crate::app::{endpoint, Context, API_VERSION};

pub(crate) struct MessageHandler {
    agent: Agent,
    context: Context,
}

impl MessageHandler {
    pub(crate) fn new(agent: Agent, context: Context) -> Self {
        Self { agent, context }
    }

    pub(crate) async fn handle(&self, message_bytes: &[u8]) {
        info!(
            "Incoming message = '{}'",
            String::from_utf8_lossy(message_bytes)
        );

        if let Err(err) = self.handle_message(message_bytes).await {
            error!(
                "Error processing a message = '{}': {}",
                String::from_utf8_lossy(message_bytes),
                err,
            );
        }
    }

    async fn handle_message(&self, message_bytes: &[u8]) -> Result<(), Error> {
        let start_timestamp = Utc::now();
        let envelope = serde_json::from_slice::<compat::IncomingEnvelope>(message_bytes)?;

        match envelope.properties() {
            compat::IncomingEnvelopeProperties::Request(ref reqp) => {
                let reqp = reqp.to_owned();
                self.handle_request(envelope, reqp, start_timestamp)
            }
            compat::IncomingEnvelopeProperties::Response(_) => {
                // TOOD: svc_agent::request::Dispatcher::response
                Ok(())
            }
            compat::IncomingEnvelopeProperties::Event(_) => {
                // TODO: event router similar to request router
                Ok(())
            }
        }
    }

    fn handle_request(
        &self,
        envelope: compat::IncomingEnvelope,
        reqp: IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<(), Error> {
        let outgoing_messages =
            endpoint::route_request(&self.context, envelope, &reqp, start_timestamp)
                .unwrap_or_else(|| {
                    Self::error_response(
                        ResponseStatus::METHOD_NOT_ALLOWED,
                        "about:blank",
                        "Uknown method",
                        &format!("Unknown method '{}'", reqp.method()),
                        &reqp,
                        start_timestamp,
                    )
                });

        self.publish_outgoing_messages(outgoing_messages)
    }

    fn publish_outgoing_messages(
        &self,
        messages: Vec<Box<dyn IntoPublishableDump>>,
    ) -> Result<(), Error> {
        let address = self.agent.address();
        let mut agent = self.agent.clone();

        for message in messages {
            let dump = message
                .into_dump(address)
                .map_err(|err| format_err!("Failed to dump message: {}", err))?;

            info!(
                "Outgoing message = '{}' sending to the topic = '{}'",
                dump.payload(),
                dump.topic(),
            );

            agent
                .publish_dump(dump)
                .map_err(|err| format_err!("Failed to publish message: {}", err))?;
        }

        Ok(())
    }

    fn error_response(
        status: ResponseStatus,
        kind: &str,
        title: &str,
        detail: &str,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Vec<Box<dyn IntoPublishableDump>> {
        let err = svc_error::Error::builder()
            .status(status)
            .kind(kind, title)
            .detail(detail)
            .build();

        let timing = ShortTermTimingProperties::until_now(start_timestamp);
        let props = reqp.to_response(status, timing);
        let resp = OutgoingResponse::unicast(err, props, reqp, API_VERSION);
        vec![Box::new(resp) as Box<dyn IntoPublishableDump>]
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) trait RequestEnvelopeHandler {
    fn handle_envelope(
        context: &Context,
        envelope: compat::IncomingEnvelope,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Vec<Box<dyn IntoPublishableDump>>;
}

impl<H: endpoint::RequestHandler> RequestEnvelopeHandler for H {
    fn handle_envelope(
        context: &Context,
        envelope: compat::IncomingEnvelope,
        reqp: &IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Vec<Box<dyn IntoPublishableDump>> {
        match envelope.payload::<H::Payload>() {
            Ok(payload) => {
                Self::handle(context, payload, reqp, start_timestamp).unwrap_or_else(|err| {
                    MessageHandler::error_response(
                        ResponseStatus::UNPROCESSABLE_ENTITY,
                        reqp.method(),
                        Self::ERROR_TITLE,
                        &err.to_string(),
                        &reqp,
                        start_timestamp,
                    )
                })
            }
            Err(err) => MessageHandler::error_response(
                ResponseStatus::BAD_REQUEST,
                reqp.method(),
                Self::ERROR_TITLE,
                &err.to_string(),
                reqp,
                start_timestamp,
            ),
        }
    }
}
