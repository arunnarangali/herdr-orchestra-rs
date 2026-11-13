//! Handlers for `orch agents check`, `orch agents sync` and `orch doctor` (M2 fills
//! these in; nothing else edits this file). Each returns `None` while it is a stub, which
//! the caller reports as "not implemented" (exit 70). `args` are the words after the
//! command (for `agents check`: after `check`).

pub fn agents_check(_args: &[String]) -> Option<i32> {
    None
}

pub fn agents_sync(_args: &[String]) -> Option<i32> {
    None
}

pub fn doctor(_args: &[String]) -> Option<i32> {
    None
}
