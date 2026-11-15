//! `GET /api/v1/health` and `GET /api/v1/openapi.json`.

use crate::api_router::ApiRouter;
use crate::doc;
use crate::state::AppState;
use axum::Extension;
use axum::Json;
use axum::extract::State;
use axum::routing::get;
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa::openapi::OpenApi;

/// What `health` returns.
#[derive(Debug, Serialize, ToSchema)]
pub struct Health {
    /// Always `ok` when the server answers.
    pub status: String,
    /// The `orch` version.
    pub version: String,
    /// The project (main repository root) this server serves.
    pub project: String,
    /// Whether a run exists there; the run routes answer 404 `no_run` when it does not.
    pub run: bool,
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        project: state.project_root().display().to_string(),
        run: state.has_run(),
    })
}

async fn openapi(Extension(doc): Extension<Arc<OpenApi>>) -> Json<OpenApi> {
    Json((*doc).clone())
}

pub(crate) fn register(api: ApiRouter) -> ApiRouter {
    api.schema::<Health>()
        .schema::<crate::error::ErrorBody>()
        .route(
            "/openapi.json",
            get(openapi),
            doc::get("openapi", "This OpenAPI 3 document", "OpenApi"),
        )
        .route(
            "/health",
            get(health),
            doc::get(
                "health",
                "Server version and the project it serves",
                "Health",
            ),
        )
}
