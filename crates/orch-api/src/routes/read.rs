//! Read endpoints (plan, tasks, history, reviews, reports…): they only look at the run.
//! Registered by M2. Routes go under `/api/v1/…` and are added with [`ApiRouter::route`].

use crate::api_router::ApiRouter;

/// Add the read routes (and their OpenAPI entries) to `api`.
pub fn register(api: ApiRouter) -> ApiRouter {
    api
}
