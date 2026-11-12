//! The typed task state machine: the 15 states, the §11 transition table (identical to Python
//! v4.10.1 `TRANSITIONS`), manual-change rules, and the task record.
//!
//! Nothing here touches storage; `store` is the only place a task's state is written, and it
//! only does so through [`check_transition`].

use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Pending,
    Coding,
    Verifying,
    AwaitingReview,
    Reviewing,
    Reviewed,
    Committed,
    Merged,
    NeedsAnswer,
    NeedsFix,
    Held,
    Paused,
    Failed,
    Conflict,
    Skipped,
}

use State::*;

impl State {
    /// All 15 states.
    pub const ALL: [State; 15] = [
        Pending,
        Coding,
        Verifying,
        AwaitingReview,
        Reviewing,
        Reviewed,
        Committed,
        Merged,
        NeedsAnswer,
        NeedsFix,
        Held,
        Paused,
        Failed,
        Conflict,
        Skipped,
    ];

    /// What `orch set` allows without force (Python `SET_OK`).
    pub const MANUAL_OK: [State; 4] = [Pending, Paused, Failed, Skipped];

    /// States whose entry (and entry into `merged`) the lead must hear about (Python `LEAD_BLOCKING`).
    pub const LEAD_BLOCKING: [State; 4] = [Paused, Failed, Held, NeedsAnswer];

    pub fn as_str(self) -> &'static str {
        match self {
            Pending => "pending",
            Coding => "coding",
            Verifying => "verifying",
            AwaitingReview => "awaiting-review",
            Reviewing => "reviewing",
            Reviewed => "reviewed",
            Committed => "committed",
            Merged => "merged",
            NeedsAnswer => "needs-answer",
            NeedsFix => "needs-fix",
            Held => "held",
            Paused => "paused",
            Failed => "failed",
            Conflict => "conflict",
            Skipped => "skipped",
        }
    }

    /// States this one may move to (the §11 table). Empty for `merged`.
    pub fn allowed_targets(self) -> &'static [State] {
        match self {
            Pending => &[Coding, NeedsAnswer, Skipped, Pending, Paused, Failed],
            NeedsAnswer => &[Pending, Skipped],
            Skipped => &[Pending],
            Coding => &[Verifying, NeedsFix, Conflict, Pending, Paused, Failed],
            Verifying => &[
                Coding,
                AwaitingReview,
                Reviewing,
                Committed,
                Merged,
                NeedsFix,
                Held,
                Pending,
                Paused,
                Failed,
            ],
            AwaitingReview => &[Reviewing, Pending, Paused, Failed],
            Reviewing => &[Reviewed, AwaitingReview, Pending, Paused, Failed],
            Reviewed => &[NeedsFix, Committed, Merged, Held, Pending, Paused, Failed],
            NeedsFix => &[Coding, Pending, Paused, Failed],
            Committed => &[Merged, Conflict, NeedsFix, Held, Pending, Paused, Failed],
            Held => &[Committed, Pending, Paused, Failed],
            Conflict => &[Coding, Pending, Paused, Failed],
            Paused => &[Coding, Skipped, Pending, Failed],
            Failed => &[Skipped, Pending, Paused],
            Merged => &[],
        }
    }

    pub fn is_terminal(self) -> bool {
        self == Merged
    }

    /// Entering this state (from another) produces a lead event.
    pub fn is_lead_event(self) -> bool {
        Self::LEAD_BLOCKING.contains(&self) || self == Merged
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for State {
    type Err = TaskError;
    fn from_str(s: &str) -> Result<Self, TaskError> {
        State::ALL
            .into_iter()
            .find(|st| st.as_str() == s)
            .ok_or_else(|| TaskError::UnknownState(s.to_string()))
    }
}

/// How a state change was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Driven by the orchestrator: the table applies.
    Auto,
    /// A manual change (`orch set`): only [`State::MANUAL_OK`] targets, and the table applies.
    Manual,
    /// A manual change with force: skips the table and the manual-target rule. Recorded as forced.
    Forced,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TaskError {
    #[error("unknown status {0:?} — one of {all}", all = State::ALL.map(State::as_str).join(", "))]
    UnknownState(String),
    #[error("{id}: {from} -> {to} is not an allowed transition{}", if *.from == State::Merged { " (merged is final)" } else { "" })]
    NotAllowed { id: String, from: State, to: State },
    #[error(
        "`orch set` only moves a task to {ok}; setting {to:?} by hand skips the steps that make it true — force it if you really mean it",
        ok = State::MANUAL_OK.map(State::as_str).join(", ")
    )]
    ManualTarget { to: State },
}

