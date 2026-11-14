//! Handlers for `orch dispatch`, `answer`, `fix`, `approve` and `escalate`. The scheduler and
//! dispatch milestone owns this file; until then each is a "not implemented" stub that already
//! has its own route, so that milestone edits only this file. `args` are the words after the
//! command. Each handler returns the exit code.
//!
//! What this file may rely on (built with the lead's event queue):
//! * `Store::transition` queues the lead's entry for paused / failed / held / needs-answer
//!   and merged itself; callers never enqueue those by hand.
//! * `Store::enqueue_lead_event` queues `escalated`, `ready`, `auto-dispatched`, ...
//! * `Store::emit_answer_provided` queues the `AnswerProvided` entry every answer must emit,
//!   whoever gave it (the lead, or the developer typing `orch answer` in their own terminal).
//! * `Store::mark_dispatched` records a milestone as started, so it leaves READY.

use crate::commands::EXIT_NOT_IMPLEMENTED;

fn not_implemented(name: &str) -> i32 {
    eprintln!("orch {name}: not implemented in this build (exit {EXIT_NOT_IMPLEMENTED})");
    EXIT_NOT_IMPLEMENTED
}

/// `orch dispatch <Mn>... [--to <worker>] [--tier <tier>]`.
pub fn dispatch(_args: &[String]) -> i32 {
    not_implemented("dispatch")
}

/// `orch answer <Mn> <text>`.
pub fn answer(_args: &[String]) -> i32 {
    not_implemented("answer")
}

/// `orch fix <Mn> <text> [--to <worker>]`.
pub fn fix(_args: &[String]) -> i32 {
    not_implemented("fix")
}

/// `orch approve <Mn>`.
pub fn approve(_args: &[String]) -> i32 {
    not_implemented("approve")
}

/// `orch escalate <Mn> [reason]`.
pub fn escalate(_args: &[String]) -> i32 {
    not_implemented("escalate")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stub_exits_70() {
        let none: [String; 0] = [];
        for f in [dispatch, answer, fix, approve, escalate] {
            assert_eq!(f(&none), 70);
        }
    }
}
