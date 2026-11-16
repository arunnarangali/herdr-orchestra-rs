//! The live event stream. Registered by M4. Mounted under `/api/v1/stream`
//! ([`crate::api_router::STREAM_PREFIX`]): only there does the server also accept the token
//! as a `token` query parameter. Streams end when `AppState::shutdown_signal` turns
//! true, so graceful shutdown does not wait on them.

#[path = "../feed.rs"]
pub mod feed;

pub use feed::*;

use crate::api_router::ApiRouter;
use crate::doc;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::StreamExt;
use serde::Deserialize;
use utoipa::openapi::path::{OperationBuilder, ParameterBuilder, ParameterIn};
use utoipa::openapi::schema::Type;
use utoipa::openapi::{
    ContentBuilder, HttpMethod, ObjectBuilder, Ref, ResponseBuilder, ResponsesBuilder,
};

#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    pub token: Option<String>,
    pub since: Option<u64>,
}

/// Handler for `GET /api/v1/stream`.
pub async fn stream_handler(
    State(state): State<AppState>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Response {
    let since = query.since.or_else(|| {
        headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
    });

    let hub = StreamHub::get_or_create(state.project_root(), state.shutdown_signal());
    match hub.subscribe(since) {
        Ok((rx, guard)) => {
            let keepalive_dur = feed::keepalive_interval();
            let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|msg| {
                Ok::<_, std::convert::Infallible>(
                    axum::response::sse::Event::default()
                        .id(msg.id.to_string())
                        .event(msg.event)
                        .data(serde_json::to_string(&msg.data).unwrap_or_default()),
                )
            });
            let guarded = feed::GuardedStream::new(stream, guard, state.shutdown_signal());
            axum::response::sse::Sse::new(guarded)
                .keep_alive(
                    axum::response::sse::KeepAlive::new()
                        .interval(keepalive_dur)
                        .text("keep-alive"),
                )
                .into_response()
        }
        Err(err) => err.into_response(),
    }
}

/// Add the stream routes (and their OpenAPI entries) to `api`.
pub fn register(api: ApiRouter) -> ApiRouter {
    let responses = ResponsesBuilder::new()
        .response(
            "200",
            ResponseBuilder::new()
                .description("Live event stream (Server-Sent Events)")
                .content(
                    "text/event-stream",
                    ContentBuilder::new()
                        .schema(Some(Ref::from_schema_name("StreamMessage")))
                        .build(),
                )
                .build(),
        )
        .response(
            "401",
            ResponseBuilder::new()
                .description("Missing or wrong token")
                .content(
                    "application/json",
                    ContentBuilder::new()
                        .schema(Some(Ref::from_schema_name(doc::ERROR_SCHEMA)))
                        .build(),
                )
                .build(),
        )
        .response(
            "429",
            ResponseBuilder::new()
                .description("Too many concurrent stream clients")
                .content(
                    "application/json",
                    ContentBuilder::new()
                        .schema(Some(Ref::from_schema_name(doc::ERROR_SCHEMA)))
                        .build(),
                )
                .build(),
        )
        .response(
            "500",
            ResponseBuilder::new()
                .description("Internal error")
                .content(
                    "application/json",
                    ContentBuilder::new()
                        .schema(Some(Ref::from_schema_name(doc::ERROR_SCHEMA)))
                        .build(),
                )
                .build(),
        )
        .build();

    let op = OperationBuilder::new()
        .operation_id(Some("streamEvents"))
        .summary(Some("Live event stream (Server-Sent Events)"))
        .responses(responses)
        .security(utoipa::openapi::security::SecurityRequirement::new::<_, _, &str>("bearer", []))
        .parameter(
            ParameterBuilder::new()
                .name("since")
                .parameter_in(ParameterIn::Query)
                .description(Some("Receive events after this event ID (reconnect)"))
                .required(utoipa::openapi::Required::False)
                .schema(Some(
                    ObjectBuilder::new()
                        .schema_type(Type::Integer)
                        .build(),
                ))
                .build(),
        )
        .parameter(
            ParameterBuilder::new()
                .name("token")
                .parameter_in(ParameterIn::Query)
                .description(Some(
                    "API token (alternative to Authorization header for EventSource)",
                ))
                .required(utoipa::openapi::Required::False)
                .schema(Some(
                    ObjectBuilder::new()
                        .schema_type(Type::String)
                        .build(),
                ))
                .build(),
        )
        .build();

    let path_item = doc::item(HttpMethod::Get, op);

    api.schema::<StreamMessage>()
        .schema::<TaskStateEvent>()
        .schema::<LeadQueueEvent>()
        .schema::<RunStateEvent>()
        .schema::<LogEvent>()
        .schema::<AttentionEvent>()
        .schema::<AttentionOp>()
        .route("/stream", get(stream_handler), path_item)
}
