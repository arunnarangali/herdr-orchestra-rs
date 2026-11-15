//! The developer's levers on a run, as typed functions: what the CLI commands and the REST API
//! both call. They never drive the run: each writes the request the event manager carries out
//! (a database transition, a plan edit, a run-control request) and records who asked.
//!
//! Also here: the run-level controls (`orch pause`, `resume`, `stop`). The request is stored in
//! `run_control`; the event manager reads it every cycle, so it survives a restart.

use crate::plan_edit::{Answered, EditError, Fixed, py_repr};
use crate::run_store::Run;
use crate::scheduler::TIERS;
use crate::store::{Store, StoreError, TransitionRequest, now};
use crate::task::{State, Task};
use rusqlite::params;
use std::path::Path;

type Res<T> = std::result::Result<T, StoreError>;

/// Where an action came from; recorded with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Cli,
    Api,
    /// The event manager itself (it clears a finished stop request when it starts).
    Manager,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Cli => "cli",
            Source::Api => "api",
            Source::Manager => "manager",
        }
    }

    /// Appended to the reason of a state change an API request caused.
    fn why_suffix(self) -> &'static str {
        match self {
            Source::Cli | Source::Manager => "",
            Source::Api => " (via the API)",
        }
    }
}

/// Why an action was refused or failed. The text is the one the CLI prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    /// No such milestone (or worker's milestone).
    NotFound(String),
    /// The request itself is wrong: a missing text, an unknown tier or worker.
    Invalid(String),
    /// The milestone's state does not allow it.
    Refused(String),
    /// Not the caller's fault: the database or a file failed.
    Internal(String),
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (ActionError::NotFound(m)
        | ActionError::Invalid(m)
        | ActionError::Refused(m)
        | ActionError::Internal(m)) = self;
        f.write_str(m)
    }
}

impl std::error::Error for ActionError {}

impl From<StoreError> for ActionError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NoSuchTask(_) => ActionError::NotFound(e.to_string()),
            StoreError::Task(_) => ActionError::Refused(e.to_string()),
            other => ActionError::Internal(other.to_string()),
        }
    }
}

impl From<EditError> for ActionError {
    fn from(e: EditError) -> Self {
        match e {
            EditError::Refused(m) if m.starts_with("unknown task") => ActionError::NotFound(m),
            EditError::Refused(m) => ActionError::Refused(m),
            EditError::Store(s) => s.into(),
            EditError::Io(m) => ActionError::Internal(m),
        }
    }
}

/// Python's refusal for an unknown milestone: `unknown task 'X' — known: M1, M2`.
pub fn unknown_task_message(run: &Run, tid: &str) -> String {
    let known: Vec<&str> = run.plan.iter().map(|p| p.id.as_str()).collect();
    format!(
        "unknown task {} — known: {}",
        py_repr(tid),
        if known.is_empty() {
            "none".to_string()
        } else {
            known.join(", ")
        }
    )
}

fn load(store: &Store) -> Result<Run, ActionError> {
    match store.load_run()? {
        Some(r) => Ok(r),
        None => Err(ActionError::Refused(
            "no run in .orchestrate/state.db — run `orch sync` after writing plan.md".into(),
        )),
    }
}

/// The milestone, or [`ActionError::NotFound`] with the CLI's wording.
pub fn known_task(store: &Store, tid: &str) -> Result<Task, ActionError> {
    match store.get_task(tid) {
        Ok(t) => Ok(t),
        Err(StoreError::NoSuchTask(_)) => Err(ActionError::NotFound(unknown_task_message(
            &load(store)?,
            tid,
        ))),
        Err(e) => Err(e.into()),
    }
}

fn check_tier(tier: Option<&str>) -> Result<(), ActionError> {
    match tier {
        Some(t) if !TIERS.contains(&t) => Err(ActionError::Invalid(
            "tier must be small, medium or high".into(),
        )),
        _ => Ok(()),
    }
}

