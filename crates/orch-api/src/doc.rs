//! Small builders for OpenAPI path items, shared by every route module so entries look alike
//! and always list the standard error responses.

use utoipa::openapi::path::{Operation, OperationBuilder, PathItem};
use utoipa::openapi::request_body::RequestBodyBuilder;
use utoipa::openapi::{ContentBuilder, HttpMethod, Ref, RefOr, ResponseBuilder, ResponsesBuilder};

/// Name of the shared error body schema in `components.schemas`.
pub const ERROR_SCHEMA: &str = "ErrorBody";

fn json_content(schema: &str) -> utoipa::openapi::Content {
    ContentBuilder::new()
        .schema(Some(Ref::from_schema_name(schema)))
        .build()
}

fn error_response(description: &str) -> RefOr<utoipa::openapi::Response> {
    ResponseBuilder::new()
        .description(description)
        .content("application/json", json_content(ERROR_SCHEMA))
        .build()
        .into()
}

/// A JSON operation: `ok_schema` (a name in `components.schemas`) for 200, plus the shared
/// error responses. `request_schema` adds a JSON request body.
pub fn operation(
    id: &str,
    summary: &str,
    ok_schema: &str,
    request_schema: Option<&str>,
) -> Operation {
    let responses = ResponsesBuilder::new()
        .response(
            "200",
            ResponseBuilder::new()
                .description("Success")
                .content("application/json", json_content(ok_schema)),
        )
        .response("401", error_response("Missing or wrong token"))
        .response("404", error_response("No run, or nothing with that id"))
        .response("500", error_response("Internal error"))
        .build();
    let mut op = OperationBuilder::new()
        .operation_id(Some(id))
        .summary(Some(summary))
        .responses(responses)
        .security(utoipa::openapi::security::SecurityRequirement::new::<
            _,
            _,
            &str,
        >("bearer", []));
    if let Some(req) = request_schema {
        op = op.request_body(Some(
            RequestBodyBuilder::new()
                .content("application/json", json_content(req))
                .build(),
        ));
    }
    op.build()
}

/// A path item with one operation under `method`.
pub fn item(method: HttpMethod, op: Operation) -> PathItem {
    PathItem::new(method, op)
}

/// A path item answering GET with JSON of the `ok_schema` shape.
pub fn get(id: &str, summary: &str, ok_schema: &str) -> PathItem {
    item(HttpMethod::Get, operation(id, summary, ok_schema, None))
}
