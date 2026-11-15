//! Action endpoints: answer, dispatch, fix, approve, escalate, set status, and the run-level
//! pause, resume and stop. The API is the developer's door, not a second driver: every action
//! calls the same `orch_core::actions` function as its CLI command, which writes the request
//! (a state change, a plan edit, a run-control request) and records that it came from the API.
//! The event manager carries it out; nothing here starts it, so with no event manager running
//! the answer says the request is `waiting`.
//!
//! `POST` (under `/api/v1`): `tasks/{id}/answer`, `dispatch`, `tasks/{id}/fix`,
//! `tasks/{id}/approve`, `tasks/{id}/escalate`, `tasks/{id}/status`, `run/pause`, `run/resume`,
//! `run/stop`. A refusal changes nothing: 404 `not_found` (no such milestone), 409 `conflict`
//! (its state does not allow it, with the reason), 400 `bad_request` (missing text, unknown
//! tier or worker), 404 `no_run`.

use crate::api_router::ApiRouter;
use crate::doc;
use crate::error::ApiError;
use crate::state::AppState;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::post;
use orch_core::actions::{self, ActionError, RunControl, Source};
use orch_core::lease::{driver_alive, driver_lock_path};
use orch_core::store::Store;
use orch_core::task::{State as TaskState, Task};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa::openapi::HttpMethod;
use utoipa::openapi::path::PathItem;

/// Body of `POST /tasks/{id}/answer`.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct AnswerBody {
    /// The developer's answer; goes under the milestone's `Notes from user`.
    #[serde(default)]
    pub text: String,
}

/// Body of `POST /dispatch`.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct DispatchBody {
    /// Milestones to start (each pending, paused or failed).
    #[serde(default)]
    pub ids: Vec<String>,
    /// A worker to give them to.
    pub to: Option<String>,
    /// A minimum tier: `small`, `medium` or `high`.
    pub tier: Option<String>,
}

/// Body of `POST /tasks/{id}/fix`.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct FixBody {
    /// What should change in work that already landed.
    #[serde(default)]
    pub text: String,
    /// A worker to give the follow-up milestone to.
    pub to: Option<String>,
}

/// Body of `POST /tasks/{id}/escalate`.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct EscalateBody {
    pub reason: Option<String>,
}

/// Body of `POST /tasks/{id}/status`.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct SetStatusBody {
    /// `pending`, `paused`, `failed` or `skipped`.
    #[serde(default)]
    pub status: String,
    pub reason: Option<String>,
}

/// A milestone after the action.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskAfter {
    pub id: String,
    pub title: String,
    pub state: String,
    pub worker: Option<String>,
}

impl From<Task> for TaskAfter {
    fn from(t: Task) -> Self {
        TaskAfter {
            id: t.id,
            title: t.title,
            state: t.status.as_str().into(),
            worker: t.worker,
        }
    }
}

/// The JSON body of a request, read by hand so that every problem with it (not JSON, wrong
/// types) is the shared 400 `bad_request`, never a framework-specific status. An empty body is
/// the type's default.
fn body<T: serde::de::DeserializeOwned + Default>(bytes: &Bytes) -> Result<T, ApiError> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(T::default());
    }
    serde_json::from_slice(bytes)
        .map_err(|e| ApiError::bad_request(format!("the request body is not valid: {e}")))
}

/// What an action did.
#[derive(Debug, Serialize, ToSchema)]
pub struct ActionResponse {
    pub action: String,
    /// The affected milestones, as they are now (for `fix`: the new follow-up milestone).
    pub tasks: Vec<TaskAfter>,
    pub event_manager_running: bool,
    /// The request is recorded but no event manager is running to carry it out.
    pub waiting: bool,
    pub message: String,
}

/// What a run control did.
#[derive(Debug, Serialize, ToSchema)]
pub struct RunControlResponse {
    pub action: String,
    /// `running`, `paused`, `stopping` or `stopped`.
    pub run_control: String,
    pub event_manager_running: bool,
    pub waiting: bool,
    pub message: String,
}

fn api_error(e: ActionError) -> ApiError {
    match e {
        ActionError::NotFound(m) => ApiError::not_found(m),
        ActionError::Invalid(m) => ApiError::bad_request(m),
        ActionError::Refused(m) if m.starts_with("no run in") => ApiError::no_run(),
        ActionError::Refused(m) => ApiError::conflict(m),
        ActionError::Internal(m) => ApiError::internal(m),
    }
}

/// Run blocking work (SQLite, files) off the async threads.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(ApiError::internal)?
}

/// The store with a stored run in it; 404 `no_run` otherwise. Never creates one.
fn open(state: &AppState) -> Result<Store, ApiError> {
    let store = state.open_store()?;
    match store.load_run().map_err(ApiError::internal)? {
        Some(_) => Ok(store),
        None => Err(ApiError::no_run()),
    }
}