fn check_worker(run: &Run, to: Option<&str>) -> Result<(), ActionError> {
    use crate::run_store::AgentRole;
    if let Some(to) = to {
        let names: Vec<&str> = run
            .agents
            .iter()
            .filter(|a| a.role == AgentRole::Worker)
            .map(|a| a.name.as_str())
            .collect();
        if !names.contains(&to) {
            return Err(ActionError::Invalid(format!(
                "unknown worker {} — workers: {}",
                py_repr(to),
                names.join(", ")
            )));
        }
    }
    Ok(())
}

/// One recorded action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRecord {
    pub id: i64,
    pub ts: i64,
    pub source: String,
    pub action: String,
    pub task: Option<String>,
    pub detail: String,
}

/// What the developer asked of the event manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunControl {
    /// Carry on (the default: no request stored).
    Running,
    /// Start no new work; running milestones finish their step.
    Paused,
    /// Save each running coder's work and end the event manager (asked, not yet done).
    Stopping,
    /// The event manager ended on request. A fresh start clears it.
    Stopped,
}

impl RunControl {
    pub fn as_str(self) -> &'static str {
        match self {
            RunControl::Running => "running",
            RunControl::Paused => "paused",
            RunControl::Stopping => "stopping",
            RunControl::Stopped => "stopped",
        }
    }

    fn parse(s: &str) -> RunControl {
        match s {
            "paused" => RunControl::Paused,
            "stopping" => RunControl::Stopping,
            "stopped" => RunControl::Stopped,
            _ => RunControl::Running,
        }
    }
}

impl Store {
    /// The stored run-control request, `Running` when there is none.
    pub fn run_control(&self) -> Res<RunControl> {
        use rusqlite::OptionalExtension;
        let s: Option<String> = self
            .conn
            .query_row("SELECT state FROM run_control WHERE id=1", [], |r| r.get(0))
            .optional()?;
        Ok(s.as_deref().map_or(RunControl::Running, RunControl::parse))
    }

