//! Route modules. `system` (health, the OpenAPI document) belongs to the foundation; `read`,
//! `actions` and `stream` are each owned by one later milestone, which adds its routes inside
//! its own `register` and touches nothing else.

use crate::api_router::ApiRouter;

pub mod actions;
pub mod chat;
pub mod lead;
pub mod prompt;
pub mod read;
pub mod stream;
pub mod system;

/// Every route module, in registration order. Adding a module is the only reason to edit this.
pub(crate) fn register_all(api: ApiRouter) -> ApiRouter {
    let api = system::register(api);
    let api = read::register(api);
    let api = actions::register(api);
    let api = chat::register(api);
    let api = lead::register(api);
    let api = prompt::register(api);
    stream::register(api)
}
