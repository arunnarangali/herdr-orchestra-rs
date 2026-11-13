//! Handlers for `orch sync`, `orch status` and `orch set`. Only the plan-validation and
//! run-storage milestone edits this module. Each handler returns `None` (reported as
//! "not implemented", exit 70) until it is built. `args` are the words after the command.

/// `orch sync`: parse `.orchestrate/plan.md` and validate it.
pub fn sync(_args: &[String]) -> Option<i32> {
    None
}

/// `orch status`: the table of workers and tasks.
pub fn status(_args: &[String]) -> Option<i32> {
    None
}

/// `orch set <Mn> <status> [reason] [--force]`.
pub fn set(_args: &[String]) -> Option<i32> {
    None
}
