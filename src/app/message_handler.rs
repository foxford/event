use std::future::Future;
use std::pin::Pin;

use anyhow::Context as AnyhowContext;
use chrono::{DateTime, Utc};
use futures::{future, stream, Stream, StreamExt};
use futures_util::pin_mut;
use svc_agent::{
    mqtt::{
        Agent, IncomingEvent, IncomingMessage, IncomingRequest, IncomingRequestProperties,
        IncomingResponse, IntoPublishableMessage, OutgoingResponse, ShortTermTimingProperties,
    },
    Addressable, Authenticable,
};
use tracing::{error, warn};
use tracing_attributes::instrument;

use crate::app::context::{AppMessageContext, Context, GlobalContext, MessageContext};
use crate::app::error::{Error as AppError, ErrorExt, ErrorKind as AppErrorKind};
use crate::app::{endpoint, API_VERSION};

////////////////////////////////////////////////////////////////////////////////

pub(crate) type MessageStream =
    Box<dyn Stream<Item = Box<dyn IntoPublishableMessage + Send>> + Send + Unpin>;

pub(crate) struct MessageHandler<C: GlobalContext> {
    agent: Agent,
    global_context: C,
}

impl<C: GlobalContext + Sync> MessageHandler<C> {
    pub(crate) fn new(agent: Agent, global_context: C) -> Self {
        Self {
            agent,
            global_context,
        }
    }

    pub(crate) fn agent(&self) -> &Agent {
        &self.agent
    }

    pub(crate) fn global_context(&self) -> &C {
        &self.global_context
    }

    pub(crate) async fn handle(&self, message: &Result<IncomingMessage<String>, String>) {
        let mut msg_context = AppMessageContext::new(&self.global_context, Utc::now());

        match message {
            Ok(ref msg) => {
                if let Err(err) = self.handle_message(&mut msg_context, msg).await {
                    let err = err.to_string();
                    Self::report_error(message, &err).await;
                }
            }
            Err(e) => {
                Self::report_error(message, e).await;
            }
        }
    }

    async fn report_error(message: &Result<IncomingMessage<String>, String>, err: &str) {
        error!("Error processing a message: {:?}: {:?}", message, err);

        let app_error = AppError::new(
            AppErrorKind::MessageHandlingFailed,
            anyhow!(err.to_string()),
        );

        app_error.notify_sentry();
    }

    async fn handle_message(
        &self,
        msg_context: &mut AppMessageContext<'_, C>,
        message: &IncomingMessage<String>,
    ) -> Result<(), AppError> {
        match message {
            IncomingMessage::Request(req) => self.handle_request(msg_context, req).await,
            IncomingMessage::Event(ev) => self.handle_event(msg_context, ev).await,
            IncomingMessage::Response(resp) => self.handle_response(msg_context, resp).await,
        }
    }

    #[instrument(
        skip(self, msg_context, request),
        fields(
            agent_label = request.properties().as_agent_id().label(),
            account_id = request.properties().as_agent_id().as_account_id().label(),
            audience = request.properties().as_agent_id().as_account_id().audience(),
            method = request.properties().method()
        )
    )]
    async fn handle_request(
        &self,
        msg_context: &mut AppMessageContext<'_, C>,
        request: &IncomingRequest<String>,
    ) -> Result<(), AppError> {
        let outgoing_message_stream = endpoint::route_request(msg_context, request)
            .await
            .unwrap_or_else(|| {
                let err = anyhow!("Unknown method '{}'", request.properties().method());
                let app_error = AppError::new(AppErrorKind::UnknownMethod, err);

                error_response(
                    app_error,
                    request.properties(),
                    msg_context.start_timestamp(),
                )
            });

        self.publish_outgoing_messages(outgoing_message_stream)
            .await
    }

    #[instrument(
        skip(self, msg_context, response),
        fields(
            agent_label = response.properties().as_agent_id().label(),
            account_id = response.properties().as_agent_id().as_account_id().label(),
            audience = response.properties().as_agent_id().as_account_id().audience(),
        )
    )]
    async fn handle_response(
        &self,
        msg_context: &mut AppMessageContext<'_, C>,
        response: &IncomingResponse<String>,
    ) -> Result<(), AppError> {
        let raw_corr_data = response.properties().correlation_data();

        let corr_data = match endpoint::CorrelationData::parse(raw_corr_data) {
            Ok(corr_data) => corr_data,
            Err(err) => {
                warn!(
                    "Failed to parse response correlation data '{}': {:?}",
                    raw_corr_data, err
                );

                return Ok(());
            }
        };

        let outgoing_message_stream =
            endpoint::route_response(msg_context, response, &corr_data).await;

        self.publish_outgoing_messages(outgoing_message_stream)
            .await
    }

    #[instrument(
        skip(self, msg_context, event),
        fields(
            agent_label = event.properties().as_agent_id().label(),
            account_id = event.properties().as_agent_id().as_account_id().label(),
            audience = event.properties().as_agent_id().as_account_id().audience(),
            label = event.properties().label()
        )
    )]
    async fn handle_event(
        &self,
        msg_context: &mut AppMessageContext<'_, C>,
        event: &IncomingEvent<String>,
    ) -> Result<(), AppError> {
        match event.properties().label() {
            Some(label) => {
                let outgoing_message_stream = endpoint::route_event(msg_context, event)
                    .await
                    .unwrap_or_else(|| {
                        warn!("Unexpected event with label = '{}'", label);
                        Box::new(stream::empty())
                    });

                self.publish_outgoing_messages(outgoing_message_stream)
                    .await
            }
            None => {
                warn!("Got event with missing label");
                Ok(())
            }
        }
    }

    async fn publish_outgoing_messages(
        &self,
        message_stream: MessageStream,
    ) -> Result<(), AppError> {
        let mut agent = self.agent.clone();
        pin_mut!(message_stream);

        while let Some(message) = message_stream.next().await {
            publish_message(&mut agent, message)?;
        }

        Ok(())
    }
}

