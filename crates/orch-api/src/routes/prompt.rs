//! Agents' open prompts, answered from the app: `POST /api/v1/attention/{id}/prompt` answers
//! the prompt of a blocked worker of the run (`POST /api/v1/lead/prompt`, in
//! [`crate::routes::lead`], answers the Lead's).
//!
//! What may be answered is narrow on purpose: only an open `blocked` attention item of this
//! run, only when its agent is a worker (or the reviewer) the run names and herdr has it
//! blocked in the pane the run recorded for it, and only with an option the pane offers at
//! that moment ([`orch_herdr::prompt`]). The keys go to that one pane. No route takes text or
//! keys, and none addresses any other agent.

use crate::api_router::ApiRouter;
use crate::doc;
use crate::error::ApiError;
use crate::models::{AttentionItem, AttentionResponse, PromptAnswerBody, PromptOption, PromptView};
use crate::routes::actions::{blocking, body};
use crate::state::AppState;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use orch_core::attention::{AttentionKind, attention_items};
use orch_core::run_store::{AgentRole, Run, RunAgent};
use orch_herdr::prompt::{self, AgentPrompt, AnswerError};
use orch_herdr::{AgentStatus, RuntimeAdapter};
use utoipa::openapi::path::{ParameterBuilder, ParameterIn, PathItem};
use utoipa::openapi::schema::Type;
use utoipa::openapi::{
    ContentBuilder, HttpMethod, ObjectBuilder, Ref, RefOr, Required, ResponseBuilder,
};

/// Error code: the agent is not blocked, so there is no prompt to answer.
pub const NO_PROMPT: &str = "no_prompt";
/// Error code: the option is not one the open prompt offers.
pub const BAD_OPTION: &str = "bad_option";

/// 409 `no_prompt`.
pub(crate) fn no_prompt(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, NO_PROMPT, message)
}

/// 400 `bad_option`: `option` is not one of `prompt`'s options (named in the message).
pub(crate) fn bad_option(option: &str, who: &str, prompt: &AgentPrompt) -> ApiError {
    let shown: String = option.chars().take(40).collect();
    let offered: Vec<String> = prompt
        .choices
        .iter()
        .map(|c| format!("`{}` ({})", c.id, c.label))
        .collect();
    ApiError::new(
        StatusCode::BAD_REQUEST,
        BAD_OPTION,
        format!(
            "`{shown}` is not one of the answers {who}'s prompt offers now, so nothing was typed; \
it offers {}. Choose one of those (the question may have changed since it was shown)",
            offered.join(", ")
        ),
    )
}

/// The run's agent `name`, when it is a worker or the reviewer (not the Lead).
fn run_worker<'a>(run: &'a Run, name: &str) -> Option<&'a RunAgent> {
    run.agents
        .iter()
        .find(|a| a.name == name && a.role != AgentRole::Lead)
}

/// The pane of `agent` while herdr has it blocked, in the pane the run recorded for it (when
/// it recorded one: herdr names are global, so another pane with that name is not ours).
fn blocked_pane(runtime: &dyn RuntimeAdapter, agent: &RunAgent) -> Option<String> {
    let a = runtime.agent(&agent.name).ok()?;
    let ours = agent.pane.as_deref().is_none_or(|p| p == a.pane_id);
    (ours && a.status == AgentStatus::Blocked).then_some(a.pane_id)
}

/// An attention item as the API shows it: a `blocked` item of a worker that herdr has blocked
/// now carries the prompt read from its pane.
pub(crate) fn attention_item(
    item: orch_core::attention::AttentionItem,
    run: &Run,
    runtime: &dyn RuntimeAdapter,
) -> AttentionItem {
    let live = (item.kind == AttentionKind::Blocked)
        .then(|| run_worker(run, item.agent.as_deref()?))
        .flatten()
        .filter(|a| blocked_pane(runtime, a).is_some())
        .map(|a| PromptView::from(&prompt::read(runtime, &a.name)));
    let mut out: AttentionItem = item.into();
    out.prompt = live;
    out
}

/// The run's attention items, with blocked workers' live prompts.
pub(crate) fn attention_response(state: &AppState) -> Result<AttentionResponse, ApiError> {
    let store = state.open_store()?;
    let run = store
        .load_run()
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::no_run)?;
    let items = attention_items(&store, Some(&run)).map_err(ApiError::internal)?;
    let herdr = state.herdr().as_ref();
    Ok(AttentionResponse {
        items: items
            .into_iter()
            .map(|i| attention_item(i, &run, herdr))
            .collect(),
    })
}

