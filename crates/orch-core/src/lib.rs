//! Shared core of orch: error type, repo paths, logging, tasks, storage and the driver lease.

pub mod error;
pub mod lease;
pub mod logging;
pub mod paths;
pub mod store;
pub mod task;

pub use error::{Error, Result};
