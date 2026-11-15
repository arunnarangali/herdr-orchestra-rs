//! The developer's chat with the lead: `GET /api/v1/chat` and `POST /api/v1/chat`.
//!
//! `POST` only stores the message (as the developer's, `queued`): it never types into a pane.
//! The event manager delivers it to the lead through the same checks as a wake-up and records
//! `sent` or `failed` on it; the lead answers with `orch say`. Every stored message and every
//! change of its delivery state is also a `chat` event on the live stream. There is no route
//! that addresses any agent but the lead.
//!
//! When no lead can receive the message the answer is 409 `lead_unavailable`, with how to start
//! one; the message is stored anyway and is delivered once there is a lead.

use crate::api_router::ApiRouter;
use crate::doc;
use crate::error::ApiError;
use crate::routes::actions::{blocking, body, manager_running};
use crate::state::AppState;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use orch_core::chat::{ChatError, ChatKind, ChatMessage, FeatureRequest};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa::openapi::path::{ParameterBuilder, ParameterIn, PathItem};
use utoipa::openapi::{
    ContentBuilder, HttpMethod, ObjectBuilder, Ref, RefOr, Required, ResponseBuilder, Type,
};

/// Most messages one `GET /chat` answers.
const LIST_MAX: usize = 1000;
/// The error code when no lead pane can receive the message.
pub const LEAD_UNAVAILABLE: &str = "lead_unavailable";

/// A feature request's fields, as the plan's request block has them. Only `feature` is needed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ChatRequest {
    #[serde(default)]
    pub feature: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub must: String,
    #[serde(default)]
    pub must_not: String,
    #[serde(default)]
    pub decided: String,
    #[serde(default)]
    pub done_when: String,
    #[serde(default)]
    pub mode: String,
}

impl From<FeatureRequest> for ChatRequest {
    fn from(r: FeatureRequest) -> Self {
        ChatRequest {
            feature: r.feature,
            why: r.why,
            must: r.must,
            must_not: r.must_not,
            decided: r.decided,
            done_when: r.done_when,
            mode: r.mode,
        }
    }
}

/// Body of `POST /chat`.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct ChatBody {
    /// `message`, `feature_request` or `plan_approval`.
    #[serde(default)]
    pub kind: String,
    /// What the developer says. May be empty for a feature request (its feature line is used)
    /// and for a plan approval ("go").
    #[serde(default)]
    pub text: String,
    /// The feature request's fields; required for `feature_request`, refused for the others.
    #[serde(default)]
    pub request: Option<ChatRequest>,
}

/// One message of the conversation (also the payload of the stream's `chat` event).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChatMessageView {
    pub id: i64,
    /// When it was stored, RFC 3339 UTC.
    pub ts: String,
    /// `developer` or `lead`.
    pub sender: String,
    /// `message`, `feature_request` or `plan_approval`.
    pub kind: String,
    pub text: String,
    pub request: Option<ChatRequest>,
    /// The milestone a lead reply is about.
    pub about: Option<String>,
    /// For the developer's messages: `queued`, `sent`, `acknowledged` or `failed`; the lead's
    /// messages have none.
    pub delivery: Option<String>,
    /// Why it waits, or what to do when it failed.
    pub delivery_note: Option<String>,
}

impl From<ChatMessage> for ChatMessageView {
    fn from(m: ChatMessage) -> Self {
        ChatMessageView {
            id: m.id,
            ts: orch_core::chat::rfc3339(m.ts),
            sender: m.sender.as_str().into(),
            kind: m.kind.as_str().into(),
            text: m.text,
            request: m.request.map(Into::into),
            about: m.about,
            delivery: m.delivery.map(|d| d.as_str().into()),
            delivery_note: m.delivery_note,
        }
    }
}

/// Answer of `GET /chat`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatListResponse {
    /// The conversation in order (oldest first).
    pub messages: Vec<ChatMessageView>,
}

