//! Handlers for `orch team` and `orch team up` (M3 fills these in; nothing else edits
//! this file). Each returns `None` while it is a stub, which the caller reports as "not
//! implemented" (exit 70). `args` are the words after the command (for `team up`: after
//! `up`).

pub fn team(_args: &[String]) -> Option<i32> {
    None
}

pub fn team_up(_args: &[String]) -> Option<i32> {
    None
}