/// Answer the prompt of attention item `id`'s blocked worker with `option`.
fn answer_worker(state: &AppState, id: &str, option: &str) -> Result<AttentionResponse, ApiError> {
    let store = state.open_store()?;
    let run = store
        .load_run()
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::no_run)?;
    let items = attention_items(&store, Some(&run)).map_err(ApiError::internal)?;
    let Some(item) = items.iter().find(|i| i.id == id) else {
        return Err(ApiError::not_found(format!(
            "no open attention item {id:?} in this run"
        )));
    };
    if item.kind != AttentionKind::Blocked {
        return Err(no_prompt(format!(
            "attention item {id:?} is not a blocked agent, so it has no prompt to answer"
        )));
    }
    let name = item.agent.as_deref().unwrap_or_default();
    let Some(agent) = run_worker(&run, name) else {
        return Err(ApiError::forbidden(format!(
            "`{name}` is not a worker of this run: the app answers prompts only for this \
project's Lead (`POST /lead/prompt`) and its run's workers"
        )));
    };
    let herdr = state.herdr().as_ref();
    let Some(pane) = blocked_pane(herdr, agent) else {
        return Err(no_prompt(format!(
            "`{name}` is not waiting for an answer, so nothing was typed"
        )));
    };
    match prompt::answer(herdr, &agent.name, &pane, option) {
        Ok(_) => {}
        Err(AnswerError::NotOffered(p)) => {
            return Err(bad_option(option, &format!("`{name}`"), &p));
        }
        Err(AnswerError::JustAnswered) => {
            return Err(no_prompt(format!(
                "that answer was just typed into `{name}`'s pane: wait for it to move on"
            )));
        }
        Err(AnswerError::Herdr(e)) => {
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                crate::lead::HERDR_UNAVAILABLE,
                format!("Herdr could not type the answer into `{name}`'s pane ({e}): try again"),
            ));
        }
    }
    attention_response(state)
}

async fn answer(
    State(state): State<AppState>,
    Path(id): Path<String>,
    bytes: Bytes,
) -> Result<Json<AttentionResponse>, ApiError> {
    let b: PromptAnswerBody = body(&bytes)?;
    blocking(move || answer_worker(&state, &id, &b.option).map(Json)).await
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

/// The answer routes' refusals, shared with `POST /lead/prompt`.
pub(crate) fn answer_responses(op: &mut utoipa::openapi::path::Operation, who: &str) {
    let r = &mut op.responses.responses;
    r.insert(
        "400".into(),
        err("`bad_option`: the option is not one the open prompt offers now (the message lists them); nothing was typed. Also an invalid body: it takes only `option`"),
    );
    r.insert(
        "409".into(),
        err(&format!("`no_prompt`: {who} is not blocked, or the same answer was just typed; nothing was typed")),
    );
    r.insert(
        "503".into(),
        err("`herdr_unavailable`: Herdr could not type the answer"),
    );
}

fn attention_answer_item() -> PathItem {
    let mut op = doc::operation(
        "answerAttentionPrompt",
        "Answer a blocked worker's prompt (a `blocked` attention item) with one of the options it offers; the option's keys are typed into that worker's pane only. Answers with the attention items as they are now",
        "AttentionResponse",
        Some("PromptAnswerBody"),
    );
    op.parameters = Some(vec![
        ParameterBuilder::new()
            .name("id")
            .parameter_in(ParameterIn::Path)
            .required(Required::True)
            .schema(Some(ObjectBuilder::new().schema_type(Type::String)))
            .description(Some("The attention item's id, e.g. blocked:7"))
            .build(),
    ]);
    answer_responses(&mut op, "the agent");
    op.responses.responses.insert(
        "403".into(),
        err("The item's agent is not a worker of this run"),
    );
    doc::item(HttpMethod::Post, op)
}

/// Add the prompt schemas and the worker answer route (and its OpenAPI entry) to `api`.
pub fn register(api: ApiRouter) -> ApiRouter {
    api.schema::<PromptOption>()
        .schema::<PromptView>()
        .schema::<PromptAnswerBody>()
        .route(
            "/attention/{id}/prompt",
            post(answer),
            attention_answer_item(),
        )
}
