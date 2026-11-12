//! The herdr runtime adapter: the one contract every later phase talks to agents
//! through (§4), the typed values it returns, a precise error type, and an
//! in-memory fake backend tests can script.
//!
//! There is no real Herdr backend in this crate yet: milestone M2 owns the
//! herdr CLI backend ([`cli`], reserved) and the fake `herdr` executable, and
//! milestone M3 owns the delivery module ([`delivery`], reserved).

pub mod adapter;
pub mod cli;
pub mod delivery;
pub mod error;
pub mod fake;
pub mod model;
pub mod status;

pub use adapter::{RuntimeAdapter, SplitPane, SplitDirection, Availability};
pub use error::HerdrError;
pub use fake::{CallKind, FakeHerdr, Sent};
pub use model::{AgentInfo, PaneInfo, WorkspaceInfo};
pub use status::AgentStatus;