    /// Store the request (replacing the previous one) and record the action.
    pub fn set_run_control(&mut self, state: RunControl, source: Source) -> Res<()> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO run_control (id, state, since, source) VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET state=?1, since=?2, source=?3",
            params![state.as_str(), now(), source.as_str()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Write one audit row.
    pub fn record_action(
        &mut self,
        source: Source,
        action: &str,
        task: Option<&str>,
        detail: &str,
    ) -> Res<()> {
        self.conn.execute(
            "INSERT INTO run_actions (ts, source, action, task_id, detail) VALUES (?1,?2,?3,?4,?5)",
            params![now(), source.as_str(), action, task, detail],
        )?;
        Ok(())
    }

    /// Recorded actions, oldest first.
    pub fn actions(&self) -> Res<Vec<ActionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, source, action, task_id, detail FROM run_actions ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ActionRecord {
                id: r.get(0)?,
                ts: r.get(1)?,
                source: r.get(2)?,
                action: r.get(3)?,
                task: r.get(4)?,
                detail: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

// ---------------------------------------------------------------------- the levers

/// `orch approve <Mn>`: let a held milestone land (it moves to `committed`; the event manager
/// merges it on its next step). Only a held milestone can be approved.
pub fn approve(store: &mut Store, tid: &str, source: Source) -> Result<Task, ActionError> {
    let t = known_task(store, tid)?;
    if t.status != State::Held {
        return Err(ActionError::Refused(format!(
            "{tid} is {}, not held",
            t.status
        )));
    }
    let why = format!("approved by the developer{}", source.why_suffix());
    store.transition(tid, &TransitionRequest::auto(State::Committed, Some(&why)))?;
    store.record_action(source, "approve", Some(tid), "")?;
    Ok(store.get_task(tid)?)
}

/// The status words `orch set` accepts: `Err` is the CLI's refusal text. Without `force` only
/// the states in `State::MANUAL_OK` are allowed.
pub fn manual_status(status: &str, force: bool) -> Result<State, String> {
    let Ok(to) = status.parse::<State>() else {
        let mut all: Vec<&str> = State::ALL.iter().map(|s| s.as_str()).collect();
        all.sort_unstable();
        return Err(format!(
            "unknown status {} — one of {}",
            py_repr(status),
            all.join(", ")
        ));
    };
    if !State::MANUAL_OK.contains(&to) && !force {
        let mut ok: Vec<&str> = State::MANUAL_OK.iter().map(|s| s.as_str()).collect();
        ok.sort_unstable();
        return Err(format!(
            "`orch set` only moves a task to {}; setting {} by hand skips the steps that make it true — add --force if you really mean it",
            ok.join(", "),
            py_repr(status)
        ));
    }
    Ok(to)
}

/// `orch set <Mn> <status> [reason] [--force]`.
pub fn set_status(
    store: &mut Store,
    tid: &str,
    to: State,
    reason: &str,
    force: bool,
    source: Source,
) -> Result<Task, ActionError> {
    let run = load(store)?;
    if !run.plan.iter().any(|p| p.id == tid) {
        return Err(ActionError::NotFound(unknown_task_message(&run, tid)));
    }
    let mut reason = reason.to_string();
    if force {
        reason = format!("manual override: {reason}").trim().to_string();
    }
    let reason = format!("{reason}{}", source.why_suffix());
    store.manual_set(tid, to, reason.trim(), force)?;
    store.record_action(source, "set", Some(tid), to.as_str())?;
    Ok(store.get_task(tid)?)
}

/// `orch dispatch <Mn>... [--to w] [--tier t]`: returns the milestones as they are now.
pub fn dispatch(
    store: &mut Store,
    ids: &[String],
    to: Option<&str>,
    tier: Option<&str>,
    source: Source,
) -> Result<Vec<Task>, ActionError> {
    if ids.is_empty() {
        return Err(ActionError::Invalid(
            "name at least one milestone to dispatch".into(),
        ));
    }
    let run = load(store)?;
    for id in ids {
        known_task(store, id)?;
    }
    check_tier(tier)?;
    check_worker(&run, to)?;
    store.dispatch_milestones(ids, to, tier)?;
    for id in ids {
        store.record_action(source, "dispatch", Some(id), "")?;
    }
    ids.iter().map(|id| Ok(store.get_task(id)?)).collect()
}

/// `orch answer <Mn> <text>`: the milestone must be paused; the text goes under its
/// `Notes from user` in `plan_path` and an `AnswerProvided` entry reaches the lead's queue.
pub fn answer(
    store: &mut Store,
    plan_path: &Path,
    tid: &str,
    text: &str,
    source: Source,
) -> Result<(Answered, Task), ActionError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ActionError::Invalid("the answer is empty".into()));
    }
    known_task(store, tid)?;
    let done = store.answer_milestone(plan_path, tid, text)?;
    store.record_action(source, "answer", Some(tid), "")?;
    Ok((done, store.get_task(tid)?))
}

/// `orch fix <Mn> <change> [--to w]`: Mn must have landed; returns the new follow-up milestone.
pub fn fix(
    store: &mut Store,
    plan_path: &Path,
    tid: &str,
    text: &str,
    to: Option<&str>,
    source: Source,
) -> Result<(Fixed, Task), ActionError> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(ActionError::Invalid("say what should change".into()));
    }
    let run = load(store)?;
    known_task(store, tid)?;
    check_worker(&run, to)?;
    let fixed = store.fix_milestone(plan_path, tid, &text, to)?;
    store.record_action(source, "fix", Some(&fixed.id), &format!("fix for {tid}"))?;
    let t = store.get_task(&fixed.id)?;
    Ok((fixed, t))
}

/// `orch escalate <Mn> [reason]`: `Ok(None)` when no stronger tier is possible (nothing
/// changed).
pub fn escalate(
    store: &mut Store,
    tid: &str,
    reason: &str,
    source: Source,
) -> Result<Option<(&'static str, Task)>, ActionError> {
    known_task(store, tid)?;
    let reason = format!("{}{}", reason.trim(), source.why_suffix());
    let Some(tier) = store.escalate_milestone(tid, reason.trim())? else {
        return Ok(None);
    };
    store.record_action(source, "escalate", Some(tid), tier)?;
    Ok(Some((tier, store.get_task(tid)?)))
}
