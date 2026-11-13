//! Handlers for `orch write-task` and `orch lead plan|run|whoami`. Only the task-contract
//! milestone edits this module. Each handler returns the exit code (or `None`, reported as
//! "not implemented", when it cannot run). `args` are the words after the command (for
//! `lead`: after the subcommand).
//!
//! The bodies live in their own files (`write_task.rs`, `lead.rs`) without touching the
//! binary's module list, as the task-contract milestone's likely-files list asks.

/// `orch write-task <Mn>`: the milestone's `task.md` and `task.json` (the coder contract).
#[path = "write_task.rs"]
pub mod write_task;

/// `orch lead plan` / `orch lead run`: print the lead playbook. `orch lead whoami`: the
/// lead's own herdr pane.
#[path = "lead.rs"]
pub mod lead;

/// `orch write-task <Mn>`: generate `TASKDIR/task.md` from the plan.
pub fn write_task(args: &[String]) -> Option<i32> {
    Some(write_task::run(args))
}

/// `orch lead plan` / `orch lead run`: print the lead playbook.
pub fn lead_playbook(which: &str, _args: &[String]) -> Option<i32> {
    Some(lead::playbook(which))
}

/// `orch lead whoami`: the lead's own herdr pane.
pub fn lead_whoami(_args: &[String]) -> Option<i32> {
    Some(lead::whoami())
}
