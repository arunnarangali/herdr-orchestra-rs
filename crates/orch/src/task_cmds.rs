//! Handlers for `orch write-task` and `orch lead plan|run|whoami`. Only the task-contract
//! milestone edits this module. Each handler returns `None` (reported as "not implemented",
//! exit 70) until it is built. `args` are the words after the command (for `lead`: after
//! the subcommand).

/// `orch write-task <Mn>`: generate `TASKDIR/task.md` from the plan.
pub fn write_task(_args: &[String]) -> Option<i32> {
    None
}

/// `orch lead plan` / `orch lead run`: print the lead playbook.
pub fn lead_playbook(_which: &str, _args: &[String]) -> Option<i32> {
    None
}

/// `orch lead whoami`: the lead's own herdr pane.
pub fn lead_whoami(_args: &[String]) -> Option<i32> {
    None
}
