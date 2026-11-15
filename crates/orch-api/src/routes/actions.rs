//! Action endpoints (answer, dispatch, approve…): they send requests the event manager
//! carries out, exactly like the CLI commands do. Registered by M3.

use crate::api_router::ApiRouter;

/// Add the action routes (and their OpenAPI entries) to `api`.
pub fn register(api: ApiRouter) -> ApiRouter {
    api
}
