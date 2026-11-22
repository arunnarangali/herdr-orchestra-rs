//! The Lead: `GET /api/v1/lead` (where it stands), `POST /api/v1/lead/start` (start it) and
//! `POST /api/v1/lead/prompt` (answer the prompt it is blocked at, with an option it offers).
//!
//! All answer for a project with no run too. Starting happens only here, on the developer's
//! request, and only ever starts the Lead: no route starts or addresses any other agent. Every
//! change of the Lead's state is also a `lead` event on the live stream (same shape as the
//! answer), so the page does not poll. The launcher itself is [`crate::lead`].

use crate::api_router::ApiRouter;
use crate::doc;
use crate::error::ApiError;
use crate::lead::{LeadStartBody, LeadState, LeadStatus, LeadView};
use crate::models::PromptAnswerBody;
use crate::routes::actions::{blocking, body};
use crate::routes::prompt::answer_responses;
use crate::routes::stream::StreamHub;
use crate::state::AppState;
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use utoipa::openapi::path::PathItem;
use utoipa::openapi::{ContentBuilder, HttpMethod, Ref, RefOr, ResponseBuilder};

/// Make the launcher send its changes to this project's stream (and watch the pane).
fn attach_stream(state: &AppState) {
    let hub = StreamHub::get_or_create(state.project_root(), state.shutdown_signal());
    hub.set_runtime(state.herdr().clone());
    state.lead().watch(hub, state.shutdown_signal());
}

async fn show(State(state): State<AppState>) -> Result<Json<LeadView>, ApiError> {
    attach_stream(&state);
    blocking(move || Ok(Json(state.lead().view()))).await
}

async fn start(
    State(state): State<AppState>,
    bytes: Bytes,
) -> Result<(StatusCode, Json<LeadView>), ApiError> {
    let b: LeadStartBody = body(&bytes)?;
    attach_stream(&state);
    blocking(move || {
        // the Lead never sees this server's token; everything else is passed on as written
        let message = b.message.map(|m| state.token().redact(&m));
        let view = state.lead().start(&b.agent, message)?;
        Ok((StatusCode::ACCEPTED, Json(view)))
    })
    .await
}

async fn answer_prompt(
    State(state): State<AppState>,
    bytes: Bytes,
) -> Result<Json<LeadView>, ApiError> {
    let b: PromptAnswerBody = body(&bytes)?;
    attach_stream(&state);
    blocking(move || state.lead().answer_prompt(&b.option).map(Json)).await
}

// ------------------------------------------------------------------------ documentation

fn err(text: &str) -> RefOr<utoipa::openapi::Response> {
    ResponseBuilder::new()
        .description(text)
        .content(
            "application/json",
            ContentBuilder::new()
                .schema(Some(Ref::from_schema_name(doc::ERROR_SCHEMA)))
                .build(),
        )
        .build()
        .into()
}

fn lead_item() -> PathItem {
    let mut op = doc::operation(
        "lead",
        "The Lead's state: none, starting, running (with its pane's status) or gone; also without a run",
        "LeadView",
        None,
    );
    op.responses.responses.remove("404");
    doc::item(HttpMethod::Get, op)
}

fn start_item() -> PathItem {
    let mut op = doc::operation(
        "startLead",
        "Start the Lead: an installed agent kind with the lead role, in the project folder, given the planning playbook and the developer's first message; starts Herdr when it is not running",
        "LeadView",
        Some("LeadStartBody"),
    );
    // accepted: the agent comes up in the background; `lead` events on the stream follow it
    if let Some(ok) = op.responses.responses.remove("200") {
        op.responses.responses.insert("202".into(), ok);
    }
    op.responses.responses.remove("404");
    let r = &mut op.responses.responses;
    r.insert(
        "400".into(),
        err("Invalid input, or `agent_not_installed`: the agent kind is unknown or not installed; a kind without the lead role is refused too. The message names the kind"),
    );
    r.insert(
        "409".into(),
        err("`lead_running`: a Lead already runs or is starting; nothing was started"),
    );
    r.insert(
        "503".into(),
        err("`herdr_unavailable`: Herdr is not installed, or it is not running and could not be started; the message says how to fix it"),
    );
    doc::item(HttpMethod::Post, op)
}

fn prompt_item() -> PathItem {
    let mut op = doc::operation(
        "answerLeadPrompt",
        "Answer the prompt the Lead is blocked at (`prompt` in the Lead's state) with one of the options it offers; the option's keys are typed into the Lead's pane only. Answers with the Lead's state; also without a run",
        "LeadView",
        Some("PromptAnswerBody"),
    );
    op.responses.responses.remove("404");
    answer_responses(&mut op, "the Lead");
    doc::item(HttpMethod::Post, op)
}

/// Add the Lead routes (and their OpenAPI entries) to `api`.
pub fn register(api: ApiRouter) -> ApiRouter {
    api.schema::<LeadState>()
        .schema::<LeadStatus>()
        .schema::<LeadView>()
        .schema::<LeadStartBody>()
        .route("/lead", get(show), lead_item())
        .route("/lead/start", post(start), start_item())
        .route("/lead/prompt", post(answer_prompt), prompt_item())
}