#[derive(Debug, Deserialize)]
struct ChatQuery {
    after: Option<i64>,
}

fn open(state: &AppState) -> Result<orch_core::store::Store, ApiError> {
    let store = state.open_store()?;
    match store.load_run().map_err(ApiError::internal)? {
        Some(_) => Ok(store),
        None => Err(ApiError::no_run()),
    }
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ChatQuery>,
) -> Result<Json<ChatListResponse>, ApiError> {
    blocking(move || {
        let store = open(&state)?;
        let after = q.after.map(|a| a.max(0));
        let messages = store
            .chat_messages(after, LIST_MAX)
            .map_err(ApiError::internal)?;
        Ok(Json(ChatListResponse {
            messages: messages.into_iter().map(Into::into).collect(),
        }))
    })
    .await
}

async fn create(
    State(state): State<AppState>,
    bytes: Bytes,
) -> Result<(StatusCode, Json<ChatMessageView>), ApiError> {
    let b: ChatBody = body(&bytes)?;
    blocking(move || {
        let kind = ChatKind::parse(&b.kind).ok_or_else(|| {
            ApiError::bad_request("`kind` must be message, feature_request or plan_approval")
        })?;
        // what is stored never carries this server's token (the core masks other secrets)
        let token = state.token();
        let text = token.redact(&b.text);
        let request = b.request.map(|r| FeatureRequest {
            feature: token.redact(&r.feature),
            why: token.redact(&r.why),
            must: token.redact(&r.must),
            must_not: token.redact(&r.must_not),
            decided: token.redact(&r.decided),
            done_when: token.redact(&r.done_when),
            mode: token.redact(&r.mode),
        });
        let mut store = open(&state)?;
        let m = store
            .post_developer_message(kind, &text, request.as_ref())
            .map_err(|e| match e {
                ChatError::Invalid(m) => ApiError::bad_request(m),
                ChatError::Store(e) => ApiError::internal(e),
            })?;
        // stored first; now say whether anyone can take it
        if let Some(problem) = store
            .lead_problem(manager_running(&state))
            .map_err(ApiError::internal)?
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                LEAD_UNAVAILABLE,
                format!("{} (message #{} is stored)", problem.how_to_fix(), m.id),
            ));
        }
        Ok((StatusCode::CREATED, Json(m.into())))
    })
    .await
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

fn chat_item() -> PathItem {
    let mut list = doc::operation(
        "chat",
        "The conversation with the lead, in order; `after` returns only newer messages",
        "ChatListResponse",
        None,
    );
    list.parameters = Some(vec![
        ParameterBuilder::new()
            .name("after")
            .parameter_in(ParameterIn::Query)
            .required(Required::False)
            .description(Some("Only messages with a larger id"))
            .schema(Some(ObjectBuilder::new().schema_type(Type::Integer)))
            .build(),
    ]);
    let mut send = doc::operation(
        "sendChat",
        "Send the lead a message, a feature request or the plan approval (stored; the event manager delivers it)",
        "ChatMessageView",
        Some("ChatBody"),
    );
    // the message is created, not just fetched
    if let Some(ok) = send.responses.responses.remove("200") {
        send.responses.responses.insert("201".into(), ok);
    }
    send.responses
        .responses
        .insert("400".into(), err("Missing or invalid input"));
    send.responses.responses.insert(
        "409".into(),
        err("`lead_unavailable`: no lead pane can receive it; the message is stored and the message says how to start the lead"),
    );
    let mut item = doc::item(HttpMethod::Get, list);
    item.post = Some(send);
    item
}

/// Add the chat routes (and their OpenAPI entries) to `api`.
pub fn register(api: ApiRouter) -> ApiRouter {
    api.schema::<ChatRequest>()
        .schema::<ChatBody>()
        .schema::<ChatMessageView>()
        .schema::<ChatListResponse>()
        .route("/chat", get(list).post(create), chat_item())
}
