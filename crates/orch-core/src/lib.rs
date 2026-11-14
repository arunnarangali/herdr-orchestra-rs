//! Shared core of orch: error type, repo paths, logging, tasks, storage and the driver lease.

pub mod claim;
pub mod conflict;
pub mod error;
pub mod events;
pub mod git;
pub mod graph;
pub mod journal;
pub mod lead_queue;
pub mod lease;
pub mod logging;
pub mod manager;
pub mod merge;
pub mod paths;
pub mod plan;
pub mod plan_edit;
pub mod reconcile;
pub mod refs;
pub mod run_store;
pub mod scheduler;
pub mod setup;
pub mod store;
pub mod task;
pub mod task_contract;
pub mod wake;
pub mod worktree;

pub use error::{Error, Result};
