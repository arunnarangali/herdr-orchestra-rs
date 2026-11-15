//! [`ApiRouter`]: the router route modules register into. A route and its OpenAPI entry are
//! added in one call, so a route cannot exist without being documented.

use crate::state::AppState;
use axum::Router;
use axum::routing::MethodRouter;
use utoipa::ToSchema;
use utoipa::openapi::path::{HttpMethod, PathItem, PathsBuilder};
use utoipa::openapi::{RefOr, Schema};

/// URL prefix of every route.
pub const API_PREFIX: &str = "/api/v1";
/// Routes under this prefix (the live stream) also accept the token as `?token=`, because a
/// browser's event source cannot set headers. No other route does.
pub const STREAM_PREFIX: &str = "/api/v1/stream";

/// One registered route, as documented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteInfo {
    /// Full path, e.g. `/api/v1/health`.
    pub path: String,
    /// Upper-case methods, e.g. `GET`.
    pub methods: Vec<String>,
}

pub struct ApiRouter {
    router: Router<AppState>,
    paths: PathsBuilder,
    routes: Vec<RouteInfo>,
    schemas: Vec<(String, RefOr<Schema>)>,
}

fn method_name(m: &HttpMethod) -> &'static str {
    match m {
        HttpMethod::Get => "GET",
        HttpMethod::Put => "PUT",
        HttpMethod::Post => "POST",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Options => "OPTIONS",
        HttpMethod::Head => "HEAD",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Trace => "TRACE",
    }
}

impl Default for ApiRouter {
    fn default() -> ApiRouter {
        ApiRouter::new()
    }
}

impl ApiRouter {
    pub fn new() -> ApiRouter {
        ApiRouter {
            router: Router::new(),
            paths: PathsBuilder::new(),
            routes: Vec::new(),
            schemas: Vec::new(),
        }
    }

    /// Serve `handler` at `API_PREFIX + path` (axum path syntax, `{id}` for parameters) and
    /// document it with `doc`. `doc` must describe exactly the methods `handler` answers;
    /// registering a path twice or with no documented operation panics at start-up, so a
    /// wiring mistake never reaches a user.
    pub fn route(
        mut self,
        path: &str,
        handler: MethodRouter<AppState>,
        doc: PathItem,
    ) -> ApiRouter {
        assert!(
            path.starts_with('/'),
            "route path must start with '/': {path}"
        );
        let full = format!("{API_PREFIX}{path}");
        let mut methods: Vec<String> = [
            &doc.get,
            &doc.put,
            &doc.post,
            &doc.delete,
            &doc.options,
            &doc.head,
            &doc.patch,
            &doc.trace,
        ]
        .into_iter()
        .zip([
            HttpMethod::Get,
            HttpMethod::Put,
            HttpMethod::Post,
            HttpMethod::Delete,
            HttpMethod::Options,
            HttpMethod::Head,
            HttpMethod::Patch,
            HttpMethod::Trace,
        ])
        .filter(|(op, _)| op.is_some())
        .map(|(_, m)| method_name(&m).to_string())
        .collect();
        methods.sort();
        assert!(
            !methods.is_empty(),
            "route {full} has no documented operation"
        );
        assert!(
            !self.routes.iter().any(|r| r.path == full),
            "route {full} registered twice"
        );
        self.router = self.router.route(&full, handler);
        self.paths = self.paths.path(full.clone(), doc);
        self.routes.push(RouteInfo {
            path: full,
            methods,
        });
        self
    }

    /// Add `T` (and the schemas it refers to) to the OpenAPI `components`, so operations can
    /// point at it by name (`T`'s type name, or its `#[schema(as = ..)]`).
    pub fn schema<T: ToSchema>(mut self) -> ApiRouter {
        self.schemas.push((T::name().into_owned(), T::schema()));
        T::schemas(&mut self.schemas);
        self
    }

    pub(crate) fn into_parts(self) -> Parts {
        Parts {
            router: self.router,
            paths: self.paths,
            routes: self.routes,
            schemas: self.schemas,
        }
    }
}

pub(crate) struct Parts {
    pub router: Router<AppState>,
    pub paths: PathsBuilder,
    pub routes: Vec<RouteInfo>,
    pub schemas: Vec<(String, RefOr<Schema>)>,
}
