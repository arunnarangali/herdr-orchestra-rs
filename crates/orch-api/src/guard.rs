//! Middleware shared by every route: the error envelope (request id, panics, plain-text
//! errors turned into the JSON body), the cross-origin guard, and token auth.

use crate::api_router::STREAM_PREFIX;
use crate::error::{ApiError, REQUEST_ID, from_status};
use crate::state::AppState;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn new_request_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("req-{:x}-{n:x}", nanos as u64)
}

fn is_json(resp: &Response) -> bool {
    resp.headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
}

/// Outermost: gives the request an id, turns a panic into a 500 `internal` (the panic text
/// goes to the log only), and makes every 4xx/5xx a JSON error body.
pub(crate) async fn envelope(req: Request, next: Next) -> Response {
    let id = new_request_id();
    REQUEST_ID
        .scope(id.clone(), async move {
            let mut resp = match AssertUnwindSafe(next.run(req)).catch_unwind().await {
                Ok(r) => r,
                Err(panic) => {
                    let what = panic
                        .downcast_ref::<&str>()
                        .map(|s| (*s).to_string())
                        .or_else(|| panic.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "non-string panic".into());
                    ApiError::internal(format!("handler panicked: {what}")).into_response()
                }
            };
            let status = resp.status();
            if (status.is_client_error() || status.is_server_error()) && !is_json(&resp) {
                resp = from_status(status).into_response();
            }
            if let Ok(v) = HeaderValue::from_str(&id) {
                resp.headers_mut().insert("x-request-id", v);
            }
            resp
        })
        .await
}

/// `host[:port]` of a `Host` header or an origin, reduced to the host.
fn host_only(authority: &str) -> &str {
    match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(rest),
        None => authority.split(':').next().unwrap_or(authority),
    }
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Refuses requests a browser page could send: any `Origin` that is not this server's own
/// (there is no CORS, so no page may talk to it), and any `Host` that is not loopback (DNS
/// rebinding). Runs before auth, so a page learns nothing from the answer.
pub(crate) async fn origin(req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok());
    if let Some(h) = host
        && !is_loopback_host(host_only(h))
    {
        return ApiError::forbidden("requests must be addressed to the loopback address")
            .into_response();
    }
    if let Some(origin) = req.headers().get(header::ORIGIN) {
        let same = origin.to_str().ok().is_some_and(|o| {
            o.strip_prefix("http://")
                .is_some_and(|rest| Some(rest) == host)
        });
        if !same {
            return ApiError::forbidden("cross-origin requests are not allowed").into_response();
        }
    }
    next.run(req).await
}

fn bearer(req: &Request) -> Option<&str> {
    let v = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, rest) = v.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| rest.trim())
}

fn on_stream_route(path: &str) -> bool {
    path == STREAM_PREFIX
        || path
            .strip_prefix(STREAM_PREFIX)
            .is_some_and(|r| r.starts_with('/'))
}

fn query_token(req: &Request) -> Option<&str> {
    req.uri()
        .query()?
        .split('&')
        .find_map(|kv| kv.strip_prefix("token="))
}

/// Requires the install's token: `Authorization: Bearer <token>`; the stream routes also take
/// `?token=` (an event source cannot set headers). Unknown routes need it too.
pub(crate) async fn auth(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let presented = bearer(&req).or_else(|| {
        if on_stream_route(req.uri().path()) {
            query_token(&req)
        } else {
            None
        }
    });
    if presented.is_some_and(|t| state.token().matches(t)) {
        return next.run(req).await;
    }
    let mut resp = ApiError::unauthorized().into_response();
    resp.headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    debug_assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    resp
}
