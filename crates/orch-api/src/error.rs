//! The one error body every API response uses:
//! `{"error": {"code": "<snake_case>", "message": "<text>", "request_id": "<id>"}}`.
//!
//! Handlers return [`ApiError`]; the request id is filled in from the request being served.
//! Internal errors keep their cause in the log and send only a generic message.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

tokio::task_local! {
    /// The id of the request being served (set by the envelope middleware).
    pub(crate) static REQUEST_ID: String;
}

/// The request id of the request being served, or `-` outside one.
pub fn current_request_id() -> String {
    REQUEST_ID
        .try_with(Clone::clone)
        .unwrap_or_else(|_| "-".into())
}

/// The `error` object.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorDetail {
    /// Stable machine-readable code, e.g. `no_run`, `not_found`, `unauthorized`.
    pub code: String,
    /// Human-readable text. Never contains the token or internals.
    pub message: String,
    /// Id of the request this answers (also the `x-request-id` response header).
    pub request_id: String,
}

/// The response body of every error.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

/// An error a handler returns.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError {
            status,
            code,
            message: message.into(),
        }
    }
    /// 404 `no_run`: the folder has no `.orchestrate/` run.
    pub fn no_run() -> ApiError {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "no_run",
            "no run exists in this project: write .orchestrate/plan.md and run `orch sync`",
        )
    }
    /// 404 `not_found`.
    pub fn not_found(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not_found", message)
    }
    /// 400 `bad_request`.
    pub fn bad_request(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }
    /// 409 `conflict`.
    pub fn conflict(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::CONFLICT, "conflict", message)
    }
    /// 401 `unauthorized`.
    pub fn unauthorized() -> ApiError {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "missing or wrong API token: send `Authorization: Bearer <token>`",
        )
    }
    /// 403 `forbidden`.
    pub fn forbidden(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::FORBIDDEN, "forbidden", message)
    }
    /// 500 `internal`. `cause` goes to the log only; the client gets a generic message.
    pub fn internal(cause: impl std::fmt::Display) -> ApiError {
        tracing::error!(request_id = %current_request_id(), %cause, "internal error");
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "internal error",
        )
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }
    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorDetail {
                code: self.code.to_string(),
                message: self.message,
                request_id: current_request_id(),
            },
        };
        (self.status, Json(body)).into_response()
    }
}

/// The error for a response a layer or extractor produced without our body (405, a rejected
/// JSON body, …): the code follows the status, the message is generic.
pub(crate) fn from_status(status: StatusCode) -> ApiError {
    let code = match status.as_u16() {
        401 => "unauthorized",
        403 => "forbidden",
        404 => "not_found",
        405 => "method_not_allowed",
        409 => "conflict",
        413 => "payload_too_large",
        415 => "unsupported_media_type",
        s if s >= 500 => "internal",
        _ => "bad_request",
    };
    let message = if status.is_server_error() {
        "internal error".to_string()
    } else {
        status
            .canonical_reason()
            .unwrap_or("request refused")
            .to_lowercase()
    };
    ApiError::new(status, code, message)
}