fn error_response(
    err: AppError,
    reqp: &IncomingRequestProperties,
    start_timestamp: DateTime<Utc>,
) -> MessageStream {
    let timing = ShortTermTimingProperties::until_now(start_timestamp);
    let props = reqp.to_response(err.status(), timing);
    let e = err.to_svc_error();
    let resp = OutgoingResponse::unicast(e, props, reqp, API_VERSION);

    Box::new(stream::once(future::ready(
        Box::new(resp) as Box<dyn IntoPublishableMessage + Send>
    )))
}

pub(crate) fn publish_message(
    agent: &mut Agent,
    message: Box<dyn IntoPublishableMessage>,
) -> Result<(), AppError> {
    agent
        .publish_publishable(message)
        .map_err(|err| anyhow!("Failed to publish message: {:?}", err))
        .error(AppErrorKind::PublishFailed)
}

///////////////////////////////////////////////////////////////////////////////

// These auto-traits are being defined on all request/event handlers.
// They do parsing of the envelope and payload, call the handler and perform error handling.
// So we don't implement these generic things in each handler.
// We just need to specify the payload type and specific logic.

pub(crate) trait RequestEnvelopeHandler<'async_trait> {
    fn handle_envelope<C: Context>(
        context: &'async_trait mut C,
        request: &'async_trait IncomingRequest<String>,
    ) -> Pin<Box<dyn Future<Output = MessageStream> + Send + 'async_trait>>;
}

// Can't use `#[async_trait]` macro here because it's not smart enough to add `'async_trait`
// lifetime to `H` type parameter. The creepy stuff around the actual implementation is what
// this macro expands to based on https://github.com/dtolnay/async-trait#explanation.
impl<'async_trait, H: 'async_trait + Sync + endpoint::RequestHandler>
    RequestEnvelopeHandler<'async_trait> for H
{
    fn handle_envelope<C: Context>(
        context: &'async_trait mut C,
        request: &'async_trait IncomingRequest<String>,
    ) -> Pin<Box<dyn Future<Output = MessageStream> + Send + 'async_trait>>
    where
        Self: Sync + 'async_trait,
    {
        // The actual implementation.
        async fn handle_envelope<H: endpoint::RequestHandler, C: Context>(
            context: &mut C,
            request: &IncomingRequest<String>,
        ) -> MessageStream {
            // Parse the envelope with the payload type specified in the handler.
            let payload = IncomingRequest::convert_payload::<H::Payload>(request);
            let reqp = request.properties();
            match payload {
                // Call handler.
                Ok(payload) => {
                    let app_result = H::handle(context, payload, reqp).await;
                    context.metrics().observe_app_result(&app_result);
                    app_result.unwrap_or_else(|app_error| {
                        error!(err = ?app_error, status = app_error.status().as_u16(), kind = app_error.kind(), "Failed to handle request");

                        app_error.notify_sentry();

                        // Handler returned an error.
                        error_response(app_error, reqp, context.start_timestamp())
                    })
                }
                // Bad envelope or payload format => 400.
                Err(err) => {
                    let app_error =
                        AppError::new(AppErrorKind::InvalidPayload, anyhow::Error::from(err));
                    error_response(app_error, reqp, context.start_timestamp())
                }
            }
        }

        Box::pin(handle_envelope::<H, C>(context, request))
    }
}

