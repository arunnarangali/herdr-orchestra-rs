//! Starting an agent in a pane that is already open: run its launch command, then
//! give it its name once herdr has seen it start. `orch team up` and the GUI's Lead
//! launcher both start agents this way, so the waiting for first-run questions
//! (trust this folder, sign in) behaves the same for both.

use crate::adapter::RuntimeAdapter;
use crate::error::HerdrError;
use std::time::Duration;

/// What to start, where, and under which name.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentStart<'a> {
    /// The pane, already open in the agent's folder.
    pub pane: &'a str,
    /// The shell command that starts the agent (the registry's launch command).
    pub command: &'a str,
    /// The name the agent gets in herdr.
    pub name: &'a str,
    /// Whether herdr can watch the kind (`herdr_detect`). An unwatchable agent is
    /// named once, without waiting: herdr never reports it started.
    pub detect: bool,
    /// Seconds herdr has to see the agent start (`ORCH_TEAM_WAIT_S`).
    pub wait_s: f64,
}

/// How starting went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    /// herdr saw the agent start and it carries its name.
    Named,
    /// The kind is unwatchable: the command runs and naming was tried once.
    Unwatched,
    /// The command runs, but herdr did not see the agent start within the wait
    /// (often a first-run question in the pane).
    NotSeen,
    /// `herdr pane run` failed: nothing runs in the pane.
    RunFailed(HerdrError),
}

/// Run `req.command` in `req.pane` and name the agent `req.name` once herdr sees it.
///
/// Naming is retried every `wait_s / 30` seconds (between 0.2 s and 2 s) until
/// `wait_s` has passed, so it takes at most about `wait_s` plus one step. `sleep`
/// waits between attempts (real sleeps in production; tests pass a no-op).
pub fn start_agent_in_pane(
    runtime: &dyn RuntimeAdapter,
    req: &AgentStart<'_>,
    sleep: &mut dyn FnMut(Duration),
) -> Started {
    if let Err(e) = runtime.run_command(req.pane, req.command) {
        return Started::RunFailed(e);
    }
    if !req.detect {
        let _ = runtime.rename_agent(req.pane, req.name);
        return Started::Unwatched;
    }
    if name_when_seen(runtime, req.pane, req.name, req.wait_s, sleep) {
        Started::Named
    } else {
        Started::NotSeen
    }
}

/// Name the agent in `pane` `name` as soon as herdr sees it, trying for about `wait_s`
/// seconds; `false` when herdr never saw it. Nothing is typed into the pane, so this is also
/// how to pick up an agent whose start was left waiting (at a first-run question).
pub fn name_when_seen(
    runtime: &dyn RuntimeAdapter,
    pane: &str,
    name: &str,
    wait_s: f64,
    sleep: &mut dyn FnMut(Duration),
) -> bool {
    let step = (wait_s / 30.0).clamp(0.2, 2.0);
    let tries = if wait_s > 0.0 {
        (wait_s / step).ceil() as u32 + 1
    } else {
        1
    };
    for attempt in 0..tries {
        if attempt > 0 {
            sleep(Duration::from_secs_f64(step));
        }
        if runtime.rename_agent(pane, name).is_ok() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{CallKind, FakeHerdr, Sent};
    use crate::model::PaneInfo;

    fn req<'a>(detect: bool, wait_s: f64) -> AgentStart<'a> {
        AgentStart {
            pane: "w1:p1",
            command: "claude --permission-mode acceptEdits",
            name: "lead",
            detect,
            wait_s,
        }
    }

    fn fake() -> FakeHerdr {
        let f = FakeHerdr::new();
        f.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        f
    }

    #[test]
    fn a_detected_agent_is_run_and_named() {
        let f = fake();
        f.set_detect_on_run(true);
        let r = start_agent_in_pane(&f, &req(true, 5.0), &mut |_| {});
        assert_eq!(r, Started::Named);
        assert_eq!(f.agent("lead").unwrap().pane_id, "w1:p1");
        assert_eq!(
            f.sent(),
            vec![Sent::Run {
                pane: "w1:p1".into(),
                command: "claude --permission-mode acceptEdits".into()
            }]
        );
    }

    #[test]
    fn an_agent_herdr_never_sees_is_reported_after_a_bounded_wait() {
        let f = fake();
        let mut slept = Duration::ZERO;
        let r = start_agent_in_pane(&f, &req(true, 6.0), &mut |d| slept += d);
        assert_eq!(r, Started::NotSeen);
        assert!(slept <= Duration::from_secs_f64(6.0 + 0.2), "{slept:?}");
        assert!(slept >= Duration::from_secs_f64(6.0 - 0.2), "{slept:?}");
    }

    #[test]
    fn a_failed_run_is_reported_and_nothing_is_named() {
        let f = fake();
        f.fail_next(CallKind::PaneRun, 1, HerdrError::other("boom"));
        let r = start_agent_in_pane(&f, &req(true, 5.0), &mut |_| panic!("no wait"));
        assert_eq!(r, Started::RunFailed(HerdrError::other("boom")));
    }

    #[test]
    fn an_unwatchable_agent_is_named_once_without_waiting() {
        let f = fake();
        let r = start_agent_in_pane(&f, &req(false, 5.0), &mut |_| panic!("no wait"));
        assert_eq!(r, Started::Unwatched);
    }
}
