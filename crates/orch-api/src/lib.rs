//! The local REST API behind `orch serve`: loopback only, every request carries the install's
//! token, one error body, and an OpenAPI document generated from the routes actually
//! registered. It has no logic of its own: route modules read and act through `orch-core`.
//!
//! Pages other than the server itself (the desktop app's window, a dev server) are refused
//! unless their exact origin was listed with [`serve_with`] (`orch serve --allow-origin`).
//!
//! Layout: [`routes`] holds one module per endpoint group (`read`, `actions`, `stream`, plus
//! the foundation's `system`); each exposes `register(ApiRouter) -> ApiRouter`. Auth, the
//! error envelope and the cross-origin guard live in [`guard`] and are shared by all of them.

pub mod api_router;
pub mod doc;
pub mod error;
pub mod lead;
pub mod models;
mod guard;
pub mod routes;
pub mod state;
pub mod token;

pub use api_router::{API_PREFIX, ApiRouter, RouteInfo, STREAM_PREFIX};
pub use error::{ApiError, ErrorBody};
pub use state::AppState;
pub use token::Token;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::net::TcpListener;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::openapi::{ComponentsBuilder, InfoBuilder, OpenApiBuilder};

/// The router, the OpenAPI document it serves, and the routes it registered.
pub struct Api {
    pub router: Router,
    pub openapi: utoipa::openapi::OpenApi,
    pub routes: Vec<RouteInfo>,
}

/// Build the router for `state`: every route module registered, wrapped in the error
/// envelope, the cross-origin guard and token auth (in that order, outermost first).
pub fn build(state: AppState) -> Api {
    build_with(state, |api| api)
}

/// [`build`] with extra routes registered after the standard ones (used by tests to exercise
/// the shared middleware with routes that panic, fail or stream).
pub fn build_with(state: AppState, extra: impl FnOnce(ApiRouter) -> ApiRouter) -> Api {
    let parts = extra(routes::register_all(ApiRouter::new())).into_parts();
    let mut components = ComponentsBuilder::new().security_scheme(
        "bearer",
        SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).build()),
    );
    for (name, schema) in parts.schemas {
        components = components.schema(name, schema);
    }
    let openapi = OpenApiBuilder::new()
        .info(
            InfoBuilder::new()
                .title("herdr-orchestra local API")
                .version(env!("CARGO_PKG_VERSION"))
                .build(),
        )
        .paths(parts.paths.build())
        .components(Some(components.build()))
        .build();
    let router = parts
        .router
        .fallback(|| async { ApiError::not_found("no such route") })
        .layer(axum::Extension(Arc::new(openapi.clone())))
        .layer(from_fn_with_state(state.clone(), guard::auth))
        .layer(from_fn(guard::origin))
        .layer(from_fn(guard::envelope))
        .with_state(state);
    Api {
        router,
        openapi,
        routes: parts.routes,
    }
}

/// Why a listener could not be opened.
#[derive(Debug)]
pub enum BindError {
    /// The host is not an address on this machine's loopback interface.
    NotLoopback(String),
    Io(std::io::Error),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::NotLoopback(h) => write!(
                f,
                "refusing to listen on `{h}`: the API only listens on loopback (127.0.0.1), \
                 never on a network address"
            ),
            BindError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BindError {}

/// Open a listener on a loopback `host` (`127.0.0.1`, `localhost`, `::1`); `port` 0 picks a
/// free one. Anything else is refused. This is the only way the server gets a socket.
pub async fn bind(host: &str, port: u16) -> Result<TcpListener, BindError> {
    let ip: IpAddr = if host.eq_ignore_ascii_case("localhost") {
        IpAddr::from([127, 0, 0, 1])
    } else {
        host.parse()
            .map_err(|_| BindError::NotLoopback(host.to_string()))?
    };
    if !ip.is_loopback() {
        return Err(BindError::NotLoopback(host.to_string()));
    }
    TcpListener::bind(SocketAddr::new(ip, port))
        .await
        .map_err(BindError::Io)
}

/// Serve `state` on `listener` until `shutdown` completes, then finish in-flight requests
/// and return. Long-lived responses are told to end so shutdown never waits on them.
pub async fn serve(
    listener: TcpListener,
    state: AppState,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    serve_with(listener, state, Vec::new(), shutdown).await
}

/// [`serve`], also letting pages from `allowed_origins` (exact `scheme://host[:port]`
/// strings) call the API from a browser. The token is still required; every other origin is
/// still refused.
pub async fn serve_with(
    listener: TcpListener,
    state: AppState,
    allowed_origins: Vec<String>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let addr = listener.local_addr()?;
    if !addr.ip().is_loopback() {
        return Err(std::io::Error::other("listener is not on loopback"));
    }
    let api = build(state.clone());
    axum::serve(listener, with_allowed_origins(api.router, allowed_origins))
        .with_graceful_shutdown(async move {
            shutdown.await;
            state.begin_shutdown();
        })
        .await
}

/// Wrap `router` so pages from `allowed` origins can use it: their preflight requests are
/// answered, their responses carry the cross-origin headers, and the `Origin` header is
/// removed before the cross-origin guard (which only knows the server's own origin) sees it.
/// With an empty list the router is returned unchanged.
pub fn with_allowed_origins(router: Router, allowed: Vec<String>) -> Router {
    if allowed.is_empty() {
        return router;
    }
    router.layer(from_fn_with_state(Arc::new(allowed), allow_origins))
}

async fn allow_origins(
    State(allowed): State<Arc<Vec<String>>>,
    mut req: Request,
    next: Next,
) -> Response {
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .filter(|o| o.to_str().is_ok_and(|o| allowed.iter().any(|a| a == o)))
        .cloned();
    let Some(origin) = origin else {
        return next.run(req).await;
    };
    let mut resp = if req.method() == Method::OPTIONS {
        let mut r = StatusCode::NO_CONTENT.into_response();
        let h = r.headers_mut();
        h.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, OPTIONS"),
        );
        h.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("authorization, content-type, last-event-id"),
        );
        h.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("600"),
        );
        r
    } else {
        req.headers_mut().remove(header::ORIGIN);
        next.run(req).await
    };
    let h = resp.headers_mut();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    h.insert(header::VARY, HeaderValue::from_static("Origin"));
    h.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("x-request-id"),
    );
    resp
}