// This is the same as with the above.
pub(crate) trait ResponseEnvelopeHandler<'async_trait, CD> {
    fn handle_envelope<C: Context>(
        context: &'async_trait mut C,
        envelope: &'async_trait IncomingResponse<String>,
        corr_data: &'async_trait CD,
    ) -> Pin<Box<dyn Future<Output = MessageStream> + Send + 'async_trait>>;
}

impl<'async_trait, H: 'async_trait + endpoint::ResponseHandler>
    ResponseEnvelopeHandler<'async_trait, H::CorrelationData> for H
{
    fn handle_envelope<C: Context>(
        context: &'async_trait mut C,
        response: &'async_trait IncomingResponse<String>,
        corr_data: &'async_trait H::CorrelationData,
    ) -> Pin<Box<dyn Future<Output = MessageStream> + Send + 'async_trait>> {
        // The actual implementation.
        async fn handle_envelope<H: endpoint::ResponseHandler, C: Context>(
            context: &mut C,
            response: &IncomingResponse<String>,
            corr_data: &H::CorrelationData,
        ) -> MessageStream {
            // Parse response envelope with the payload from the handler.
            let payload = IncomingResponse::convert_payload::<H::Payload>(response);
            let respp = response.properties();

            match payload {
                // Call handler.
                Ok(payload) => {
                    let app_result = H::handle(context, payload, respp, corr_data).await;
                    context.metrics().observe_app_result(&app_result);
                    app_result.unwrap_or_else(|app_error| {
                        // Handler returned an error.
                        error!(err = ?app_error, status = app_error.status().as_u16(), kind = app_error.kind(), "Failed to handle response");


                        app_error.notify_sentry();
                        Box::new(stream::empty())
                    })
                }
                Err(err) => {
                    // Bad envelope or payload format.
                    error!("Failed to parse response: {:?}", err);
                    Box::new(stream::empty())
                }
            }
        }

        Box::pin(handle_envelope::<H, C>(context, response, corr_data))
    }
}

pub(crate) trait EventEnvelopeHandler<'async_trait> {
    fn handle_envelope<C: Context>(
        context: &'async_trait mut C,
        envelope: &'async_trait IncomingEvent<String>,
    ) -> Pin<Box<dyn Future<Output = MessageStream> + Send + 'async_trait>>;
}

// This is the same as with the above.
impl<'async_trait, H: 'async_trait + endpoint::EventHandler> EventEnvelopeHandler<'async_trait>
    for H
{
    fn handle_envelope<C: Context>(
        context: &'async_trait mut C,
        event: &'async_trait IncomingEvent<String>,
    ) -> Pin<Box<dyn Future<Output = MessageStream> + Send + 'async_trait>> {
        // The actual implementation.
        async fn handle_envelope<H: endpoint::EventHandler, C: Context>(
            context: &mut C,
            event: &IncomingEvent<String>,
        ) -> MessageStream {
            // Parse event envelope with the payload from the handler.
            let payload = IncomingEvent::convert_payload::<H::Payload>(event);
            let evp = event.properties();

            match payload {
                // Call handler.
                Ok(payload) => {
                    let app_result = H::handle(context, payload, evp).await;
                    context.metrics().observe_app_result(&app_result);
                    app_result.unwrap_or_else(|app_error| {
                        // Handler returned an error.
                        error!(err = ?app_error, status = app_error.status().as_u16(), kind = app_error.kind(), "Failed to handle event");


                        app_error.notify_sentry();
                        Box::new(stream::empty())
                    })
                }
                Err(err) => {
                    // Bad envelope or payload format.
                    error!("Failed to parse event: {:?}", err);
                    Box::new(stream::empty())
                }
            }
        }

        Box::pin(handle_envelope::<H, C>(context, event))
    }
}

////////////////////////////////////////////////////////////////////////////////

impl endpoint::CorrelationData {
    pub(crate) fn dump(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).context("Failed to dump correlation data")
    }

    fn parse(raw_corr_data: &str) -> anyhow::Result<Self> {
        serde_json::from_str::<Self>(raw_corr_data).context("Failed to parse correlation data")
    }
}