fn manager_running(state: &AppState) -> bool {
    driver_alive(&driver_lock_path(&state.orchestrate_dir()))
}

fn response(
    state: &AppState,
    action: &str,
    tasks: Vec<Task>,
    message: String,
) -> Json<ActionResponse> {
    let running = manager_running(state);
    Json(ActionResponse {
        action: action.into(),
        tasks: tasks.into_iter().map(Into::into).collect(),
        event_manager_running: running,
        waiting: !running,
        message,
    })
}

fn tail(running: bool) -> &'static str {
    if running {
        "the event manager carries it out"
    } else {
        "recorded; the event manager is not running — it is carried out when `orch autopilot` starts"
    }
}

async fn answer(
    State(state): State<AppState>,
    Path(id): Path<String>,
    bytes: Bytes,
) -> Result<Json<ActionResponse>, ApiError> {
    let body: AnswerBody = body(&bytes)?;
    blocking(move || {
        let mut store = open(&state)?;
        let plan = state.orchestrate_dir().join("plan.md");
        let (done, t) =
            actions::answer(&mut store, &plan, &id, &body.text, Source::Api).map_err(api_error)?;
        let msg = format!(
            "answered {id}: written to plan.md; it goes back to {}; {}",
            done.last_worker.as_deref().unwrap_or("the next free coder"),
            tail(manager_running(&state))
        );
        Ok(response(&state, "answer", vec![t], msg))
    })
    .await
}

