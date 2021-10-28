use std::{
    sync::Arc,
    task::{Context, Poll},
};

use crate::app::message_handler::publish_message;
use axum::{
    handler::{delete, get, post},
    response::IntoResponse,
    routing::BoxRoute,
    AddExtensionLayer, Router,
};

use futures::{future::BoxFuture, StreamExt};
use futures_util::pin_mut;
use http::{Request, Response};

use svc_agent::mqtt::{Agent, IntoPublishableMessage};
use tower::{layer::layer_fn, Service};

use super::{context::AppContext, endpoint, error};
use crate::app::message_handler::MessageStream;

pub fn build_router(
    context: Arc<AppContext>,
    agent: Agent,
    authn: svc_authn::jose::ConfigMap,
) -> Router<BoxRoute> {
    let router = Router::new()
        .route("/rooms", post(endpoint::room::create))
        .route(
            "/rooms/:id",
            get(endpoint::room::read).patch(endpoint::room::update),
        )
        .route("/rooms/:id/adjust", post(endpoint::room::adjust))
        .route("/rooms/:id/enter", post(endpoint::room::enter))
        .route(
            "/rooms/:id/locked_types",
            post(endpoint::room::locked_types),
        )
        .route("/rooms/:id/dump_events", post(endpoint::room::dump_events))
        .route(
            "/rooms/:id/events",
            get(endpoint::event::list).post(endpoint::event::create),
        )
        .route("/rooms/:id/state", get(endpoint::state::read))
        .route(
            "/rooms/:id/agents",
            get(endpoint::agent::list).patch(endpoint::agent::update),
        )
        .route(
            "/rooms/:id/editions",
            get(endpoint::edition::list).post(endpoint::edition::create),
        )
        .route("/editions/:id", delete(endpoint::edition::delete))
        .route("/editions/:id/commit", post(endpoint::edition::commit))
        .route("/editions/:id", delete(endpoint::edition::delete))
        .route(
            "/editions/:id/changes",
            get(endpoint::change::list).post(endpoint::change::create),
        )
        .route("/changes/:id", delete(endpoint::change::delete))
        .layer(layer_fn(|inner| NotificationsMiddleware { inner }))
        .layer(svc_utils::middleware::CorsLayer)
        .layer(AddExtensionLayer::new(context))
        .layer(AddExtensionLayer::new(agent))
        .layer(AddExtensionLayer::new(authn));
    let router = Router::new().nest("/api/v1", router);
    router.boxed()
}

impl IntoResponse for error::Error {
    type Body = axum::body::Body;

    type BodyError = <Self::Body as axum::body::HttpBody>::Error;

    fn into_response(self) -> hyper::Response<Self::Body> {
        let err = svc_error::Error::builder()
            .status(self.status())
            .kind(self.kind(), self.title())
            .detail(&self.source().to_string())
            .build();
        let error =
            serde_json::to_string(&err).unwrap_or_else(|_| "Failed to serialize error".to_string());
        http::Response::builder()
            .status(self.status())
            .body(axum::body::Body::from(error))
            .expect("This is a valid response")
    }
}

#[derive(Clone)]
struct NotificationsMiddleware<S> {
    inner: S,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for NotificationsMiddleware<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
    ResBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        // best practice is to clone the inner service like this
        // see https://github.com/tower-rs/tower/issues/547 for details
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        Box::pin(async move {
            let mut agent = req.extensions().get::<Agent>().cloned().unwrap();
            let mut res: Response<ResBody> = inner.call(req).await?;
            if let Some(notifications) = res
                .extensions_mut()
                .remove::<Vec<Box<dyn IntoPublishableMessage + Send + Sync + 'static>>>()
            {
                for notification in notifications {
                    if let Err(err) = publish_message(&mut agent, notification) {
                        error!("Failed to publish message, err = {:?}", err);
                    }
                }
            }

            if let Some(notifications_stream) = res.extensions_mut().remove::<MessageStream>() {
                tokio::task::spawn(async move {
                    pin_mut!(notifications_stream);
                    while let Some(message) = notifications_stream.next().await {
                        if let Err(err) = publish_message(&mut agent, message) {
                            error!("Failed to publish message, err = {:?}", err);
                        }
                    }
                });
            }

            Ok(res)
        })
    }
}
