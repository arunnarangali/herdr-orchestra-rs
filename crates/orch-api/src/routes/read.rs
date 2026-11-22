//! Read endpoints: everything the Overview, Workflows, Execution Monitor, Agents and Reports
//! screens show. They only look at the run: nothing here writes a file, marks an event read or
//! starts anything. Every answer is built from a typed view in `orch_core::view` (the same
//! code `orch status`, `orch report` and `orch events` print from) and shaped by
//! [`crate::models`].
//!
//! `GET` (all under `/api/v1`): `status`, `plan`, `tasks`, `tasks/{id}`, `events`, `report`,
//! `agents`, `models`, `logs`. A project without a run answers 404 `no_run` on the run's routes
//! (`agents` and `models` read the user's configuration and always answer).

use crate::api_router::ApiRouter;
use crate::doc;
use crate::error::ApiError;
use crate::models::{
    AgentOutputResponse, AgentsResponse, AttentionResponse, EventsResponse, LogsResponse,
    ModelsResponse, PlanResponse, ReportResponse, StatusResponse, TaskDetailResponse, TasksResponse,
};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use orch_core::run_store::Run;
use orch_core::store::Store;
use orch_core::view;
use serde::Deserialize;
use utoipa::openapi::path::{Parameter, ParameterBuilder, ParameterIn, PathItem};
use utoipa::openapi::{ObjectBuilder, Required, Type};

/// Default and largest `lines` of `GET /logs`.
const LOG_LINES_DEFAULT: usize = 200;
const LOG_LINES_MAX: usize = 5000;
/// Largest `limit` of `GET /events`.
const EVENTS_LIMIT_MAX: usize = 1000;

/// Run blocking work (SQLite, files) off the async threads.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(ApiError::internal)?
}

/// The store and the run in it; 404 `no_run` when there is none. Never creates one.
fn load(state: &AppState) -> Result<(Store, Run), ApiError> {
    let store = state.open_store()?;
    match store.load_run().map_err(ApiError::internal)? {
        Some(run) => Ok((store, run)),
        None => Err(ApiError::no_run()),
    }
}

async fn status(State(state): State<AppState>) -> Result<Json<StatusResponse>, ApiError> {
    blocking(move || {
        let (store, run) = load(&state)?;
        let v = view::status_view(&store, &run, &state.orchestrate_dir())
            .map_err(ApiError::internal)?;
        Ok(Json(v.into()))
    })
    .await
}

async fn plan(State(state): State<AppState>) -> Result<Json<PlanResponse>, ApiError> {
    blocking(move || {
        load(&state)?;
        let path = state.orchestrate_dir().join("plan.md");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(ApiError::not_found("the run has no plan.md"));
            }
            Err(e) => return Err(ApiError::internal(e)),
        };
        let plan = orch_core::plan::parse_plan(&text).map_err(|e| {
            ApiError::conflict(format!(
                "plan.md is not valid: line {}: {}",
                e.line, e.message
            ))
        })?;
        Ok(Json(view::plan_view(&plan).into()))
    })
    .await
}

async fn tasks(State(state): State<AppState>) -> Result<Json<TasksResponse>, ApiError> {
    blocking(move || {
        let (store, run) = load(&state)?;
        let rows = view::task_rows(&store, &run).map_err(ApiError::internal)?;
        Ok(Json(TasksResponse {
            tasks: rows.into_iter().map(Into::into).collect(),
        }))
    })
    .await
}

async fn task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TaskDetailResponse>, ApiError> {
    blocking(move || {
        let (store, run) = load(&state)?;
        let detail = view::task_detail(&store, &run, state.project_root(), &id)
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::not_found(format!("no milestone {id:?} in this run")))?;
        Ok(Json(detail.into()))
    })
    .await
}

#[derive(Debug, Deserialize)]
struct EventsQuery {
    since: Option<i64>,
    limit: Option<usize>,
}