impl From<TaskError> for Error {
    fn from(e: TaskError) -> Self {
        Error::wrap(e, crate::error::EXIT_USAGE)
    }
}

/// Is `from -> to` allowed for a request in `mode`? Same-state moves are always fine here
/// (the store turns them into a no-op or a reason-only record).
pub fn check_transition(id: &str, from: State, to: State, mode: Mode) -> Result<(), TaskError> {
    if mode == Mode::Forced {
        return Ok(());
    }
    if mode == Mode::Manual && !State::MANUAL_OK.contains(&to) {
        return Err(TaskError::ManualTarget { to });
    }
    if to != from && !from.allowed_targets().contains(&to) {
        return Err(TaskError::NotAllowed {
            id: id.to_string(),
            from,
            to,
        });
    }
    Ok(())
}

/// A task as stored: state, dependencies, assignment and round counters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: State,
    pub reason: String,
    pub deps: Vec<String>,
    pub worker: Option<String>,
    pub last_worker: Option<String>,
    pub model: Option<String>,
    pub attempt: u32,
    pub fix_rounds: u32,
    pub review_rounds: u32,
    pub conflict_rounds: u32,
    /// Attempts lost (agent vanished) — Python `lost`.
    pub lost: u32,
    /// Whether the one escalation to a stronger model was used.
    pub escalated: bool,
}

impl Task {
    /// Derived: coding with a worker. Not a stored state.
    pub fn is_assigned(&self) -> bool {
        self.status == Coding && self.worker.is_some()
    }

    /// Derived: pending with every dependency merged. `dep_status` looks a dependency up
    /// (`None` = unknown, which is not merged). Not a stored state.
    pub fn is_ready(&self, dep_status: impl Fn(&str) -> Option<State>) -> bool {
        self.status == Pending && self.deps.iter().all(|d| dep_status(d) == Some(Merged))
    }
}

