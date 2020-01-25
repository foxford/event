use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use failure::{format_err, Error};
use log::{error, info, warn};
use svc_agent::mqtt::{
    compat, Agent, IncomingEventProperties, IncomingRequestProperties, IntoPublishableDump,
    OutgoingResponse, ResponseStatus, ShortTermTimingProperties,
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
                self.handle_request(envelope, reqp, start_timestamp).await
            }
            compat::IncomingEnvelopeProperties::Response(_) => {
                // TOOD: svc_agent::request::Dispatcher::response
                Ok(())
            }
            compat::IncomingEnvelopeProperties::Event(ref evp) => {
                let evp = evp.to_owned();
                self.handle_event(envelope, evp, start_timestamp).await
            }
        }
    }

    async fn handle_request(
        &self,
        envelope: compat::IncomingEnvelope,
        reqp: IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<(), Error> {
        let outgoing_messages =
            endpoint::route_request(&self.context, envelope, &reqp, start_timestamp)
                .await
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

    async fn handle_event(
        &self,
        envelope: compat::IncomingEnvelope,
        evp: IncomingEventProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Result<(), Error> {
        match evp.label() {
            Some(label) => {
                let outgoing_messages =
                    endpoint::route_event(&self.context, envelope, &evp, start_timestamp)
                        .await
                        .unwrap_or_else(|| {
                            warn!("Unexpected event with label = '{}'", label);
                            vec![]
                        });

                self.publish_outgoing_messages(outgoing_messages)
            }
            None => {
                warn!("Got event with missing label");
                Ok(())
            }
        }
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

// These auto-traits are being defined on all request/event handlers.
// They do parsing of the envelope and payload, call the handler and perform error handling.
// So we don't implement these generic things in each handler.
// We just need to specify the payload type and specific logic.

pub(crate) trait RequestEnvelopeHandler<'async_trait> {
    fn handle_envelope(
        context: &'async_trait Context,
        envelope: compat::IncomingEnvelope,
        reqp: &'async_trait IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Vec<Box<dyn IntoPublishableDump>>> + Send + 'async_trait>>;
}

// Can't use `#[async_trait]` macro here because it's not smart enough to add `'async_trait`
// lifetime to `H` type parameter. The creepy stuff around the actual implementation is what
// this macro expands to based on https://github.com/dtolnay/async-trait#explanation.
impl<'async_trait, H: 'async_trait + Sync + endpoint::RequestHandler>
    RequestEnvelopeHandler<'async_trait> for H
{
    fn handle_envelope(
        context: &'async_trait Context,
        envelope: compat::IncomingEnvelope,
        reqp: &'async_trait IncomingRequestProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Vec<Box<dyn IntoPublishableDump>>> + Send + 'async_trait>>
    where
        Self: Sync + 'async_trait,
    {
        // The actual implementation.
        async fn handle_envelope<H: endpoint::RequestHandler>(
            context: &Context,
            envelope: compat::IncomingEnvelope,
            reqp: &IncomingRequestProperties,
            start_timestamp: DateTime<Utc>,
        ) -> Vec<Box<dyn IntoPublishableDump>> {
            // Parse the envelope with the payload type specified in the handler.
            match envelope.payload::<H::Payload>() {
                // Call handler.
                Ok(payload) => H::handle(context, payload, reqp, start_timestamp)
                    .await
                    .unwrap_or_else(|err| {
                        // Handler returned an error => 422.
                        MessageHandler::error_response(
                            ResponseStatus::UNPROCESSABLE_ENTITY,
                            reqp.method(),
                            H::ERROR_TITLE,
                            &err.to_string(),
                            &reqp,
                            start_timestamp,
                        )
                    }),
                // Bad envelope or payload format => 400.
                Err(err) => MessageHandler::error_response(
                    ResponseStatus::BAD_REQUEST,
                    reqp.method(),
                    H::ERROR_TITLE,
                    &err.to_string(),
                    reqp,
                    start_timestamp,
                ),
            }
        }

        Box::pin(handle_envelope::<H>(
            context,
            envelope,
            reqp,
            start_timestamp,
        ))
    }
}

pub(crate) trait EventEnvelopeHandler<'async_trait> {
    fn handle_envelope(
        context: &'async_trait Context,
        envelope: compat::IncomingEnvelope,
        evp: &'async_trait IncomingEventProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Vec<Box<dyn IntoPublishableDump>>> + Send + 'async_trait>>;
}

// This is the same as with the above.
impl<'async_trait, H: 'async_trait + endpoint::EventHandler> EventEnvelopeHandler<'async_trait>
    for H
{
    fn handle_envelope(
        context: &'async_trait Context,
        envelope: compat::IncomingEnvelope,
        evp: &'async_trait IncomingEventProperties,
        start_timestamp: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Vec<Box<dyn IntoPublishableDump>>> + Send + 'async_trait>>
    {
        // The actual implementation.
        async fn handle_envelope<H: endpoint::EventHandler>(
            context: &Context,
            envelope: compat::IncomingEnvelope,
            evp: &IncomingEventProperties,
            start_timestamp: DateTime<Utc>,
        ) -> Vec<Box<dyn IntoPublishableDump>> {
            // Parse event envelope with the payload from the handler.
            match envelope.payload::<H::Payload>() {
                // Call handler.
                Ok(payload) => H::handle(context, payload, evp, start_timestamp)
                    .await
                    .unwrap_or_else(|err| {
                        // Handler returned an error.
                        if let Some(label) = evp.label() {
                            error!("Failed to handle event with label = '{}': {}", label, err);
                        }

                        vec![]
                    }),
                Err(err) => {
                    // Bad envelope or payload format.
                    if let Some(label) = evp.label() {
                        error!("Failed to parse event with label = '{}': {}", label, err);
                    }

                    vec![]
                }
            }
        }

        Box::pin(handle_envelope::<H>(
            context,
            envelope,
            evp,
            start_timestamp,
        ))
    }
}
