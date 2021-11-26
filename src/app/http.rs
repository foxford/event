use std::{
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    response::IntoResponse,
    routing::{delete, get, post},
    AddExtensionLayer, Router,
};

use futures::{future::BoxFuture, StreamExt};
use futures_util::pin_mut;
use http::{Request, Response};
use hyper::{body::HttpBody, Body};
use svc_agent::mqtt::{Agent, IntoPublishableMessage};
use tower::{layer::layer_fn, Service};
use tower_http::trace::TraceLayer;
use tracing::{error, field::Empty, info, Span};

use crate::app::message_handler::publish_message;
use crate::app::message_handler::MessageStream;

use super::{context::AppContext, endpoint, error::Error as AppError};

pub fn build_router(
    context: Arc<AppContext>,
    agent: Agent,
    authn: svc_authn::jose::ConfigMap,
) -> Router {
    let router = Router::new()
        .route("/rooms", post(endpoint::room::create))
        .route(
            "/rooms/:id",
            get(endpoint::room::read)
                .patch(endpoint::room::update)
                .options(endpoint::read_options),
        )
        .route("/rooms/:id/adjust", post(endpoint::room::adjust))
        .route(
            "/rooms/:id/enter",
            post(endpoint::room::enter).options(endpoint::read_options),
        )
        .route(
            "/rooms/:id/locked_types",
            post(endpoint::room::locked_types).options(endpoint::read_options),
        )
        .route("/rooms/:id/dump_events", post(endpoint::room::dump_events))
        .route(
            "/rooms/:id/events",
            get(endpoint::event::list)
                .post(endpoint::event::create)
                .options(endpoint::read_options),
        )
        .route(
            "/rooms/:id/state",
            get(endpoint::state::read).options(endpoint::read_options),
        )
        .route(
            "/rooms/:id/agents",
            get(endpoint::agent::list)
                .patch(endpoint::agent::update)
                .options(endpoint::read_options),
        )
        .route(
            "/rooms/:id/editions",
            get(endpoint::edition::list)
                .post(endpoint::edition::create)
                .options(endpoint::read_options),
        )
        .route(
            "/rooms/:id/bans",
            get(endpoint::ban::list).options(endpoint::read_options),
        )
        .route(
            "/editions/:id",
            delete(endpoint::edition::delete).options(endpoint::read_options),
        )
        .route(
            "/editions/:id/commit",
            post(endpoint::edition::commit).options(endpoint::read_options),
        )
        .route(
            "/editions/:id/changes",
            get(endpoint::change::list)
                .post(endpoint::change::create)
                .options(endpoint::read_options),
        )
        .route(
            "/changes/:id",
            delete(endpoint::change::delete).options(endpoint::read_options),
        )
        .layer(layer_fn(|inner| NotificationsMiddleware { inner }))
        .layer(svc_utils::middleware::CorsLayer)
        .layer(AddExtensionLayer::new(context))
        .layer(AddExtensionLayer::new(agent))
        .layer(AddExtensionLayer::new(Arc::new(authn)));

    let routes = Router::new().nest("/api/v1", router);

    let pingz_router = Router::new().route(
        "/healthz",
        get(|| async { Response::builder().body(Body::from("pong")).unwrap() }),
    );

    let routes = routes.merge(pingz_router);

    routes.layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &Request<Body>| {
                tracing::error_span!(
                    "http-api-request",
                    status_code = Empty,
                    path = request.uri().path(),
                    query = request.uri().query(),
                    body_size = ?request.body().size_hint().upper()
                )
            })
            .on_response(|response: &Response<_>, latency: Duration, span: &Span| {
                span.record("status_code", &tracing::field::debug(response.status()));
                if response.status().is_success() {
                    info!("response generated in {:?}", latency)
                } else {
                    error!("response generated in {:?}", latency)
                }
            }),
    )
}

impl IntoResponse for AppError {
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
