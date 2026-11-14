//! `orch autopilot`, the event manager. The event-manager milestone owns this file; until then
//! it is a "not implemented" stub with its own route. `args` are the words after the command.
//!
//! The lead's wake-up logic lives in `orch_core::wake` (`wake_tick`); the loop calls it with
//! [`crate::events_cmd::HerdrLead`], `NotifySend` and `SystemClock`, and the autopilot log as
//! the `say` sink.

use crate::commands::EXIT_NOT_IMPLEMENTED;

/// `orch autopilot`.
pub fn run(_args: &[String]) -> i32 {
    eprintln!("orch autopilot: not implemented in this build (exit {EXIT_NOT_IMPLEMENTED})");
    EXIT_NOT_IMPLEMENTED
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_stub_exits_70() {
        assert_eq!(super::run(&[]), 70);
    }
}
