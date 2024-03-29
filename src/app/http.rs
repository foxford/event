use std::{
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    response::IntoResponse,
    routing::{delete, get, post},
    Extension, Json, Router,
};

use futures::{future::BoxFuture, StreamExt};
use futures_util::pin_mut;
use http::{
    header::{HeaderName, AUTHORIZATION, CONTENT_TYPE},
    Method, Request, Response,
};
use hyper::Body;
use svc_agent::mqtt::Agent;
use svc_utils::middleware::MeteredRoute;
use tower::{layer::layer_fn, Service, ServiceBuilder};
use tower_http::cors::{Any, CorsLayer};
use tracing::error;

use crate::app::{
    message_handler::{publish_message, MessageStream},
    service_utils,
};

use super::{context::AppContext, endpoint, error::Error as AppError};

pub fn build_router(
    context: Arc<AppContext>,
    agent: Agent,
    authn: svc_authn::jose::ConfigMap,
) -> Router {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([
            AUTHORIZATION,
            CONTENT_TYPE,
            HeaderName::from_static("ulms-app-audience"),
            HeaderName::from_static("ulms-scope"),
            HeaderName::from_static("ulms-app-version"),
            HeaderName::from_static("ulms-app-label"),
            HeaderName::from_static("x-agent-label"),
        ])
        .max_age(std::time::Duration::from_secs(3600))
        .allow_origin(Any);

    let middleware = ServiceBuilder::new()
        .layer(Extension(agent))
        .layer(Extension(Arc::new(authn)))
        .layer(Extension(context))
        .layer(layer_fn(|inner| NotificationsMiddleware { inner }))
        .layer(cors);

    let router = Router::new()
        .metered_route("/rooms", post(endpoint::room::create))
        .metered_route(
            "/rooms/:id",
            get(endpoint::room::read)
                .patch(endpoint::room::update)
                .options(endpoint::read_options),
        )
        .metered_route("/rooms/:id/adjust", post(endpoint::room::adjust))
        .metered_route(
            "/rooms/:id/enter",
            post(endpoint::room::enter).options(endpoint::read_options),
        )
        .metered_route(
            "/rooms/:id/locked_types",
            post(endpoint::room::locked_types).options(endpoint::read_options),
        )
        .metered_route(
            "/rooms/:id/whiteboard_access",
            post(endpoint::room::whiteboard_access).options(endpoint::read_options),
        )
        .metered_route("/rooms/:id/dump_events", post(endpoint::room::dump_events))
        .metered_route(
            "/rooms/:id/events",
            get(endpoint::event::list)
                .post(endpoint::event::create)
                .options(endpoint::read_options),
        )
        .metered_route(
            "/rooms/:id/state",
            get(endpoint::state::read).options(endpoint::read_options),
        )
        .metered_route(
            "/rooms/:id/agents",
            get(endpoint::agent::list)
                .patch(endpoint::agent::update)
                .options(endpoint::read_options),
        )
        .metered_route(
            "/rooms/:id/editions",
            get(endpoint::edition::list)
                .post(endpoint::edition::create)
                .options(endpoint::read_options),
        )
        .metered_route(
            "/rooms/:id/bans",
            get(endpoint::ban::list).options(endpoint::read_options),
        )
        .metered_route(
            "/editions/:id",
            delete(endpoint::edition::delete).options(endpoint::read_options),
        )
        .metered_route(
            "/editions/:id/commit",
            post(endpoint::edition::commit).options(endpoint::read_options),
        )
        .metered_route(
            "/editions/:id/changes",
            get(endpoint::change::list)
                .post(endpoint::change::create)
                .options(endpoint::read_options),
        )
        .metered_route(
            "/changes/:id",
            delete(endpoint::change::delete).options(endpoint::read_options),
        )
        .layer(middleware);

    let routes = Router::new().nest("/api/v1", router);

    let pingz_router = Router::new().route(
        "/healthz",
        get(|| async { Response::builder().body(Body::from("pong")).unwrap() }),
    );

    let routes = routes.merge(pingz_router);

    routes.layer(svc_utils::middleware::LogLayer::new())
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        self.notify_sentry();

        let err = self.to_svc_error();

        let mut r = (self.status(), Json(err)).into_response();
        r.extensions_mut().insert(self.error_kind());

        r
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
                .remove::<service_utils::Notifications>()
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