async fn dispatch(
    State(state): State<AppState>,
    bytes: Bytes,
) -> Result<Json<ActionResponse>, ApiError> {
    let body: DispatchBody = body(&bytes)?;
    blocking(move || {
        let mut store = open(&state)?;
        let mut ids: Vec<String> = Vec::new();
        for id in body.ids {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let tasks = actions::dispatch(
            &mut store,
            &ids,
            body.to.as_deref(),
            body.tier.as_deref(),
            Source::Api,
        )
        .map_err(api_error)?;
        let msg = format!(
            "dispatched {}; {}",
            ids.join(", "),
            tail(manager_running(&state))
        );
        Ok(response(&state, "dispatch", tasks, msg))
    })
    .await
}

async fn fix(
    State(state): State<AppState>,
    Path(id): Path<String>,
    bytes: Bytes,
) -> Result<Json<ActionResponse>, ApiError> {
    let body: FixBody = body(&bytes)?;
    blocking(move || {
        let mut store = open(&state)?;
        let plan = state.orchestrate_dir().join("plan.md");
        let (f, t) = actions::fix(
            &mut store,
            &plan,
            &id,
            &body.text,
            body.to.as_deref(),
            Source::Api,
        )
        .map_err(api_error)?;
        let msg = format!(
            "added {} (\"{}\") to plan.md as a follow-up to {id}; {}",
            f.id,
            f.title,
            tail(manager_running(&state))
        );
        Ok(response(&state, "fix", vec![t], msg))
    })
    .await
}

async fn approve(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ActionResponse>, ApiError> {
    blocking(move || {
        let mut store = open(&state)?;
        let t = actions::approve(&mut store, &id, Source::Api).map_err(api_error)?;
        let msg = format!(
            "approved {id}; it lands on the next step: {}",
            tail(manager_running(&state))
        );
        Ok(response(&state, "approve", vec![t], msg))
    })
    .await
}

async fn escalate(
    State(state): State<AppState>,
    Path(id): Path<String>,
    bytes: Bytes,
) -> Result<Json<ActionResponse>, ApiError> {
    let reason = body::<EscalateBody>(&bytes)?.reason.unwrap_or_default();
    blocking(move || {
        let mut store = open(&state)?;
        match actions::escalate(&mut store, &id, &reason, Source::Api).map_err(api_error)? {
            Some((tier, t)) => {
                let msg = format!("escalated {id} to a {tier}-tier model; {}", tail(manager_running(&state)));
                Ok(response(&state, "escalate", vec![t], msg))
            }
            None => Err(ApiError::conflict(format!(
                "{id} cannot be escalated: it already had its one escalation, or no stronger model is on the team"
            ))),
        }
    })
    .await
}

async fn set_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    bytes: Bytes,
) -> Result<Json<ActionResponse>, ApiError> {
    let body: SetStatusBody = body(&bytes)?;
    blocking(move || {
        // Only what the CLI allows without `--force`: pending, paused, failed, skipped.
        let to: TaskState =
            actions::manual_status(&body.status, false).map_err(ApiError::bad_request)?;
        let mut store = open(&state)?;
        let t = actions::set_status(
            &mut store,
            &id,
            to,
            body.reason.as_deref().unwrap_or(""),
            false,
            Source::Api,
        )
        .map_err(api_error)?;
        let msg = format!("{id} is now {}", t.status);
        Ok(response(&state, "set", vec![t], msg))
    })
    .await
}

fn control(
    state: &AppState,
    want: RunControl,
    action: &str,
    what: &str,
) -> Result<Json<RunControlResponse>, ApiError> {
    let mut store = open(state)?;
    let running = manager_running(state);
    // A stop with no event manager has nothing to end: the run is already stopped.
    let stored = if want == RunControl::Stopping && !running {
        RunControl::Stopped
    } else {
        want
    };
    store
        .set_run_control(stored, Source::Api)
        .map_err(ApiError::internal)?;
    store
        .record_action(Source::Api, action, None, "")
        .map_err(ApiError::internal)?;
    Ok(Json(RunControlResponse {
        action: action.into(),
        run_control: stored.as_str().into(),
        event_manager_running: running,
        waiting: !running,
        message: format!("{what}; {}", tail(running)),
    }))
}

async fn pause(State(state): State<AppState>) -> Result<Json<RunControlResponse>, ApiError> {
    blocking(move || {
        control(
            &state,
            RunControl::Paused,
            "pause",
            "no new work starts, running milestones finish their step",
        )
    })
    .await
}

async fn resume(State(state): State<AppState>) -> Result<Json<RunControlResponse>, ApiError> {
    blocking(move || control(&state, RunControl::Running, "resume", "the run carries on")).await
}

async fn stop(State(state): State<AppState>) -> Result<Json<RunControlResponse>, ApiError> {
    blocking(move || {
        control(
            &state,
            RunControl::Stopping,
            "stop",
            "each running coder's work is saved and the event manager ends",
        )
    })
    .await
}

// ------------------------------------------------------------------------ documentation

/// A POST path item: `body` names the request schema, `ok` the response schema.
fn post_item(id: &str, summary: &str, body: Option<&str>, ok: &str) -> PathItem {
    let mut op = doc::operation(id, summary, ok, body);
    // Actions can also be refused for the milestone's state, or for a bad request.
    use utoipa::openapi::{ContentBuilder, Ref, ResponseBuilder};
    let err = |text: &str| {
        ResponseBuilder::new()
            .description(text)
            .content(
                "application/json",
                ContentBuilder::new()
                    .schema(Some(Ref::from_schema_name(doc::ERROR_SCHEMA)))
                    .build(),
            )
            .build()
    };
    op.responses
        .responses
        .insert("400".into(), err("Missing or invalid input").into());
    op.responses.responses.insert(
        "409".into(),
        err("The milestone's state does not allow it (the reason is in the message)").into(),
    );
    doc::item(HttpMethod::Post, op)
}

/// Add the action routes (and their OpenAPI entries) to `api`.
pub fn register(api: ApiRouter) -> ApiRouter {
    api.schema::<AnswerBody>()
        .schema::<DispatchBody>()
        .schema::<FixBody>()
        .schema::<EscalateBody>()
        .schema::<SetStatusBody>()
        .schema::<ActionResponse>()
        .schema::<RunControlResponse>()
        .route(
            "/tasks/{id}/answer",
            post(answer),
            post_item(
                "answer",
                "Answer a paused milestone's question",
                Some("AnswerBody"),
                "ActionResponse",
            ),
        )
        .route(
            "/dispatch",
            post(dispatch),
            post_item(
                "dispatch",
                "Start pending, paused or failed milestones",
                Some("DispatchBody"),
                "ActionResponse",
            ),
        )
        .route(
            "/tasks/{id}/fix",
            post(fix),
            post_item(
                "fix",
                "Add a follow-up milestone for work that landed",
                Some("FixBody"),
                "ActionResponse",
            ),
        )
        .route(
            "/tasks/{id}/approve",
            post(approve),
            post_item(
                "approve",
                "Let a held milestone land",
                None,
                "ActionResponse",
            ),
        )
        .route(
            "/tasks/{id}/escalate",
            post(escalate),
            post_item(
                "escalate",
                "One more go on a stronger model",
                Some("EscalateBody"),
                "ActionResponse",
            ),
        )
        .route(
            "/tasks/{id}/status",
            post(set_status),
            post_item(
                "setStatus",
                "Set a milestone to pending, paused, failed or skipped",
                Some("SetStatusBody"),
                "ActionResponse",
            ),
        )
        .route(
            "/run/pause",
            post(pause),
            post_item(
                "pauseRun",
                "Start no new work; running milestones finish their step",
                None,
                "RunControlResponse",
            ),
        )
        .route(
            "/run/resume",
            post(resume),
            post_item(
                "resumeRun",
                "Carry on after a pause",
                None,
                "RunControlResponse",
            ),
        )
        .route(
            "/run/stop",
            post(stop),
            post_item(
                "stopRun",
                "Save each running coder's work and end the event manager",
                None,
                "RunControlResponse",
            ),
        )
}
