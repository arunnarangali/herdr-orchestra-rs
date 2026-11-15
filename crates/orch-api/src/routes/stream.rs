//! The live event stream. Registered by M4. Mount its routes under `/api/v1/stream`
//! ([`crate::api_router::STREAM_PREFIX`]): only there does the server also accept the token
//! as a `token` query parameter. Streams should end when `AppState::shutdown_signal` turns
//! true, so graceful shutdown does not wait on them.

use crate::api_router::ApiRouter;

/// Add the stream routes (and their OpenAPI entries) to `api`.
pub fn register(api: ApiRouter) -> ApiRouter {
    api
}