async fn events(
    State(state): State<AppState>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<EventsResponse>, ApiError> {
    if q.limit.is_some_and(|l| l == 0 || l > EVENTS_LIMIT_MAX) {
        return Err(ApiError::bad_request(format!(
            "limit must be 1 to {EVENTS_LIMIT_MAX}"
        )));
    }
    blocking(move || {
        let (store, run) = load(&state)?;
        let v = view::events_view(&store, &run, q.since, q.limit).map_err(ApiError::internal)?;
        Ok(Json(v.into()))
    })
    .await
}

async fn report(State(state): State<AppState>) -> Result<Json<ReportResponse>, ApiError> {
    blocking(move || {
        let (store, run) = load(&state)?;
        let v = view::report_view(&store, &run).map_err(ApiError::internal)?;
        // Tiers come from the catalog; a catalog that cannot load only leaves them out.
        let catalog = orch_config::models::load(None).ok();
        Ok(Json(crate::models::report_response(v, catalog.as_ref())))
    })
    .await
}

async fn agents(State(state): State<AppState>) -> Result<Json<AgentsResponse>, ApiError> {
    blocking(move || {
        let reg = orch_config::agents::load(None)
            .map_err(|e| ApiError::internal(format!("agents registry: {e}")))?;
        let (run, tasks) = if state.has_run() {
            if let Ok((store, r)) = load(&state) {
                let tasks = store.list_tasks().ok();
                (Some(r), tasks)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };
        Ok(Json(crate::models::agents_response(
            &reg,
            run.as_ref(),
            tasks.as_deref(),
        )))
    })
    .await
}

async fn attention(State(state): State<AppState>) -> Result<Json<AttentionResponse>, ApiError> {
    // a blocked worker's item carries the prompt read from its pane now
    blocking(move || crate::routes::prompt::attention_response(&state).map(Json)).await
}

/// Default and largest `lines` of `GET /agents/{name}/output`.
const AGENT_OUTPUT_LINES_DEFAULT: usize = 200;
const AGENT_OUTPUT_LINES_MAX: usize = 5000;

#[derive(Debug, Deserialize)]
struct AgentOutputQuery {
    lines: Option<usize>,
}

async fn agent_output(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(q): Query<AgentOutputQuery>,
) -> Result<Json<AgentOutputResponse>, ApiError> {
    let lines = q.lines.unwrap_or(AGENT_OUTPUT_LINES_DEFAULT);
    if lines == 0 || lines > AGENT_OUTPUT_LINES_MAX {
        return Err(ApiError::bad_request(format!(
            "lines must be 1 to {AGENT_OUTPUT_LINES_MAX}"
        )));
    }
    if name.is_empty() || name.len() > 100 {
        return Err(ApiError::bad_request("name must be 1 to 100 characters"));
    }
    blocking(move || {
        let (_, run) = load(&state)?;
        if !run.agents.iter().any(|a| a.name == name) {
            return Err(ApiError::new(
                axum::http::StatusCode::NOT_FOUND,
                "agent_not_found",
                format!("no agent named {name:?} in this run"),
            ));
        }

        let raw = state.herdr().read_output(&name, lines as u32).map_err(|e| {
            match e {
                orch_herdr::HerdrError::AgentNotFound { .. } => ApiError::new(
                    axum::http::StatusCode::NOT_FOUND,
                    "agent_not_found",
                    format!("agent {name:?} not found in herdr"),
                ),
                other => ApiError::new(
                    axum::http::StatusCode::BAD_GATEWAY,
                    "herdr_read_failed",
                    format!("cannot read output for agent {name:?}: {other}"),
                ),
            }
        })?;

        let text = orch_core::attention::strip_escapes(&raw);
        let ts = orch_core::attention::format_rfc3339(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        );
        let count = if text.is_empty() { 0 } else { text.lines().count() };

        Ok(Json(AgentOutputResponse {
            agent: name,
            lines: count,
            text,
            ts,
        }))
    })
    .await
}

async fn models(State(_): State<AppState>) -> Result<Json<ModelsResponse>, ApiError> {
    blocking(move || {
        let cat = orch_config::models::load(None)
            .map_err(|e| ApiError::internal(format!("model catalog: {e}")))?;
        Ok(Json(crate::models::models_response(&cat)))
    })
    .await
}

#[derive(Debug, Deserialize)]
struct LogsQuery {
    lines: Option<usize>,
    milestone: Option<String>,
    agent: Option<String>,
}

async fn logs(
    State(state): State<AppState>,
    Query(q): Query<LogsQuery>,
) -> Result<Json<LogsResponse>, ApiError> {
    let lines = q.lines.unwrap_or(LOG_LINES_DEFAULT);
    if lines == 0 || lines > LOG_LINES_MAX {
        return Err(ApiError::bad_request(format!(
            "lines must be 1 to {LOG_LINES_MAX}"
        )));
    }
    for (name, v) in [("milestone", &q.milestone), ("agent", &q.agent)] {
        if v.as_deref().is_some_and(|v| v.is_empty() || v.len() > 100) {
            return Err(ApiError::bad_request(format!(
                "{name} must be 1 to 100 characters"
            )));
        }
    }
    blocking(move || {
        if !state.orchestrate_dir().is_dir() {
            return Err(ApiError::no_run());
        }
        let lines = view::log_tail(
            &state.orchestrate_dir(),
            lines,
            q.milestone.as_deref(),
            q.agent.as_deref(),
        )
        .map_err(ApiError::internal)?;
        Ok(Json(LogsResponse { lines }))
    })
    .await
}

// ------------------------------------------------------------------------ documentation

fn param(name: &str, location: ParameterIn, required: bool, kind: Type, text: &str) -> Parameter {
    ParameterBuilder::new()
        .name(name)
        .parameter_in(location)
        .required(if required {
            Required::True
        } else {
            Required::False
        })
        .description(Some(text))
        .schema(Some(ObjectBuilder::new().schema_type(kind)))
        .build()
}

/// `item`'s GET operation with `params` documented.
fn with_params(mut item: PathItem, params: Vec<Parameter>) -> PathItem {
    if let Some(op) = item.get.as_mut() {
        op.parameters = Some(params);
    }
    item
}

/// Add the read routes (and their OpenAPI entries) to `api`.
pub fn register(api: ApiRouter) -> ApiRouter {
    use crate::models::*;
    api.schema::<StatusResponse>()
        .schema::<PlanResponse>()
        .schema::<TasksResponse>()
        .schema::<TaskDetailResponse>()
        .schema::<EventsResponse>()
        .schema::<ReportResponse>()
        .schema::<AgentsResponse>()
        .schema::<ModelsResponse>()
        .schema::<LogsResponse>()
        .schema::<AttentionResponse>()
        .schema::<AgentOutputResponse>()
        .route(
            "/status",
            get(status),
            doc::get("status", "Run state, progress, workers and the lead's event counts", "StatusResponse"),
        )
        .route("/plan", get(plan), doc::get("plan", "The plan: header, feature breakdown and every milestone", "PlanResponse"))
        .route("/tasks", get(tasks), doc::get("tasks", "Every milestone with its state, worker, model and counters", "TasksResponse"))
        .route(
            "/tasks/{id}",
            get(task),
            with_params(
                doc::get("task", "One milestone inspected: history, checks, reviews, the coder's results", "TaskDetailResponse"),
                vec![param("id", ParameterIn::Path, true, Type::String, "Milestone id, e.g. M1")],
            ),
        )
        .route(
            "/events",
            get(events),
            with_params(
                doc::get("events", "The lead's queue: new, still open and ready for dispatch (does not mark anything read)", "EventsResponse"),
                vec![
                    param("since", ParameterIn::Query, false, Type::Integer, "Only events with a larger id"),
                    param("limit", ParameterIn::Query, false, Type::Integer, "At most this many new and this many still-open events (1 to 1000)"),
                ],
            ),
        )
        .route("/report", get(report), doc::get("report", "Times, rounds, first-pass rate, wasted work, per-model table", "ReportResponse"))
        .route("/agents", get(agents), doc::get("agents", "Installed agent kinds, roles, herdr detection, skills and login status", "AgentsResponse"))
        .route("/models", get(models), doc::get("models", "The model catalog: tiers, ratings and what the policy denies", "ModelsResponse"))
        .route(
            "/logs",
            get(logs),
            with_params(
                doc::get("logs", "The tail of the event manager's log, as plain text", "LogsResponse"),
                vec![
                    param("lines", ParameterIn::Query, false, Type::Integer, "How many lines (1 to 5000, default 200)"),
                    param("milestone", ParameterIn::Query, false, Type::String, "Only lines naming this milestone"),
                    param("agent", ParameterIn::Query, false, Type::String, "Only lines naming this agent"),
                ],
            ),
        )
        .route(
            "/attention",
            get(attention),
            doc::get(
                "attention",
                "Open items that need the developer: blocked agents (with the prompt in a blocked worker's pane), questions, held/failed milestones, wake-ups, plan approval",
                "AttentionResponse",
            ),
        )
        .route(
            "/agents/{name}/output",
            get(agent_output),
            with_params(
                doc::get(
                    "agent_output",
                    "Recent output of one agent as plain text with ANSI escapes removed",
                    "AgentOutputResponse",
                ),
                vec![
                    param("name", ParameterIn::Path, true, Type::String, "Agent name on the team, e.g. coder-1"),
                    param("lines", ParameterIn::Query, false, Type::Integer, "How many recent lines (1 to 5000, default 200)"),
                ],
            ),
        )
}