/// One recorded state change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub task: String,
    pub ts: i64,
    pub from: State,
    pub to: State,
    /// Triggering event / reason.
    pub why: String,
    pub worker: Option<String>,
    pub model: Option<String>,
    pub attempt: u32,
    pub forced: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The §11 / Python TRANSITIONS table, written independently as text.
    const TABLE: &[(&str, &str)] = &[
        (
            "pending",
            "coding needs-answer skipped pending paused failed",
        ),
        ("needs-answer", "pending skipped"),
        ("skipped", "pending"),
        (
            "coding",
            "verifying needs-fix conflict pending paused failed",
        ),
        (
            "verifying",
            "coding awaiting-review reviewing committed merged needs-fix held pending paused failed",
        ),
        ("awaiting-review", "reviewing pending paused failed"),
        (
            "reviewing",
            "reviewed awaiting-review pending paused failed",
        ),
        (
            "reviewed",
            "needs-fix committed merged held pending paused failed",
        ),
        ("needs-fix", "coding pending paused failed"),
        (
            "committed",
            "merged conflict needs-fix held pending paused failed",
        ),
        ("held", "committed pending paused failed"),
        ("conflict", "coding pending paused failed"),
        ("paused", "coding skipped pending failed"),
        ("failed", "skipped pending paused"),
        ("merged", ""),
    ];

    fn table_allows(from: State, to: State) -> bool {
        TABLE
            .iter()
            .find(|(f, _)| *f == from.as_str())
            .unwrap()
            .1
            .split_whitespace()
            .any(|t| t == to.as_str())
    }

    #[test]
    fn fifteen_states_roundtrip() {
        assert_eq!(State::ALL.len(), 15);
        for s in State::ALL {
            assert_eq!(s.as_str().parse::<State>().unwrap(), s);
            let j = serde_json::to_string(&s).unwrap();
            assert_eq!(j, format!("\"{}\"", s.as_str()));
            assert_eq!(serde_json::from_str::<State>(&j).unwrap(), s);
        }
        assert!("nope".parse::<State>().is_err());
    }

    #[test]
    fn all_225_pairs_match_the_table() {
        for from in State::ALL {
            for to in State::ALL {
                // same-state is allowed by the table for park states, and a no-op otherwise
                let expect = from == to || table_allows(from, to);
                assert_eq!(
                    check_transition("M1", from, to, Mode::Auto).is_ok(),
                    expect,
                    "{from} -> {to}"
                );
                if from != to {
                    assert_eq!(
                        from.allowed_targets().contains(&to),
                        table_allows(from, to),
                        "{from} -> {to}"
                    );
                }
            }
        }
    }

    #[test]
    fn merged_is_final() {
        assert!(Merged.is_terminal());
        let e = check_transition("M1", Merged, Coding, Mode::Auto).unwrap_err();
        assert!(e.to_string().contains("merged is final"), "{e}");
        let e = check_transition("M1", Coding, Merged, Mode::Auto).unwrap_err();
        assert!(!e.to_string().contains("merged is final"));
    }

    #[test]
    fn manual_targets_and_force() {
        for to in State::ALL {
            let ok = State::MANUAL_OK.contains(&to);
            let r = check_transition("M1", Pending, to, Mode::Manual);
            if !ok {
                assert!(matches!(r, Err(TaskError::ManualTarget { .. })), "{to}");
            }
            assert!(check_transition("M1", Merged, to, Mode::Forced).is_ok());
        }
        // manual still obeys the table
        assert!(check_transition("M1", Merged, Pending, Mode::Manual).is_err());
        assert!(check_transition("M1", Coding, Paused, Mode::Manual).is_ok());
    }

    #[test]
    fn lead_events() {
        assert!(Paused.is_lead_event() && Merged.is_lead_event() && NeedsAnswer.is_lead_event());
        assert!(!Coding.is_lead_event());
    }

    fn task(status: State, deps: &[&str], worker: Option<&str>) -> Task {
        Task {
            id: "M2".into(),
            title: String::new(),
            status,
            reason: String::new(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            worker: worker.map(String::from),
            last_worker: None,
            model: None,
            attempt: 0,
            fix_rounds: 0,
            review_rounds: 0,
            conflict_rounds: 0,
            lost: 0,
            escalated: false,
        }
    }

    #[test]
    fn derived_ready_and_assigned() {
        let st = |d: &str| match d {
            "M1" => Some(Merged),
            "M0" => Some(Coding),
            _ => None,
        };
        assert!(task(Pending, &[], None).is_ready(st));
        assert!(task(Pending, &["M1"], None).is_ready(st));
        assert!(!task(Pending, &["M1", "M0"], None).is_ready(st));
        assert!(!task(Pending, &["gone"], None).is_ready(st));
        assert!(!task(Coding, &["M1"], None).is_ready(st));
        assert!(task(Coding, &[], Some("w1")).is_assigned());
        assert!(!task(Coding, &[], None).is_assigned());
        assert!(!task(Pending, &[], Some("w1")).is_assigned());
    }

    #[test]
    fn converts_into_core_error() {
        let e: Error = TaskError::UnknownState("x".into()).into();
        assert_eq!(e.exit_code(), 2);
    }
}
