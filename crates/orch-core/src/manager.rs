//! The event manager (§9, §24 Phase 6): the single driver of a run. It holds the
//! driver lease, applies validated runtime events through the store's transition
//! function, asks the scheduler (M2) for assignments, delivers prompts through the
//! crash-safe deliverer (§15), wakes the lead (M1), auto-dispatches when the lead is
//! slow or absent, and stops with the same exit codes as Python's `cmd_autopilot`.
//!
//! Everything git, check, commit, merge and review related goes through the narrow
//! [`Work`] trait — Phase 7 and Phase 8 implement it; tests use fakes. This phase
//! adds no git code.
//!
//! There is no async runtime (§25 rule 21): one synchronous loop, bounded polls, a
//! stop request checked every cycle. Every state change is one store transaction, so
//! a kill at any point leaves the run consistent (reconcile repairs the rest).

use crate::claim::ClaimError;
use crate::conflict::IssueKind;
use crate::check::Records;
use crate::fix::FixKind;
use crate::events::{AgentState, PollOutcome, PollSource, RunEvent, RuntimeView};
use crate::error::Result;
use crate::lead_queue::LeadKind;
use crate::reconcile::park_with_lost;
use crate::run_store::AgentRole;
use crate::scheduler::{self, Assignment, Escalation, Job};
use crate::actions::{RunControl, Source};
use crate::store::{Counter, Delivery, DeliveryKind, Res, Store, TransitionRequest};
use crate::task::{check_transition, Mode, State};
use crate::wake::{wake_tick, Clock, LeadRuntime, Notifier, WakeEnv, WakeTunables};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Exit codes, Python's `cmd_autopilot`: 0 all done, 3 a stop, 2 usage (lease held).
pub const EXIT_ALL_DONE: i32 = 0;
pub const EXIT_STOPPED: i32 = 3;

/// Python's round caps (§15): the fix, review and merge-conflict rounds before a
/// milestone escalates or fails. (They are `ORCH_MAX_*` in orch-config; the loop
/// takes its copy here so orch-core stays below orch-config in the crate layers.)
pub const MAX_FIX: u32 = 3;
pub const MAX_REVIEW: u32 = 2;
pub const MAX_CONFLICT: u32 = 2;
/// §15: bench an agent after this many consecutive failed deliveries.
pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// §15 recovery: an unconfirmed wait older than this is resolved from the agent's
/// status (at least `ORCH_CONFIRM_S`).
pub const DEFAULT_CONFIRM_GRACE_S: i64 = 30;

/// The milestones' work that Phases 7 and 8 implement behind this trait: preparing a
/// milestone's worktree and contract files, running the check, building fix and
/// review prompts, reading the reviewer's verdict, and committing and merging. The
/// event manager calls only this — it adds no git code itself.
pub trait Work {
    /// Open the milestone's worktree (worktree mode) and run the setup command when
    /// dependency files changed (Phase 7).
    fn prepare(&mut self, task: &str, worker: &str) -> std::result::Result<(), String>;
    /// Write the milestone's contract files into `.orch-task/<Mn>/` (Phase 7).
    fn write_task(&mut self, task: &str) -> std::result::Result<(), String>;
    /// The body of a fix round's prompt file (`fix-<round>.md`; a `tail` problem on
    /// the result file asks for `result-fix-1.md`). Phase 8 builds it.
    fn fix_body(&mut self, task: &str, round: u32, tail: &str) -> std::result::Result<String, String>;
    /// Run the plan's check command (Phase 8). `task` is `"(final check)"` for the
    /// branch-wide check when every milestone is merged.
    fn check(&mut self, task: &str) -> std::result::Result<CheckOutcome, String>;
    /// Keep a milestone's work in its hidden ref before freeing its coder (Phase 7).
    fn save_work(&mut self, task: &str) -> std::result::Result<(), String>;
    /// The body of the review prompt file `review-task-<round>.md` (Phase 7 builds
    /// the diff, Phase 8 the instructions).
    fn review_body(&mut self, task: &str, round: u32) -> std::result::Result<String, String>;
    /// The reviewer's verdict for round `round`, from `review-<round>.json`/`.md`
    /// (Phase 8).
    fn review_verdict(&mut self, task: &str, round: u32) -> std::result::Result<ReviewOutcome, String>;
    /// Commit the milestone and merge it into the run branch (Phase 7).
    fn commit(&mut self, task: &str) -> std::result::Result<CommitOutcome, String>;

    /// The fix prompt for a typed problem, naming what is wrong and the result file to write
    /// next. The default folds the kind into [`fix_body`](Work::fix_body)'s `tail` argument
    /// (the check's tail, `"result file"`, or empty), so a `Work` that only knows the narrow
    /// form keeps working.
    fn fix_prompt(&mut self, task: &str, round: u32, kind: &FixKind) -> std::result::Result<String, String> {
        let tail = match kind {
            FixKind::Check { tail } => tail.as_str(),
            FixKind::ResultFile => "result file",
            _ => "",
        };
        self.fix_body(task, round, tail)
    }
    /// The files the coder changed outside the milestone's `Likely files` right now (the scope
    /// check). The default finds none, so only a real `Work` can send a milestone back for it.
    fn strays(&mut self, _task: &str) -> std::result::Result<Vec<String>, String> {
        Ok(Vec::new())
    }
    /// Write a queued job's body (`fix-1.md`, `review-fix-1.md`, `conflict-1.md`) where the coder
    /// will read it, just before the job is sent. The default writes it under the main folder's
    /// `.orch-task/<Mn>/`; a real `Work` writes it into the coder's own folder.
    fn write_job(&mut self, root: &Path, task: &str, file: &str, body: &str) -> std::result::Result<(), String> {
        let dir = root.join(".orch-task").join(task);
        std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        std::fs::write(dir.join(file), body).map_err(|e| format!("writing {}: {e}", dir.join(file).display()))
    }
    /// The `BLOCKER: …` lines of review round `round`, for the coder's fix job. The default has
    /// none (no job is queued); a real `Work` reads them from the reviewer's file.
    fn blocker_lines(&mut self, _task: &str, _round: u32) -> std::result::Result<Vec<String>, String> {
        Ok(Vec::new())
    }
    /// The check on the run branch before any work starts (the baseline gate): `(passed, the
    /// tail of its output)`. Not recorded against a milestone. The default passes.
    fn baseline(&mut self) -> std::result::Result<(bool, String), String> {
        Ok((true, String::new()))
    }
}

/// The prompt a coder gets for a prompt file (`task.md`, `fix-2.md`, …): Python `cmd_send`'s
/// text, with `attempt` naming the result, report and done files.
pub fn coder_prompt(task: &str, file: &str, attempt: u32) -> String {
    let rel = format!(".orch-task/{task}");
    format!(
        "Read {rel}/{file} and carry it out exactly (the milestone's contract is {rel}/task.json). When \
         completely finished, write {rel}/result-{attempt}.json in the `result_format` shape from task.json, \
         and a short report {rel}/report-{attempt}.md (max 20 lines: files changed, what you did, check \
         result; if something was ambiguous, status `question` in the result, a line starting QUESTION: \
         here, and stop; last line `COMMIT: <one-line commit message for the change>`), then create the \
         empty file {rel}/done-{attempt} as your very last action."
    )
}

/// The git-backed parts of [`Work`] (§13): `prepare` and `save_work`, over the run's
/// repository. It holds no state of its own: each call opens the run's database, so the event
/// manager can keep its own connection. An implementation of [`Work`] delegates to it.
#[derive(Debug, Clone)]
pub struct GitWork {
    /// The main repository root.
    pub root: PathBuf,
}

impl GitWork {
    pub fn new(root: PathBuf) -> Self {
        GitWork { root }
    }

    fn open(&self) -> crate::Result<Store> {
        Ok(Store::open_in(&self.root.join(crate::paths::ORCH_DIR))?)
    }

    /// [`Work::prepare`]: put the coder's folder on the milestone's starting point (its saved
    /// work, else the feature branch tip) and run the setup command there when none ran before
    /// or the dependency files changed. Refuses, with a message, a folder holding unsaved
    /// changes of another milestone.
    pub fn prepare(&self, task: &str, worker: &str) -> std::result::Result<(), String> {
        let run = || -> crate::Result<()> {
            let mut store = self.open()?;
            let prepared = crate::worktree::prepare_workdir(&mut store, &self.root, task, worker)?;
            for note in &prepared.notes {
                tracing::warn!("{note}");
            }
            let settings = store.run_settings()?.ok_or_else(|| {
                crate::Error::wrap(std::io::Error::other("no run in state.db"), crate::error::EXIT_USAGE)
            })?;
            if settings.isolation == "worktree"
                && let Some(why) = crate::setup::ensure_setup(&mut store, &settings, &prepared.dir)?
            {
                tracing::info!(?why, dir = %prepared.dir.display(), "ran the setup command");
            }
            Ok(())
        };
        run().map_err(|e| e.to_string())
    }

    /// [`Work::commit`]: land the milestone on the feature branch — squash, check on the merged
    /// result, commit only when green (see [`crate::merge`]). Repairs an interrupted merge first.
    /// `Err` means nothing was landed and the tree is as it was (a merge in progress elsewhere,
    /// a dirty main folder, missing saved work).
    pub fn commit(&self, task: &str) -> std::result::Result<CommitOutcome, String> {
        let run = || -> crate::Result<CommitOutcome> {
            let mut store = self.open()?;
            let landing = crate::merge::land(&mut store, &self.root, task, &crate::merge::Fault::from_env())?;
            if let Some(r) = &landing.recovered {
                tracing::warn!("{r}");
            }
            Ok(landing.outcome.outcome(task))
        };
        run().map_err(|e| e.to_string())
    }

    /// [`Work::save_work`]: keep the milestone's uncommitted work in its hidden ref. An error
    /// means the work is still in the folder, untouched.
    pub fn save_work(&self, task: &str) -> std::result::Result<(), String> {
        let run = || -> crate::Result<()> {
            let store = self.open()?;
            crate::worktree::save_task_work(&store, &self.root, task, "saved by the event manager")?;
            Ok(())
        };
        run().map_err(|e| e.to_string())
    }
}

/// What the plan's check command said (Phase 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    Pass,
    Fail { tail: String },
}

/// What the reviewer's round concluded (Phase 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewOutcome {
    Approved,
    Blockers { count: usize },
    Unusable { line: String },
}

/// What a commit and merge did (Phase 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    Merged,
    Held { line: String },
    Conflict { line: String },
    /// The milestone passes alone but fails the check on the run branch.
    BranchFails { line: String },
}

/// The outcome of one send or re-send (the delivery code's view, §15): the
/// runtime's own error type is reduced to its message.
#[derive(Debug, Clone, PartialEq)]
pub enum DeliveryOutcome {
    /// The runtime accepted the prompt: the delivery is confirmed.
    Sent { delivery: Delivery },
    /// The runtime refused it: the delivery stays unconfirmed with the error kept.
    Failed { delivery: Delivery, error: String },
}

/// What recovery decided about one unconfirmed delivery (§15, AC8).
#[derive(Debug, Clone, PartialEq)]
pub enum Recovered {
    /// The agent is working or blocked: the prompt reached it. Confirmed.
    Confirmed { delivery: Delivery },
    /// The agent is gone: reported, not re-sent, not confirmed.
    Gone { delivery: Delivery },
    /// Still unconfirmed and re-sendable (the agent is idle/unknown, or the
    /// runtime could not be asked).
    Unconfirmed { delivery: Delivery },
}

/// A prompt to record and deliver (§15): the one agent it may reach, the task and
/// attempt it starts, and the prompt text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewDelivery<'a> {
    pub agent: &'a str,
    pub task: &'a str,
    pub attempt: u32,
    pub kind: DeliveryKind,
    pub prompt: &'a str,
}

/// The crash-safe prompt delivery behind the manager (§15, AC3, AC10): the
/// record-before-send and confirm-after mechanics — and their herdr wiring — live
/// behind this trait, so the manager carries no runtime code; tests use a fake. The
/// store is passed per call: the manager owns it exclusively.
pub trait Deliveries {
    /// Record the delivery durably, then send it.
    fn send(&mut self, store: &mut Store, req: NewDelivery<'_>) -> Res<DeliveryOutcome>;
    /// Re-send a recorded but unconfirmed delivery to the agent it was recorded for,
    /// with the same prompt, task and attempt.
    fn resend(&mut self, store: &mut Store, id: i64) -> Res<DeliveryOutcome>;
    /// The agent's consecutive failed deliveries (§15: bench it after three).
    fn failure_streak(&self, store: &Store, agent: &str) -> u32;
    /// Resolve the unconfirmed deliveries a restart finds (§15).
    fn recover(&mut self, store: &mut Store, now: i64, grace_s: i64) -> Res<Vec<Recovered>>;
}

/// The loop's knobs (from the environment's tunables; the binary maps orch-config's
/// values onto these fields, tests set them directly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopTunables {
    /// `ORCH_IDLE_GRACE_S`: idle counts only after this.
    pub idle_grace_s: i64,
    /// `ORCH_CONFIRM_S`: a fresh unconfirmed delivery may still be in flight.
    pub confirm_s: i64,
    /// `ORCH_CODER_BUDGET_MIN` / `ORCH_REVIEW_BUDGET_MIN`, in seconds.
    pub coder_budget_s: i64,
    pub review_budget_s: i64,
    /// `ORCH_SEND_RETRY_S`: re-try a refused prompt after this.
    pub send_retry_s: i64,
    /// `ORCH_UNREACHABLE_MIN`: the bench after three failures in a row.
    pub unreachable_min: i64,
    /// `ORCH_COOLDOWN_MIN`: a rate-limited agent's bench.
    pub cooldown_min: i64,
    /// `ORCH_GONE_COOLDOWN_MIN`: a vanished agent's bench.
    pub gone_cooldown_min: i64,
    /// `ORCH_MAX_LOST`: fail a milestone after this many lost attempts.
    pub max_lost: i64,
    /// `ORCH_LEAD_WAIT_MIN` / `ORCH_LEAD_IDLE_MAX_MIN`, in seconds (lead mode).
    pub lead_wait_s: i64,
    pub lead_idle_max_s: i64,
    /// The lead's wake-up knobs (`ORCH_LEAD_*`, M1).
    pub lead_confirm_s: i64,
    pub lead_confirm_wakes: i64,
    pub lead_rewake_s: i64,
    pub lead_max_wakes: i64,
}

impl LoopTunables {
    /// The confirm grace for recovery: at least `ORCH_CONFIRM_S`, never below
    /// [`DEFAULT_CONFIRM_GRACE_S`].
    pub fn confirm_grace_s(&self) -> i64 {
        self.confirm_s.max(DEFAULT_CONFIRM_GRACE_S)
    }
}

/// A fix or conflict job body kept across a failed delivery, so the work is queued
/// again instead of lost (Python's `t["job"]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedJob {
    pub task: String,
    pub file: String,
    pub body: String,
}

/// `.orchestrate/events.jsonl`: one JSON line per lifecycle event, for humans and tools.
fn events_path(root: &Path) -> PathBuf {
    root.join(crate::paths::ORCH_DIR).join("events.jsonl")
}

fn append_lifecycle_event(root: &Path, event: &str, ts: i64) {
    append_lifecycle_event_with(root, event, ts, false);
}

/// Append `{"event", "ts"[, "resumed"]}` as one line (a single `write` of a whole line, so a
/// kill never leaves half a line). Best effort: the log is not worth failing the run for.
fn append_lifecycle_event_with(root: &Path, event: &str, ts: i64, resumed: bool) {
    use std::io::Write as _;
    let mut v = serde_json::json!({ "event": event, "ts": ts });
    if event == "autopilot-started" {
        v["resumed"] = serde_json::Value::Bool(resumed);
    }
    let line = format!("{v}\n");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(events_path(root)) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// The newest `autopilot-*` event in `events.jsonl` (`autopilot-started` or
/// `autopilot-shutdown`): a start that follows a shutdown is a resume.
fn last_lifecycle_event(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(events_path(root)).ok()?;
    text.lines().rev().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        let e = v.get("event")?.as_str()?;
        e.starts_with("autopilot-").then(|| e.to_string())
    })
}


/// The driver's own small state, in `.orchestrate/driver.json`. Progress that must
/// survive a restart (the driven budget, kept jobs) lives here; the orchestration
/// state itself stays in state.db.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DriverState {
    /// Driving seconds summed across restarts; waiting for the lead or the
    /// developer does not count.
    #[serde(default)]
    pub driven_s: f64,
    /// When the manager started only-waiting (for the lead or a human answer).
    #[serde(default)]
    pub waiting_since: Option<i64>,
    /// BLOCKED alert keys: a restart does not alert twice for the same wait.
    #[serde(default)]
    pub notified: Vec<String>,
    /// `{task}:{attempt}` keys of the one nudge per wait (§15: nudge once, then park).
    #[serde(default)]
    pub nudged: Vec<String>,
    /// The undispatched set the lead last heard about (`ready`, once per set).
    #[serde(default)]
    pub lead_ready_told: Vec<String>,
    /// When an undispatched ready milestone was first seen (the `LEAD_WAIT` clock).
    #[serde(default)]
    pub ready_ts: Vec<(String, i64)>,
    /// Result-file rewrite rounds already asked for (once each).
    #[serde(default)]
    pub result_fixes: Vec<String>,
    /// Fix/conflict job bodies kept over a failed delivery.
    #[serde(default)]
    pub jobs: Vec<QueuedJob>,
}

impl DriverState {
    /// Load `.orchestrate/driver.json`; a missing file is a fresh state.
    pub fn load(orch_dir: &Path) -> DriverState {
        std::fs::read(orch_dir.join("driver.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Save it (atomically replaced, like Python's `save`).
    pub fn save(&self, orch_dir: &Path) -> Result<()> {
        let tmp = orch_dir.join("driver.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self).map_err(serde_err)?)
            .map_err(|e| crate::Error::io("write driver.json.tmp", e))?;
        std::fs::rename(&tmp, orch_dir.join("driver.json"))
            .map_err(|e| crate::Error::io("rename driver.json", e))?;
        Ok(())
    }

    /// Drop what only matters to a live driver: the waiting clock and the per-wait
    /// alert keys (a restart may alert again), keeping budget and kept jobs.
    pub fn prune_stale(&mut self) {
        self.waiting_since = None;
        self.notified.clear();
        self.nudged.clear();
    }

    fn ready_ts(&self, task: &str) -> Option<i64> {
        self.ready_ts.iter().find(|(t, _)| t == task).map(|(_, ts)| *ts)
    }

    fn set_ready_ts(&mut self, task: &str, ts: i64) {
        if self.ready_ts(task).is_none() {
            self.ready_ts.push((task.to_string(), ts));
        }
    }
}

fn serde_err(e: serde_json::Error) -> crate::Error {
    crate::Error::io("serialize driver state", std::io::Error::other(e))
}

/// One pass's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cycle {
    /// Keep going.
    Continue,
    /// Stop with this exit code.
    Exit(i32),
}

/// The event manager over its dependencies. Construct with [`Manager::new`], run
/// with [`Manager::run`] (the loop) or [`Manager::cycle`] (one pass, for tests).
pub struct Manager<'a> {
    /// The main repository root (`.orch-task/` and `.orchestrate/` live here).
    pub root: PathBuf,
    pub store: &'a mut Store,
    /// The narrow runtime view (§4): agent statuses and recent output.
    pub runtime: &'a dyn RuntimeView,
    /// Whether the runtime can watch an agent (the plan's `detect` field).
    pub watchable: &'a dyn Fn(&str) -> bool,
    pub poll: &'a mut dyn PollSource,
    pub work: &'a mut dyn Work,
    pub lead: &'a dyn LeadRuntime,
    pub notifier: &'a dyn Notifier,
    pub clock: &'a dyn Clock,
    /// The crash-safe prompt delivery (§15, AC3) behind [`Deliveries`].
    pub deliveries: &'a mut dyn Deliveries,
    pub tun: LoopTunables,
    /// The log sink: `[HH:MM:SS] msg` to stdout and `autopilot.log`; `important`
    /// also reaches the desktop notification (Python's `say`).
    pub say: &'a mut dyn FnMut(&str, bool),
    pub state: DriverState,
    /// Set (from a signal handler's thread) when the process was asked to end. It is not the
    /// developer's `orch stop`: nothing is written to `run_control`, the loop only finishes
    /// the step in hand, starts nothing new and returns, so the next start carries on.
    pub interrupt: Arc<AtomicBool>,
    last_tick: i64,
    /// Keys of events applied in this process; the durable dedup is the task
    /// states themselves.
    applied: HashSet<String>,
}

impl<'a> Manager<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        root: PathBuf,
        store: &'a mut Store,
        runtime: &'a dyn RuntimeView,
        watchable: &'a dyn Fn(&str) -> bool,
        poll: &'a mut dyn PollSource,
        work: &'a mut dyn Work,
        lead: &'a dyn LeadRuntime,
        notifier: &'a dyn Notifier,
        clock: &'a dyn Clock,
        deliveries: &'a mut dyn Deliveries,
        tun: LoopTunables,
        say: &'a mut dyn FnMut(&str, bool),
    ) -> Result<Self> {
        let orch = root.join(crate::paths::ORCH_DIR);
        std::fs::create_dir_all(&orch).map_err(|e| crate::Error::io("create .orchestrate", e))?;
        Ok(Manager {
            state: DriverState::load(&orch),
            root,
            store,
            runtime,
            watchable,
            poll,
            work,
            lead,
            notifier,
            clock,
            deliveries,
            tun,
            say,
            interrupt: Arc::new(AtomicBool::new(false)),
            last_tick: clock.now(),
            applied: HashSet::new(),
        })
    }

    fn log(&mut self, msg: &str, important: bool) {
        (self.say)(msg, important);
    }

    fn now(&self) -> i64 {
        self.clock.now()
    }

    fn interrupted(&self) -> bool {
        self.interrupt.load(Ordering::SeqCst)
    }

    /// Add the driving time since the last tick to the budget and save it. Waiting for an
    /// answer and a pause do not count.
    fn account_time(&mut self) -> Result<()> {
        let now = self.now();
        let mut el = (now - self.last_tick) as f64;
        self.last_tick = now;
        if self.state.waiting_since.is_some() || self.paused() {
            el = 0.0;
        }
        self.state.driven_s += el;
        self.state.save(&self.root.join(crate::paths::ORCH_DIR))
    }

    /// A stop signal ended the loop: the step in hand is finished, the time driven is
    /// saved, and the shutdown is written to the log and `events.jsonl` so the next start
    /// knows it is a resume. The run is kept as it is (no `run_control` write).
    fn shutdown(&mut self) -> i32 {
        if let Err(e) = self.account_time() {
            self.log(&format!("could not save driver state: {e}"), true);
        }
        append_lifecycle_event(&self.root, "autopilot-shutdown", self.now());
        self.log(
            "shutdown on a stop signal — the step in hand is done, the run is kept; `orch autopilot` carries on where it stopped",
            true,
        );
        EXIT_ALL_DONE
    }

    fn wake_env(&self) -> WakeEnv<'a> {
        WakeEnv {
            runtime: self.lead,
            notifier: self.notifier,
            clock: self.clock,
            tun: WakeTunables {
                confirm_s: self.tun.lead_confirm_s,
                confirm_max: self.tun.lead_confirm_wakes,
                rewake_s: self.tun.lead_rewake_s,
                max_wakes: self.tun.lead_max_wakes,
            },
        }
    }

    fn task_dir(&self, task: &str) -> PathBuf {
        self.root.join(".orch-task").join(task)
    }

    fn settings(&self) -> Result<Option<crate::run_store::RunSettings>> {
        Ok(self.store.run_settings()?)
    }

    fn lead_on(&self) -> bool {
        self.store.lead_on().unwrap_or(false)
    }

    /// Start the loop after reconciling whatever a crash left half-done. `stop` is
    /// checked every cycle (`orch autopilot --once` stops after one). A stop signal sets
    /// [`interrupt`](Self::interrupt): the step in hand finishes, nothing new starts, and `run`
    /// returns 0 so the caller frees the lease. A hard kill is also safe: every state write is
    /// one transaction, and the merge journal and reconcile bring back the rest on the next start.
    pub fn run(&mut self, stop: &dyn Fn() -> bool) -> i32 {
        let resumed = last_lifecycle_event(&self.root).as_deref() == Some("autopilot-shutdown");
        append_lifecycle_event_with(&self.root, "autopilot-started", self.now(), resumed);
        if resumed {
            self.log("resuming after a stop signal — carrying on where the run stopped", false);
        }
        // a merge a crash interrupted is finished or undone before anything is scheduled
        match crate::merge::recover(self.store, &self.root) {
            Ok(Some(r)) => self.log(&r.to_string(), true),
            Ok(None) => {}
            Err(e) => {
                self.log(&format!("merge recovery failed: {e}"), true);
                return EXIT_STOPPED;
            }
        }
        {
            let store = &mut *self.store;
            let work = &mut *self.work;
            let say = &mut *self.say;
            let say = &mut move |m: &str| say(m, false);
            let now = self.clock.now();
            let tun = self.tun;
            match crate::reconcile::reconcile(
                store,
                self.runtime,
                self.deliveries,
                self.watchable,
                work,
                &tun,
                now,
                say,
            ) {
                Ok(report) => {
                    for n in report.notes {
                        self.log(&n, false);
                    }
                }
                Err(e) => {
                    self.log(&format!("reconcile failed: {e}"), true);
                    return EXIT_STOPPED;
                }
            }
        }
        self.state.prune_stale();
        // A stop the developer asked for ended the last manager; starting a new one is the
        // developer's choice to carry on. A pause stays (only `resume` lifts it), and a stop
        // still waiting to be carried out (`Stopping`) is carried out by this manager.
        if matches!(self.store.run_control(), Ok(RunControl::Stopped)) {
            let _ = self.store.set_run_control(RunControl::Running, Source::Manager);
        }
        if let Some(code) = self.banner() {
            return code;
        }
        if let Some(code) = self.baseline_gate() {
            return code;
        }
        if let Some(code) = self.integration_gate() {
            return code;
        }
        loop {
            if self.interrupted() {
                return self.shutdown();
            }
            if stop() {
                self.log("stopped by request — the run is kept", false);
                return EXIT_ALL_DONE;
            }
            if matches!(self.store.run_control(), Ok(RunControl::Stopping)) {
                return self.stop_on_request();
            }
            match self.cycle() {
                Cycle::Continue => {}
                Cycle::Exit(code) => return code,
            }
            if self.interrupted() {
                return self.shutdown();
            }
            // 4. wait for the next event: a bounded poll of herdr and the done files
            match self.poll.poll(self.store, 30) {
                PollOutcome::Events(events) => {
                    for ev in events {
                        if let Err(e) = self.apply_event(&ev) {
                            // one task's error never stops the run (Python `guarded`)
                            self.log(&format!("{}: event not applied: {e}", ev.task()), false);
                        }
                    }
                }
                PollOutcome::Waiting { .. } => {}
            }
        }
    }

    /// The developer paused the run (`orch pause`, `POST /run/pause`).
    fn paused(&self) -> bool {
        matches!(self.store.run_control(), Ok(RunControl::Paused))
    }

    /// `orch stop`: save each running coder's work and end cleanly. Their milestones go back
    /// to `pending` (the work is kept in its hidden ref and goes to a coder again on the next
    /// start); milestones in the manager's own steps (check, review, merge) are left as they
    /// are, because they hold no coder.
    fn stop_on_request(&mut self) -> i32 {
        self.log("STOP requested — saving each running coder's work", true);
        let tasks = self.store.list_tasks().unwrap_or_default();
        for t in tasks.iter().filter(|t| t.status == State::Coding && t.worker.is_some()) {
            if let Err(e) = self.park(&t.id, State::Pending, "stopped by the developer — work saved", false) {
                self.log(&format!("{}: could not save its work: {e}", t.id), true);
            }
        }
        let _ = self.store.set_run_control(RunControl::Stopped, Source::Manager);
        let _ = self.store.enqueue_lead_event(LeadKind::Stopped, None, "the developer stopped the run");
        self.log("stopped — `orch autopilot` (or `orch dispatch`) starts it again; work in progress is kept", true);
        EXIT_ALL_DONE
    }

    /// The start banner; `Some(code)` stops the run right away (no synced run).
    fn banner(&mut self) -> Option<i32> {
        let Some(s) = self.settings().ok().flatten() else {
            self.log("no synced run — `orch sync` first", true);
            return Some(crate::error::EXIT_USAGE);
        };
        let workers: Vec<String> = self
            .store
            .load_run()
            .ok()
            .flatten()
            .map(|r| {
                r.agents
                    .iter()
                    .filter(|a| a.role == AgentRole::Worker)
                    .map(|a| match &a.model {
                        Some(m) => format!("{} ({m})", a.name),
                        None => a.name.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let budget = s
            .budget_s
            .map(|b| format!(", budget {} min", b / 60))
            .unwrap_or_default();
        let lead = if self.lead_on() {
            self.store
                .lead_name()
                .ok()
                .flatten()
                .map(|n| format!("; events go to the lead ({n})"))
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.log(
            &format!(
                "autopilot started: {} mode, max {} parallel, workers [{}]{budget}{lead}",
                s.isolation,
                s.max_parallel,
                workers.join(", ")
            ),
            false,
        );
        None
    }

    /// One pass of the loop (Python steps 0–3). Events and the poll wait live in
    /// [`run`](Self::run).
    pub fn cycle(&mut self) -> Cycle {
        // 0. budget: driving time adds up across restarts; waiting for an answer
        //    does not count.
        if let Err(e) = self.account_time() {
            self.log(&format!("could not save driver state: {e}"), true);
            return Cycle::Exit(EXIT_STOPPED);
        }
        if let Some(budget) = self.settings().ok().flatten().and_then(|s| s.budget_s)
            && self.state.driven_s > budget as f64
        {
            let _ = self.store.enqueue_lead_event(
                LeadKind::Stopped,
                None,
                &format!("the run's budget ({} min) is used up", budget / 60),
            );
            self.log(
                &format!(
                    "STOPPED — the run's budget ({} min of autopilot time) is used up. Raise `Budget:` in \
                     plan.md (or remove it) and run autopilot again; work in progress is kept.",
                    budget / 60
                ),
                true,
            );
            return Cycle::Exit(EXIT_STOPPED);
        }

        // a `gone` bench ends as soon as herdr sees the agent again (§15)
        self.end_gone_benches();

        // 1. act on tasks waiting for us (checks, reviews, commits)
        let tasks = match self.store.list_tasks() {
            Ok(t) => t,
            Err(e) => {
                self.log(&format!("could not list tasks: {e}"), true);
                return Cycle::Exit(EXIT_STOPPED);
            }
        };
        for t in tasks {
            // a stop signal: the step in hand (a check, a merge) was finished above; the
            // next milestone's step waits for the next start
            if self.interrupted() {
                return Cycle::Continue;
            }
            if let Err(e) = self.handle_in_flight(&t.id) {
                self.log(&format!("{}: {e}", t.id), false);
            }
        }
        if self.interrupted() {
            return Cycle::Continue; // no new prompt goes out after the signal
        }

        // Paused by the developer: running milestones finished their step above; nothing new
        // starts, and the run is neither "done" nor "stuck" while it waits.
        let paused = self.paused();
        if paused && self.applied.insert("run-paused".into()) {
            self.log("PAUSED — no new work starts; running milestones finish their step. `orch resume` carries on.", true);
        }
        if !paused && self.applied.remove("run-paused") {
            self.log("resumed — carrying on", false);
        }

        // 2. dispatch: the scheduler assigns queued fix jobs and new milestones
        if !paused {
            if let Err(e) = self.dispatch_pass() {
                self.log(&format!("dispatch failed: {e}"), false);
            }
            if let Err(e) = self.lead_ready() {
                self.log(&format!("lead dispatch check failed: {e}"), false);
            }
        }
        {
            let env = self.wake_env();
            let store = &mut *self.store;
            let say = &mut *self.say;
            let say = &mut move |m: &str| say(m, false);
            if let Err(e) = wake_tick(store, &env, say) {
                self.log(&format!("wake tick failed: {e}"), false);
            }
        }

        // 3. finished or stuck? (not while paused: the run waits for `orch resume`)
        if paused {
            return Cycle::Continue;
        }
        self.finish_or_stop_check()
    }

    fn end_gone_benches(&mut self) {
        let now = self.now();
        let Ok(benches) = self.store.active_benches(now) else { return };
        for b in benches.iter().filter(|b| b.reason == "gone") {
            if (self.watchable)(&b.agent)
                && self.runtime.status(&b.agent).is_some_and(|s| s != AgentState::Gone)
                && self.store.end_bench(&b.agent).is_ok()
            {
                self.log(&format!("{} is back — its bench ends", b.agent), false);
            }
        }
    }

    // ---- 1. in-flight work

    /// Drive one milestone through its waiting states: verifying (check, fix
    /// rounds), awaiting-review (the reviewer), reviewed (verdict) and committed
    /// (the merge). Errors park the task, never stop the run.
    fn handle_in_flight(&mut self, id: &str) -> Result<()> {
        let t = self.store.get_task(id)?;
        match t.status {
            State::Verifying => self.verify(id),
            State::AwaitingReview => self.send_review(id),
            State::Reviewing => Ok(()), // the REVIEWED event moves it on
            State::Reviewed => {
                let round = t.review_rounds.max(1);
                match self.work.review_verdict(id, round) {
                    Ok(ReviewOutcome::Approved) => {
                        self.log(&format!("{id} review passed"), false);
                        self.land(id)
                    }
                    Ok(ReviewOutcome::Unusable { line }) => {
                        self.park(id, State::Paused, &line, false)
                    }
                    Ok(ReviewOutcome::Blockers { count }) => self.review_blockers(id, count),
                    Err(e) => self.park(id, State::Paused, &format!("orch error: {e}"), false),
                }
            }
            State::Committed => self.land(id),
            _ => Ok(()),
        }
    }

    /// The verifying flow: the result file, the check, fix rounds, the handoff to
    /// review or the merge.
    fn verify(&mut self, id: &str) -> Result<()> {
        let t = self.store.get_task(id)?;
        let attempt = t.attempt.max(1);
        let result = self.task_dir(id).join(format!("result-{attempt}.json"));
        let report = self.task_dir(id).join(format!("report-{attempt}.md"));
        let qline: Option<String> = std::fs::read_to_string(&report)
            .ok()
            .and_then(|text| {
                text.lines()
                    .find(|l| l.trim().to_uppercase().starts_with("QUESTION:"))
                    .map(|l| l.trim()[9..].trim().to_string())
            });
        let res: Option<serde_json::Value> =
            std::fs::read(&result).ok().and_then(|b| serde_json::from_slice(&b).ok());
        match res {
            Some(ref v) => {
                if v.get("status")
                    .and_then(|s| s.as_str())
                    .is_some_and(|s| s == "question" || s == "blocked")
                {
                    let qs: Vec<String> = v
                        .get("questions")
                        .and_then(|q| q.as_array())
                        .map(|a| {
                            a.iter().filter_map(|q| q.as_str().map(String::from)).collect()
                        })
                        .unwrap_or_else(|| {
                            vec![format!("the coder is {}", v.get("status").unwrap())]
                        });
                    let joined: String = qs.join("; ").chars().take(400).collect();
                    return self.park(id, State::Paused, &joined, false);
                }
                if self.scope_gate(id)? {
                    return Ok(());
                }
                let unmet: Vec<&serde_json::Value> = v
                    .get("criteria")
                    .and_then(|c| c.as_array())
                    .map(|a| {
                        a.iter()
                            .filter(|c| c.get("met").and_then(|m| m.as_bool()) == Some(false))
                            .collect()
                    })
                    .unwrap_or_default();
                if !unmet.is_empty() {
                    let ids: Vec<String> = unmet
                        .iter()
                        .filter_map(|c| c.get("id").and_then(|i| i.as_str()))
                        .map(String::from)
                        .collect();
                    let pairs: Vec<(String, String)> = unmet
                        .iter()
                        .map(|c| {
                            let text = |k: &str| c.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                            (text("id"), text("evidence"))
                        })
                        .collect();
                    self.log(
                        &format!("{id}: the coder reports {} not met -> fix round", ids.join(", ")),
                        false,
                    );
                    return self.fix_round(
                        id,
                        &format!("acceptance criteria still not met: {}", ids.join(", ")),
                        FixKind::Criteria { unmet: pairs },
                    );
                }
            }
            None if result.exists() => {
                // the result file is there but unusable: ask the coder once to
                // rewrite it
                let key = format!("{id}:result-fix");
                if !self.state.result_fixes.contains(&key) {
                    self.state.result_fixes.push(key);
                    self.log(
                        &format!("{id}: result file not usable -> asked the coder once to rewrite it"),
                        false,
                    );
                    let body = match self.work.fix_prompt(id, 1, &FixKind::ResultFile) {
                        Ok(b) => b,
                        Err(e) => return self.park(id, State::Paused, &format!("orch error: {e}"), false),
                    };
                    return self.send_fix_file(id, "result-fix-1.md", &body);
                }
                return self.park(
                    id,
                    State::Paused,
                    &format!("result file still not usable: {}", result.display()),
                    false,
                );
            }
            None => {
                // no result file yet (or a QUESTION in the report): the report's
                // question parks the task, otherwise the check still runs
                if let Some(q) = qline {
                    return self.park(id, State::Paused, &q, false);
                }
                if self.scope_gate(id)? {
                    return Ok(());
                }
            }
        }
        let outcome = match self.work.check(id) {
            Ok(o) => o,
            Err(e) => return self.park(id, State::Paused, &format!("orch error: {e}"), false),
        };
        match outcome {
            CheckOutcome::Fail { tail } => {
                self.log(&format!("{id} checks failed -> fix round"), false);
                self.fix_round(
                    id,
                    &format!(
                        "checks still failing: {}",
                        tail.lines().next().unwrap_or("no output")
                    ),
                    FixKind::Check { tail },
                )
            }
            CheckOutcome::Pass => {
                let wants_review = self
                    .store
                    .load_run()?
                    .and_then(|r| r.plan.iter().find(|p| p.id == id).map(|p| p.review))
                    .unwrap_or(false);
                if wants_review && t.review_rounds == 0 {
                    // save the work, free the coder (review pipelining, M2), queue review
                    if let Err(e) = self.work.save_work(id) {
                        return self.park(id, State::Paused, &format!("orch error: {e}"), false);
                    }
                    self.store.release_claim(id).ok();
                    self.store
                        .transition(id, &TransitionRequest::auto(State::AwaitingReview, None))?;
                    self.log(
                        &format!(
                            "{id} saved for review; {} is free for other work",
                            t.worker.unwrap_or_default()
                        ),
                        false,
                    );
                    Ok(())
                } else {
                    self.land(id)
                }
            }
        }
    }

    /// The scope check: the coder's real diff against `Likely files`. Strays are kept in the
    /// milestone's record for the summary. `Scope: warn` says so and carries on (`false`);
    /// `Scope: strict` sends the milestone back for the strays not already complained about
    /// (`true`: handled, stop verifying). A check that cannot be made never blocks the milestone.
    fn scope_gate(&mut self, id: &str) -> Result<bool> {
        let out = match self.work.strays(id) {
            Ok(o) => o,
            Err(e) => {
                self.log(&format!("{id}: could not check its files against Likely files: {e}"), false);
                return Ok(false);
            }
        };
        let previous = Records::load(&self.root).0.get(id).map(|r| r.out_of_scope.clone()).unwrap_or_default();
        let fresh: Vec<String> = out.iter().filter(|f| !previous.contains(f)).cloned().collect();
        if !out.is_empty() {
            Records::set_out_of_scope(&self.root, id, &out)?;
        }
        if fresh.is_empty() {
            return Ok(false);
        }
        let strict = self.settings()?.is_some_and(|s| s.scope == "strict");
        let joined = fresh.join(", ");
        if !strict {
            let hot: Vec<&str> = fresh.iter().map(String::as_str).filter(|f| crate::scope::is_hotspot(f)).collect();
            let extra = if hot.is_empty() {
                String::new()
            } else {
                format!(" (shared hotspot files: {})", hot.join(", "))
            };
            self.log(&format!("{id} changed files outside its Likely files: {joined}{extra}"), !hot.is_empty());
            return Ok(false);
        }
        self.log(&format!("{id} changed files outside its Likely files: {joined} -> fix round"), false);
        let allowed = self
            .store
            .load_run()?
            .and_then(|r| r.plan.into_iter().find(|p| p.id == id).map(|p| p.files))
            .unwrap_or_default();
        self.fix_round(
            id,
            &format!("still outside its scope after {MAX_FIX} fix rounds: {joined}"),
            FixKind::Scope { allowed, files: fresh },
        )?;
        Ok(true)
    }

    /// A run branch marked broken (its final check failed): check it before anything else. Still
    /// red stops the run (exit 3) with the failing lines; green clears the mark and carries on.
    fn integration_gate(&mut self) -> Option<i32> {
        let broken = match self.store.integration_broken() {
            Ok(Some(b)) => b,
            Ok(None) => return None,
            Err(e) => {
                self.log(&format!("could not read the broken-branch mark: {e}"), true);
                return Some(EXIT_STOPPED);
            }
        };
        let branch = self.settings().ok().flatten().map_or_else(String::new, |s| s.branch);
        let (ok, tail) = match self.work.baseline() {
            Ok(r) => r,
            Err(e) => (false, format!("orch error: {e}")),
        };
        if !ok {
            let msg = format!(
                "{branch} still fails its check (broken since {}). Fix it, then run autopilot again. Stopping.",
                broken.reason
            );
            let _ = self.store.enqueue_lead_event(LeadKind::Stopped, None, &msg);
            self.log(&format!("{msg}\n{tail}"), true);
            return Some(EXIT_STOPPED);
        }
        if let Err(e) = self.store.clear_integration_broken() {
            self.log(&format!("could not clear the broken-branch mark: {e}"), true);
            return Some(EXIT_STOPPED);
        }
        self.log(&format!("{branch} passes its check again — carrying on"), true);
        None
    }

    /// The baseline gate (Python's start of `cmd_autopilot`): with no `.orchestrate/baseline.txt`,
    /// run the check on the run branch before any work. Green writes the file (its output tail);
    /// red stops the run, leaving no file, so the next start runs it again. `Some(exit code)`
    /// stops the run.
    fn baseline_gate(&mut self) -> Option<i32> {
        let file = self.root.join(crate::paths::ORCH_DIR).join("baseline.txt");
        if file.exists() {
            return None;
        }
        let (ok, tail) = match self.work.baseline() {
            Ok(r) => r,
            Err(e) => (false, format!("orch error: {e}")),
        };
        if ok {
            if let Err(e) = std::fs::write(&file, &tail) {
                self.log(&format!("could not write baseline.txt: {e}"), true);
                return Some(EXIT_STOPPED);
            }
            return None;
        }
        let msg = format!(
            "Baseline check fails on the feature branch before any work. Autopilot needs a green check: fix \
             the failures, or narrow the plan's `Check command` to what should pass (ask the lead: \
             `orch lead run` helps with this), then run autopilot again. Stopping.\n{tail}"
        );
        let _ = std::fs::remove_file(&file);
        let _ = self.store.enqueue_lead_event(LeadKind::Stopped, None, msg.lines().next().unwrap_or_default());
        self.log(&msg, true);
        Some(EXIT_STOPPED)
    }

    /// Count a fix round (capped at `MAX_FIX`), then queue a fix job the scheduler
    /// hands to a coder — preferably the builder (M2).
    fn fix_round(&mut self, id: &str, cap_reason: &str, kind: FixKind) -> Result<()> {
        let t = self.store.get_task(id)?;
        if t.fix_rounds >= MAX_FIX {
            return self.out_of_rounds(id, cap_reason);
        }
        let n = self.store.bump(id, Counter::Fix)?;
        let body = match self.work.fix_prompt(id, n, &kind) {
            Ok(b) => b,
            Err(e) => return self.park(id, State::Paused, &format!("orch error: {e}"), false),
        };
        // the fix job goes back out through the scheduler, which only hands jobs to tasks
        // that hold no coder (the builder is preferred through `last_worker`)
        self.store.release_claim(id).ok();
        self.store.transition(
            id,
            &TransitionRequest::auto(State::NeedsFix, Some(&format!("fix round {n}"))),
        )?;
        self.state.jobs.push(QueuedJob {
            task: id.to_string(),
            file: format!("{}-{n}.md", crate::fix::file_prefix(&kind)),
            body,
        });
        self.state.save(&self.root.join(crate::paths::ORCH_DIR)).ok();
        Ok(())
    }

    /// A capped task: one fresh round on a stronger model when the team has one
    /// (M2's escalate, exactly once), else failed with its history.
    fn out_of_rounds(&mut self, id: &str, reason: &str) -> Result<()> {
        let t = self.store.get_task(id)?;
        let snap = self
            .store
            .scheduler_snapshot()?
            .ok_or_else(|| crate::Error::wrap(TaskNotFound(id.to_string()), crate::error::EXIT_ERROR))?;
        let team: Vec<Option<&str>> = snap.workers.iter().map(|w| w.tier.as_deref()).collect();
        let cur = t
            .worker
            .as_deref()
            .or(t.last_worker.as_deref())
            .and_then(|w| snap.workers.iter().find(|x| x.name == w))
            .and_then(|w| w.tier.as_deref());
        let need = scheduler::need_rank(
            snap.tasks.iter().find(|x| x.task.id == id).and_then(|x| x.tier.as_deref()),
            None,
        );
        match scheduler::escalate(t.escalated, cur, need, &team) {
            Escalation::ToTier(tier) => {
                self.store.mark_escalated(id)?;
                let mut intent = self.store.intent(id)?.unwrap_or_default();
                intent.go = true;
                intent.min_tier = Some(tier.to_string());
                intent.assign = None;
                self.store.set_intent(id, &intent)?;
                let why = format!("escalated to a {tier}-tier model: {reason}");
                self.store.transition(id, &TransitionRequest::auto(State::Pending, Some(&why)))?;
                let _ = self.store.enqueue_lead_event(LeadKind::Escalated, Some(id), &why);
                self.log(&format!("{id}: {reason} -> {why}, with the work so far"), true);
                Ok(())
            }
            Escalation::NoEscalation => {
                let why = format!("{reason} — out of rounds");
                self.park(id, State::Failed, &why, false)
            }
        }
    }

    /// Send a fix body that is already written (the result-file rewrite): record,
    /// deliver, and on failure keep it queued (AC4).
    fn send_fix_file(&mut self, id: &str, file: &str, body: &str) -> Result<()> {
        let dir = self.task_dir(id);
        std::fs::create_dir_all(&dir).map_err(|e| crate::Error::io("create .orch-task dir", e))?;
        std::fs::write(dir.join(file), body).map_err(|e| crate::Error::io("write fix file", e))?;
        match self.send_prompt(id, file, DeliveryKind::Coder) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.state.jobs.push(QueuedJob {
                    task: id.to_string(),
                    file: file.to_string(),
                    body: body.to_string(),
                });
                self.bench_after_delivery_failure(&self.coder_of(id)?, id, &e)?;
                self.park_after_delivery_failure(id, Job::Fix, &e)
            }
        }
    }

    fn coder_of(&self, id: &str) -> Result<String> {
        Ok(self.store.get_task(id)?.worker.unwrap_or_default())
    }

    /// The reviewer flow from `awaiting-review`: build the review prompt (Phase 7/8
    /// material), deliver it, and move to `reviewing`.
    fn send_review(&mut self, id: &str) -> Result<()> {
        let t = self.store.get_task(id)?;
        let round = (t.review_rounds + 1).min(MAX_REVIEW.max(1));
        if let Some(why) = self.reviewer_problem(id)? {
            return self.park(id, State::Paused, &why, false);
        }
        // a benched reviewer (gone, unreachable) is not prompted: the review only waits
        let reviewer = self.reviewer_of()?;
        let now = self.now();
        if self.store.active_benches(now)?.iter().any(|b| b.agent == reviewer) {
            return Ok(());
        }
        let body = match self.work.review_body(id, round) {
            Ok(b) => b,
            Err(e) => return self.park(id, State::Paused, &format!("orch error: {e}"), false),
        };
        let dir = self.root.join(crate::paths::ORCH_DIR).join("review").join(id);
        std::fs::create_dir_all(&dir).map_err(|e| crate::Error::io("create review dir", e))?;
        std::fs::write(dir.join(format!("review-task-{round}.md")), &body)
            .map_err(|e| crate::Error::io("write review prompt", e))?;
        self.store.bump(id, Counter::Review)?;
        self.store.transition(
            id,
            &TransitionRequest::auto(State::Reviewing, Some(&format!("review round {round}"))),
        )?;
        if let Err(e) = self.send_prompt(id, &format!("review-task-{round}.md"), DeliveryKind::Reviewer) {
            // the reviewer could not be prompted: back to awaiting-review; the
            // failure streak benches it (AC4)
            self.store
                .transition(id, &TransitionRequest::auto(State::AwaitingReview, Some("reviewer unreachable")))?;
            self.store.drop_review_round(id)?;
            let reviewer = self.reviewer_of()?;
            self.bench_after_delivery_failure(&reviewer, id, &e)?;
        }
        Ok(())
    }

    /// Whether the milestone can be reviewed at all. A reviewer that is missing, or is the very
    /// coder who wrote the milestone, parks it with the reason (never a silent skip); a reviewer
    /// of a lower tier than the coder's is said in the log and the review goes ahead.
    fn reviewer_problem(&mut self, id: &str) -> Result<Option<String>> {
        let Some(run) = self.store.load_run()? else {
            return Ok(None);
        };
        let Some(reviewer) = run.agents.iter().find(|a| a.role == AgentRole::Reviewer) else {
            return Ok(Some(format!("{id} needs a review (`Review: yes`) but the team has no reviewer")));
        };
        let t = self.store.get_task(id)?;
        let coder_name = t.worker.as_deref().or(t.last_worker.as_deref());
        if coder_name == Some(reviewer.name.as_str()) {
            return Ok(Some(format!(
                "{id}: the reviewer {} is the coder who wrote it — a milestone is never reviewed by its own coder",
                reviewer.name
            )));
        }
        let coder_tier = coder_name
            .and_then(|c| run.agents.iter().find(|a| a.name == c))
            .and_then(|a| a.tier.clone());
        let (rr, cr) = (scheduler::rank(reviewer.tier.as_deref()), scheduler::rank(coder_tier.as_deref()));
        if rr > 0 && cr > 0 && rr < cr {
            self.log(
                &format!(
                    "{id}: the reviewer {} ({}-tier) is weaker than its coder's tier ({}) — reviewing anyway",
                    reviewer.name,
                    reviewer.tier.as_deref().unwrap_or("?"),
                    coder_tier.as_deref().unwrap_or("?")
                ),
                true,
            );
        }
        Ok(None)
    }

    fn reviewer_of(&self) -> Result<String> {
        Ok(self
            .store
            .load_run()?
            .and_then(|r| {
                r.agents
                    .iter()
                    .find(|a| a.role == AgentRole::Reviewer)
                    .map(|a| a.name.clone())
            })
            .unwrap_or_default())
    }

    /// Reviewer blockers: count a review round (capped), then queue a fix job.
    fn review_blockers(&mut self, id: &str, count: usize) -> Result<()> {
        let t = self.store.get_task(id)?;
        if t.review_rounds >= MAX_REVIEW {
            return self.out_of_rounds(
                id,
                &format!("reviewer still has blockers after {MAX_REVIEW} rounds"),
            );
        }
        self.log(&format!("{id} review: {count} blocker(s) -> fix job queued"), false);
        let round = t.review_rounds.max(1);
        let lines = self.work.blocker_lines(id, round).unwrap_or_default();
        let body = if lines.is_empty() {
            None
        } else {
            match self.work.fix_prompt(id, round, &FixKind::Blockers { round, lines }) {
                Ok(b) => Some(b),
                Err(e) => return self.park(id, State::Paused, &format!("orch error: {e}"), false),
            }
        };
        self.store.transition(
            id,
            &TransitionRequest::auto(State::NeedsFix, Some(&format!("{count} review blocker(s)"))),
        )?;
        if let Some(body) = body {
            self.state.jobs.retain(|j| j.task != id);
            self.state.jobs.push(QueuedJob { task: id.to_string(), file: format!("review-fix-{round}.md"), body });
            self.state.save(&self.root.join(crate::paths::ORCH_DIR)).ok();
        }
        Ok(())
    }

    /// The merge (Phase 7 through the trait); `held` waits for `orch approve`.
    fn land(&mut self, id: &str) -> Result<()> {
        let outcome = match self.work.commit(id) {
            Ok(o) => o,
            Err(e) => return self.park(id, State::Paused, &format!("orch error: {e}"), false),
        };
        match outcome {
            CommitOutcome::Merged => {
                self.store.release_claim(id).ok();
                self.store.transition(id, &TransitionRequest::auto(State::Merged, None))?;
                self.log(&format!("{id} checked and merged"), false);
                Ok(())
            }
            CommitOutcome::Held { line } => {
                self.store.transition(id, &TransitionRequest::auto(State::Held, Some(&line)))?;
                self.log(
                    &format!(
                        "{line}. Look at it, then run `orch approve {id}`; autopilot keeps going with the rest"
                    ),
                    true,
                );
                Ok(())
            }
            CommitOutcome::BranchFails { line } => self.integration_failed(id, &line),
            CommitOutcome::Conflict { line } => {
                // a conflict line can run to many lines: the transition keeps the first
                let line = line.lines().next().unwrap_or_default().to_string();
                let t = self.store.get_task(id)?;
                if t.conflict_rounds >= MAX_CONFLICT {
                    return self.park(
                        id,
                        State::Failed,
                        &format!("merge conflict still unresolved after {MAX_CONFLICT} attempts"),
                        false,
                    );
                }
                let n = self.store.bump(id, Counter::Conflict)?;
                self.store.release_claim(id).ok();
                self.store.transition(
                    id,
                    &TransitionRequest::auto(
                        State::Conflict,
                        Some(&format!("{line} (conflict round {n})")),
                    ),
                )?;
                self.queue_conflict_job(id, n);
                self.log(&format!("{line} -> queued for the next free coder"), false);
                Ok(())
            }
        }
    }

    /// The squash merged cleanly but the check fails on the run branch (Python's
    /// `_integration_failed`): nothing was committed. The round counts against the milestone's
    /// fix rounds; the job goes to its own coder. After `MAX_FIX` rounds it fails and the lead is
    /// woken (no escalation: a stronger model does not change what the merge broke).
    fn integration_failed(&mut self, id: &str, line: &str) -> Result<()> {
        let Some(s) = self.settings()? else {
            return self.park(id, State::Paused, "orch error: no synced run", false);
        };
        let n = self.store.bump(id, Counter::Fix)?;
        // the tail merge.rs stored for the coder's job (the outcome's line carries it too)
        let tail = match self.store.issue(id)? {
            Some(i) if i.kind == IssueKind::Check => i.detail,
            _ => line.split_once('\n').map_or(String::new(), |(_, t)| t.to_string()),
        };
        if n > MAX_FIX {
            let reason = format!("passes alone but fails the check on {} after {MAX_FIX} fix rounds", s.branch);
            self.park(id, State::Failed, &reason, false)?;
            self.log(
                &format!("CAP {id}: still fails the check on {} after {MAX_FIX} fix rounds\n{tail}", s.branch),
                true,
            );
            return Ok(());
        }
        let attempt = self.store.get_task(id)?.attempt;
        let body = crate::integration::job_body(
            id,
            &s.branch,
            &s.check_command,
            n,
            &tail,
            &format!("result-{}.json", attempt + 1),
        );
        self.store.release_claim(id).ok();
        self.store.transition(
            id,
            &TransitionRequest::auto(
                State::NeedsFix,
                Some(&format!("fails the check on {} once merged (round {n})", s.branch)),
            ),
        )?;
        self.state.jobs.retain(|j| j.task != id);
        self.state.jobs.push(QueuedJob { task: id.to_string(), file: crate::integration::job_file(n), body });
        self.state.save(&self.root.join(crate::paths::ORCH_DIR)).ok();
        self.log(
            &format!(
                "{id} passes alone but fails the check on {} once merged -> integration-fix round {n} for its coder\n{tail}",
                s.branch
            ),
            true,
        );
        Ok(())
    }

    /// Queue the conflict job (`conflict-<n>.md`) for the milestone's coder: the stored conflict
    /// files, and the plan's check command to re-run.
    fn queue_conflict_job(&mut self, id: &str, n: u32) {
        let (Ok(Some(s)), Ok(t)) = (self.store.run_settings(), self.store.get_task(id)) else {
            return;
        };
        let files = self.store.issue(id).ok().flatten().map(|i| i.files).unwrap_or_default();
        self.state.jobs.push(QueuedJob {
            task: id.to_string(),
            file: format!("conflict-{n}.md"),
            body: crate::conflict::job_body(id, &t.title, &s.branch, &s.check_command, &files),
        });
        self.state.save(&self.root.join(crate::paths::ORCH_DIR)).ok();
    }

    // ---- 2. dispatch (the scheduler, M2)

    /// One scheduling pass: assignments to free coders, fix and conflict jobs first,
    /// new milestones after — the pure scheduler decides; here we prepare, claim and
    /// deliver.
    fn dispatch_pass(&mut self) -> Result<()> {
        let Some(snap) = self.store.scheduler_snapshot()? else {
            return Ok(()); // no synced run yet
        };
        let out = scheduler::schedule(&snap, self.now());
        for a in &out.assignments {
            if let Err(e) = self.dispatch_one(a) {
                self.log(&format!("{}: {e}", a.task), false);
            }
        }
        Ok(())
    }

    fn dispatch_one(&mut self, a: &Assignment) -> Result<()> {
        // a dispatch error pauses this milestone; the others carry on
        if let Err(e) = self.work.prepare(&a.task, &a.worker).and_then(|()| self.work.write_task(&a.task)) {
            let first = e.lines().find(|l| !l.trim().is_empty()).unwrap_or(&e).trim().to_string();
            return self.park(
                &a.task,
                State::Paused,
                &format!("orch error: {}", &first[..first.len().min(300)]),
                false,
            );
        }
        // the attempt, then the atomic claim
        self.store.bump(&a.task, Counter::Attempt)?;
        match self.store.claim(&a.task, &a.worker, None) {
            Ok(()) => {}
            Err(ClaimError::WorkerBusy { .. }) => return Ok(()), // taken meanwhile: next pass
            Err(e) => return Err(crate::Error::wrap(e, crate::error::EXIT_ERROR)),
        }
        // the prompt: the milestone, or the kept fix job
        let job = self
            .state
            .jobs
            .iter()
            .position(|j| j.task == a.task)
            .map(|i| self.state.jobs.remove(i));
        if let Some(j) = &job
            && let Err(e) = self.write_job(&a.task, j)
        {
            self.state.jobs.push(j.clone());
            self.state.save(&self.root.join(crate::paths::ORCH_DIR)).ok();
            return self.park(&a.task, State::Paused, &format!("orch error: {e}"), false);
        }
        self.mark_dispatched(&a.task)?;
        let who = a.worker.clone();
        let file = job.as_ref().map_or("task.md", |j| j.file.as_str());
        match self.send_prompt(&a.task, file, DeliveryKind::Coder) {
            Ok(()) => {
                if a.job == Job::Fix {
                    self.log(&format!("{} fix job -> {who}", a.task), false);
                } else {
                    self.log(&format!("{} -> {who}", a.task), false);
                }
                self.state.save(&self.root.join(crate::paths::ORCH_DIR)).ok();
                Ok(())
            }
            Err(e) => {
                // AC4: back to the queue with its job kept, the agent benched
                if let Some(j) = job {
                    self.state.jobs.push(j);
                }
                self.state.save(&self.root.join(crate::paths::ORCH_DIR)).ok();
                self.bench_after_delivery_failure(&who, &a.task, &e)?;
                self.park_after_delivery_failure(&a.task, a.job, &e)
            }
        }
    }

    /// Put a job's body where the coder reads it (the `Work`'s choice of folder).
    fn write_job(&mut self, task: &str, job: &QueuedJob) -> std::result::Result<(), String> {
        self.work.write_job(&self.root, task, &job.file, &job.body)
    }

    fn mark_dispatched(&mut self, task: &str) -> Result<()> {
        if self.lead_on() {
            self.store.mark_dispatched(task)?;
        }
        Ok(())
    }

    /// Record the prompt durably, deliver it, confirm it (§15; AC3). The prompt
    /// text is Python `cmd_send`'s, with the milestone's attempt as the suffix.
    fn send_prompt(&mut self, task: &str, file: &str, kind: DeliveryKind) -> std::result::Result<(), String> {
        let t = self.store.get_task(task).map_err(|e| e.to_string())?;
        let attempt = t.attempt.max(1);
        let text = match kind {
            DeliveryKind::Coder => coder_prompt(task, file, attempt),
            DeliveryKind::Reviewer => crate::review::reviewer_message(task, t.review_rounds.max(1)),
        };
        let agent = match kind {
            DeliveryKind::Coder => t.worker.clone().unwrap_or_default(),
            DeliveryKind::Reviewer => self.reviewer_of().map_err(|e| e.to_string())?,
        };
        if agent.is_empty() {
            return Err(format!("{task} has no agent to prompt"));
        }
        match self
            .deliveries
            .send(self.store, NewDelivery { agent: &agent, task, attempt, kind, prompt: &text })
        {
            Ok(DeliveryOutcome::Sent { .. }) => Ok(()),
            Ok(DeliveryOutcome::Failed { error, .. }) => Err(error),
            Err(e) => Err(e.to_string()),
        }
    }

    // ---- lead mode (AC6)

    /// Ready milestones the lead has not dispatched: tell it once per set, and
    /// start them ourselves after `ORCH_LEAD_WAIT_MIN`.
    fn lead_ready(&mut self) -> Result<()> {
        if !self.lead_on() {
            return Ok(());
        }
        let hold = self.store.undispatched()?;
        if hold.is_empty() {
            self.state.lead_ready_told.clear();
            return Ok(());
        }
        let now = self.now();
        for k in &hold {
            self.state.set_ready_ts(k, now);
        }
        let late: Vec<String> = hold
            .iter()
            .filter(|k| now - self.state.ready_ts(k).unwrap_or(now) > self.tun.lead_wait_s)
            .cloned()
            .collect();
        for k in &late {
            let mut intent = self.store.intent(k)?.unwrap_or_default();
            intent.go = true;
            self.store.set_intent(k, &intent)?;
            self.store.mark_dispatched(k)?;
            let why = format!("not dispatched by the lead within {} min", self.tun.lead_wait_s / 60);
            let _ = self.store.enqueue_lead_event(LeadKind::AutoDispatched, Some(k), &why);
            self.log(&format!("{k}: the lead did not dispatch it in time -> started without it"), true);
        }
        let rest: Vec<String> = hold.iter().filter(|k| !late.contains(k)).cloned().collect();
        if !rest.is_empty() && rest != self.state.lead_ready_told {
            let _ = self.store.enqueue_lead_event(LeadKind::Ready, None, &rest.join(", "));
        }
        self.state.lead_ready_told = rest;
        Ok(())
    }

    // ---- 3. finished or stuck

    fn finish_or_stop_check(&mut self) -> Cycle {
        let tasks = match self.store.list_tasks() {
            Ok(t) => t,
            Err(_) => return Cycle::Continue,
        };
        if !tasks.is_empty() && tasks.iter().all(|t| matches!(t.status, State::Merged | State::Skipped)) {
            let (ok, tail) = match self.work.check("(final check)") {
                Ok(CheckOutcome::Pass) => (true, String::new()),
                Ok(CheckOutcome::Fail { tail }) => (false, tail),
                Err(e) => (false, format!("orch error: {e}")),
            };
            let branch = self.settings().ok().flatten().map_or_else(String::new, |s| s.branch);
            if !ok {
                // never carry on over a red branch: mark it, tell the lead, stop
                if let Err(e) = self.store.mark_integration_broken(crate::integration::FINAL_CHECK_REASON) {
                    self.log(&format!("could not record the broken branch: {e}"), true);
                }
                let msg = format!(
                    "{branch} fails its final check after every milestone merged. Fix it, then run autopilot \
                     again: it re-checks before it carries on."
                );
                let _ = self.store.enqueue_lead_event(LeadKind::Stopped, None, &msg);
                self.log(&format!("STOPPED — {msg}\n{tail}"), true);
                return Cycle::Exit(EXIT_STOPPED);
            }
            if let Err(e) = self.store.clear_integration_broken() {
                self.log(&format!("could not clear the broken mark: {e}"), true);
            }
            let _ = self.store.enqueue_lead_event(
                LeadKind::AllDone,
                None,
                &format!("final check PASS on {branch}"),
            );
            self.log(
                &format!("ALL_DONE on {branch} — final check PASS. Run `orch finish` to push and open the PR."),
                true,
            );
            return Cycle::Exit(EXIT_ALL_DONE);
        }
        let active = tasks.iter().any(|t| {
            matches!(
                t.status,
                State::Coding
                    | State::Reviewing
                    | State::Verifying
                    | State::Committed
                    | State::AwaitingReview
                    | State::Reviewed
                    | State::NeedsFix
                    | State::Conflict
            )
        });
        let ready = tasks
            .iter()
            .any(|t| t.status == State::Pending && t.is_ready(|d| self.store.get_task(d).ok().map(|x| x.status)));
        if !active {
            let undispatched = self.lead_on() && !self.store.undispatched().unwrap_or_default().is_empty();
            let open_blocking = self
                .lead_on()
                && self
                    .store
                    .still_open_lead_events()
                    .is_ok_and(|e| !e.is_empty());
            if undispatched || (!ready && open_blocking) {
                // nothing runs and the next move is the lead's (or the developer's
                // through the lead): stay up; no AI runs while we wait
                if self.state.waiting_since.is_none() {
                    self.state.waiting_since = Some(self.now());
                    let names = self.store.undispatched().unwrap_or_default().join(", ");
                    self.log(&format!("waiting for the lead: {names}"), true);
                } else if self.now() - self.state.waiting_since.unwrap_or(self.now())
                    > self.tun.lead_idle_max_s
                {
                    let _ = self.store.enqueue_lead_event(
                        LeadKind::Stopped,
                        None,
                        "no lead activity for the idle limit",
                    );
                    self.log(
                        &format!(
                            "STOPPED — no answer for {} min. Everything is kept: run `orch autopilot` again when \
                             you are back (`orch events --peek` shows what is open).",
                            self.tun.lead_idle_max_s / 60
                        ),
                        true,
                    );
                    return Cycle::Exit(EXIT_STOPPED);
                }
                return Cycle::Continue;
            }
            if !ready {
                let waiting: Vec<String> = tasks
                    .iter()
                    .filter(|t| {
                        matches!(
                            t.status,
                            State::Paused | State::Failed | State::NeedsAnswer | State::Held
                        )
                    })
                    .map(|t| format!("  {} {}: {}", t.id, t.status.as_str(), t.reason))
                    .collect();
                let _ = self
                    .store
                    .enqueue_lead_event(LeadKind::Stopped, None, "needs a human decision");
                self.log(
                    &format!(
                        "STOPPED — needs you:\n{}\nAnswer in plan.md 'Notes from user', run `orch set <Mn> pending` \
                         for paused/failed tasks, `orch approve <Mn>` for held ones, then run `orch autopilot` \
                         again.",
                        waiting.join("\n")
                    ),
                    true,
                );
                return Cycle::Exit(EXIT_STOPPED);
            }
        } else {
            self.state.waiting_since = None;
        }
        Cycle::Continue
    }

    // ---- events (AC2)

    /// Validate and apply one runtime event: known milestone, legal transition,
    /// idempotency key — a replayed event changes nothing and is logged as a
    /// duplicate. After it, the next cycle's scheduler pass claims and sends
    /// eligible work.
    pub fn apply_event(&mut self, ev: &RunEvent) -> Result<()> {
        let key = ev.key();
        if self.store.get_task(ev.task()).is_err() {
            self.log(&format!("unknown milestone {}: event dropped", ev.task()), false);
            return Ok(());
        }
        // The same event of a later attempt or review round (the coder's DONE after a fix
        // round, a second GONE) is a new event, not a replay.
        let (attempt, round) = self
            .store
            .get_task(ev.task())
            .map(|t| (t.attempt, t.review_rounds))
            .unwrap_or_default();
        let key = format!("{key} @{attempt}.{round}");
        if self.applied.contains(&key) {
            self.log(&format!("duplicate: {key} — nothing changed"), false);
            return Ok(());
        }
        self.log(&ev.line(), false);
        let r = match ev {
            RunEvent::Done { task, .. } => {
                self.applied.insert(key.clone());
                self.transition_checked(task, State::Verifying, "done file")
            }
            RunEvent::Reviewed { task, .. } => {
                self.applied.insert(key.clone());
                self.transition_checked(task, State::Reviewed, "review done file")
            }
            RunEvent::Unconfirmed { task, .. } => {
                self.applied.insert(key.clone());
                self.resend_unconfirmed(task)
            }
            RunEvent::Orphan { task, status } => {
                self.applied.insert(key.clone());
                self.orphan(task, status)
            }
            RunEvent::Blocked { task, agent, prompt } => {
                self.applied.insert(key.clone());
                if !self.state.notified.contains(&key) {
                    self.state.notified.push(key.clone());
                    let wants = prompt
                        .as_deref()
                        .map_or_else(String::new, |p| format!(" It wants: {p}."));
                    self.log(
                        &format!(
                            "{agent} is waiting for a PERMISSION/QUESTION in its pane (task \
                             {task}).{wants} Answer it there; autopilot keeps going."
                        ),
                        true,
                    );
                    // §15A item 1: the lead hears of it with the agent and the prompt
                    let _ = self
                        .store
                        .enqueue_blocked_lead_event(task, agent, prompt.as_deref());
                }
                Ok(())
            }
            RunEvent::Idle { task, agent, .. } => {
                self.applied.insert(key.clone());
                self.idle(task, agent)
            }
            RunEvent::Timeout { task, after_min, .. } => {
                self.applied.insert(key.clone());
                self.park(
                    task,
                    State::Paused,
                    &format!("over its time budget ({after_min} min)"),
                    false,
                )
            }
            RunEvent::Gone { task, agent } => {
                self.applied.insert(key.clone());
                self.gone(task, agent)
            }
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) if e.to_string().contains("is not an allowed transition")
                || e.to_string().contains("merged is final") =>
            {
                // a replayed or stale event: no change
                self.log(&format!("duplicate or illegal: {e} — nothing changed"), false);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// A store transition whose illegality means "already there": replayed event.
    fn transition_checked(&mut self, task: &str, to: State, why: &str) -> Result<()> {
        let t = self.store.get_task(task)?;
        if t.status == to {
            self.log(&format!("{task} is already {to} — duplicate event, nothing changed"), false);
            return Ok(());
        }
        check_transition(task, t.status, to, Mode::Auto)
            .map_err(|e| crate::Error::wrap(e, crate::error::EXIT_USAGE))?;
        self.store.transition(task, &TransitionRequest::auto(to, Some(why)))?;
        Ok(())
    }

    /// The prompt was recorded but not confirmed: re-send it to the same agent
    /// (never a second coder, AC3).
    fn resend_unconfirmed(&mut self, task: &str) -> Result<()> {
        let unconfirmed = self.store.unconfirmed_deliveries()?;
        let Some(d) = unconfirmed.iter().find(|d| d.task == task) else {
            return Ok(()); // confirmed in the meantime
        };
        let id = d.id;
        match self.deliveries.resend(self.store, id) {
            Ok(DeliveryOutcome::Sent { .. }) => {
                self.log(
                    &format!(
                        "{task}: the prompt was recorded but not confirmed (interrupted run?) -> sent again to \
                         the same agent"
                    ),
                    false,
                );
                Ok(())
            }
            Ok(DeliveryOutcome::Failed { error, .. }) => {
                self.log(&format!("{task}: could not re-send: {error} — retried on the next pass"), false);
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// No prompt ever went out for an in-flight milestone (interrupted run).
    fn orphan(&mut self, task: &str, status: &str) -> Result<()> {
        match status {
            "reviewing" => {
                self.store.transition(
                    task,
                    &TransitionRequest::auto(State::AwaitingReview, Some("the review prompt never went out")),
                )?;
                self.store.drop_review_round(task)?;
                self.log(&format!("{task}: the review prompt never went out -> queued for review again"), false);
                Ok(())
            }
            "coding" => {
                self.park(
                    task,
                    State::Pending,
                    "its prompt never went out (interrupted run) — will be sent again",
                    true,
                )
            }
            other => self.park(
                task,
                State::Paused,
                &format!("{other} with no job and no agent working on it — check it and `orch set {task} pending`"),
                false,
            ),
        }
    }

    /// The agent went idle without a done file: accept a written report, bench a
    /// rate-limited coder, else nudge once and park the second time (AC5).
    fn idle(&mut self, task: &str, agent: &str) -> Result<()> {
        let t = self.store.get_task(task)?;
        let attempt = t.attempt.max(1);
        let has_report = self.task_dir(task).join(format!("report-{attempt}.md")).exists()
            || self.task_dir(task).join(format!("result-{attempt}.json")).exists();
        if has_report {
            self.log(&format!("{task}: {agent} wrote its report but no done file — accepting it"), false);
            return self.apply_event(&RunEvent::Done { task: task.into(), agent: agent.into() });
        }
        if !self.is_reviewer(agent)
            && let Some(pane) = self.runtime.output(agent, 40)
            && is_rate_limited(&pane)
        {
            self.store.bench_agent(agent, "rate-limit", self.now() + self.tun.cooldown_min * 60)?;
            self.log(&format!("{agent} benched; {task} will move to another coder with its work so far"), true);
            return self.park(task, State::Pending, &format!("{agent} hit a usage/rate limit"), true);
        }
        let key = format!("{task}:{attempt}");
        if !self.state.nudged.contains(&key) {
            self.state.nudged.push(key);
            self.log(&format!("{task}: {agent} went idle early -> nudged once"), false);
            return self.resend_last(task);
        }
        self.park(task, State::Paused, &format!("{agent} stopped twice without finishing"), true)
    }

    fn is_reviewer(&self, agent: &str) -> bool {
        self.store
            .load_run()
            .ok()
            .flatten()
            .and_then(|r| {
                r.agents
                    .iter()
                    .find(|a| a.role == AgentRole::Reviewer)
                    .map(|a| a.name == agent)
            })
            .unwrap_or(false)
    }

    /// Nudge: the prompt goes out once more, to the same agent.
    fn resend_last(&mut self, task: &str) -> Result<()> {
        let attempt = self.store.get_task(task)?.attempt.max(1);
        let unconfirmed = self.store.unconfirmed_deliveries()?;
        let id = match unconfirmed.iter().find(|d| d.task == task) {
            Some(d) => d.id,
            None => self
                .store
                .latest_delivery(task, attempt, DeliveryKind::Coder)?
                .map(|d| d.id)
                .ok_or_else(|| crate::Error::wrap(TaskNotFound(task.to_string()), crate::error::EXIT_ERROR))?,
        };
        self.deliveries.resend(self.store, id)?;
        Ok(())
    }

    /// herdr lost the agent: the coder's work goes to the next free coder, the
    /// reviewer's task waits for review again; the agent is benched (Python
    /// cooldowns, AC5).
    fn gone(&mut self, task: &str, agent: &str) -> Result<()> {
        let reviewer = self.is_reviewer(agent);
        self.store.bench_agent(agent, "gone", self.now() + self.tun.gone_cooldown_min * 60)?;
        if reviewer {
            self.store.transition(
                task,
                &TransitionRequest::auto(State::AwaitingReview, Some("the reviewer is gone")),
            )?;
            self.store.drop_review_round(task)?;
            self.log(
                &format!(
                    "reviewer {agent} is gone — restart it; {task} waits for review (the coder's work is kept)"
                ),
                true,
            );
            Ok(())
        } else {
            self.park(task, State::Pending, &format!("{agent} disappeared from herdr"), true)?;
            self.log(
                &format!("{agent} is gone — restart it in its folder; {task} goes to the next free coder"),
                true,
            );
            Ok(())
        }
    }

    // ---- parking, benching, failing (AC4, AC5)

    /// Park: free the coder (keeping `last_worker`), keep the work (Phase 7 hook),
    /// and count a lost attempt when asked — at `ORCH_MAX_LOST` the task fails
    /// with its history.
    fn park(&mut self, task: &str, status: State, reason: &str, lost: bool) -> Result<()> {
        let final_status = park_with_lost(
            self.store,
            Some(&mut *self.work),
            self.tun.max_lost,
            task,
            status,
            reason,
            lost,
        )?;
        let t = self.store.get_task(task)?;
        if final_status == State::Failed && status != State::Failed {
            self.log(&format!("{task} failed: {}", t.reason), true);
        } else {
            self.log(&format!("{task} {}: {reason}", status.as_str()), true);
        }
        Ok(())
    }

    /// A send that failed: bench the agent — `ORCH_SEND_RETRY_S` (2 min) at first,
    /// `ORCH_UNREACHABLE_MIN` (30 min) after three failures in a row — and tell
    /// the lead once benched (AC4).
    fn bench_after_delivery_failure(&mut self, agent: &str, task: &str, error: &str) -> Result<()> {
        let fails = self.deliveries.failure_streak(self.store, agent);
        let (mins, benched) = if fails >= MAX_CONSECUTIVE_FAILURES {
            (self.tun.unreachable_min, true)
        } else {
            ((self.tun.send_retry_s + 59) / 60, false)
        };
        self.store.bench_agent(agent, "unreachable", self.now() + mins * 60)?;
        if benched {
            self.log(
                &format!("BENCHED {agent} for {mins} min: {fails} prompts in a row did not reach it ({error})"),
                true,
            );
            let _ = self.store.enqueue_lead_event(
                LeadKind::Paused,
                Some(task),
                &format!("could not reach {agent} — it is benched {mins} min and the work is queued again"),
            );
        } else {
            self.log(
                &format!("{task}: could not reach {agent} ({error}) — retried after {mins} min"),
                false,
            );
        }
        Ok(())
    }

    fn park_after_delivery_failure(&mut self, task: &str, job: Job, error: &str) -> Result<()> {
        let who = self.coder_of(task)?;
        let status = match job {
            Job::New => State::Pending,
            Job::Fix => State::NeedsFix,
        };
        let reason = format!("could not prompt {who}: {error}");
        park_with_lost(self.store, Some(&mut *self.work), self.tun.max_lost, task, status, &reason, true)?;
        let t = self.store.get_task(task)?;
        if t.status == State::Failed {
            self.log(&format!("{task} failed: {}", t.reason), true);
        } else {
            self.log(&format!("DELIVERY_FAILED {task} -> {who}: {error}"), false);
        }
        Ok(())
    }
}

/// A task the store does not know (an internal inconsistency; the event validation
/// keeps events away from this).
#[derive(Debug, thiserror::Error)]
#[error("unknown milestone {0}")]
struct TaskNotFound(String);

/// Python's `RATE_RE`: an agent that hit a usage/rate limit.
fn is_rate_limited(pane: &str) -> bool {
    let lower = pane.to_lowercase();
    [
        "usage limit",
        "rate limit",
        "rate-limit",
        "quota",
        "too many requests",
        "429",
        "limit reached",
        "try again in",
        "try again at",
        "out of credits",
    ]
    .iter()
    .any(|p| lower.contains(p))
}
#[cfg(test)]
mod blocked_tests {
    use super::*;
    use crate::events::{AgentState, PollOutcome, RuntimeView};
    use crate::lead_queue::LeadKind;
    use crate::run_store::{PlanTask, RunAgent, RunSettings, SyncInput};
    use crate::store::StoreError;
    use crate::wake::LeadPane;
    use std::cell::Cell;
    use std::rc::Rc;

    // ---- minimal fakes: apply_event needs the log, the clock and the state only

    struct FkClock(Rc<Cell<i64>>);
    impl Clock for FkClock {
        fn now(&self) -> i64 {
            self.0.get()
        }
    }

    struct FkPoll;
    impl PollSource for FkPoll {
        fn poll(&mut self, _store: &mut Store, _secs: u64) -> PollOutcome {
            PollOutcome::Waiting { nothing_running: true }
        }
    }

    struct FkRuntime {
        st: std::collections::HashMap<String, AgentState>,
    }
    impl RuntimeView for FkRuntime {
        fn status(&self, agent: &str) -> Option<AgentState> {
            self.st.get(agent).copied()
        }
        fn output(&self, _agent: &str, _lines: u32) -> Option<String> {
            None
        }
    }

    struct FkWork;
    impl Work for FkWork {
        fn prepare(&mut self, _: &str, _: &str) -> std::result::Result<(), String> {
            Ok(())
        }
        fn write_task(&mut self, _: &str) -> std::result::Result<(), String> {
            Ok(())
        }
        fn fix_body(&mut self, _: &str, _: u32, _: &str) -> std::result::Result<String, String> {
            Ok(String::new())
        }
        fn fix_prompt(
            &mut self,
            _: &str,
            _: u32,
            _: &FixKind,
        ) -> std::result::Result<String, String> {
            Ok(String::new())
        }
        fn blocker_lines(
            &mut self,
            _: &str,
            _: u32,
        ) -> std::result::Result<Vec<String>, String> {
            Ok(vec![])
        }
        fn strays(&mut self, _: &str) -> std::result::Result<Vec<String>, String> {
            Ok(vec![])
        }
        fn check(&mut self, _: &str) -> std::result::Result<CheckOutcome, String> {
            Ok(CheckOutcome::Pass)
        }
        fn save_work(&mut self, _: &str) -> std::result::Result<(), String> {
            Ok(())
        }
        fn review_body(&mut self, _: &str, _: u32) -> std::result::Result<String, String> {
            Ok(String::new())
        }
        fn review_verdict(
            &mut self,
            _: &str,
            _: u32,
        ) -> std::result::Result<ReviewOutcome, String> {
            Ok(ReviewOutcome::Approved)
        }
        fn commit(&mut self, _: &str) -> std::result::Result<CommitOutcome, String> {
            Ok(CommitOutcome::Merged)
        }
        fn baseline(&mut self) -> std::result::Result<(bool, String), String> {
            Ok((true, String::new()))
        }
    }

    struct FkLead;
    impl LeadRuntime for FkLead {
        fn pane(&self, _: &str) -> LeadPane {
            LeadPane::Idle
        }
        fn send(&self, _: &str, _: &str) -> std::result::Result<(), String> {
            Ok(())
        }
    }

    struct FkNotes;
    impl Notifier for FkNotes {
        fn notify(&self, _: &str) {}
    }

    struct FkDeliveries;
    impl Deliveries for FkDeliveries {
        fn send(
            &mut self,
            store: &mut Store,
            req: NewDelivery<'_>,
        ) -> std::result::Result<DeliveryOutcome, StoreError> {
            let id = store.record_delivery(req.agent, req.task, req.attempt, req.kind, req.prompt)?;
            store.confirm_delivery(id)?;
            Ok(DeliveryOutcome::Sent { delivery: store.delivery(id)? })
        }
        fn resend(
            &mut self,
            _store: &mut Store,
            _id: i64,
        ) -> std::result::Result<DeliveryOutcome, StoreError> {
            unreachable!()
        }
        fn failure_streak(&self, _: &Store, _: &str) -> u32 {
            0
        }
        fn recover(
            &mut self,
            _: &mut Store,
            _: i64,
            _: i64,
        ) -> std::result::Result<Vec<Recovered>, StoreError> {
            Ok(vec![])
        }
    }

    fn tun() -> LoopTunables {
        LoopTunables {
            idle_grace_s: 120,
            confirm_s: 30,
            coder_budget_s: 2700,
            review_budget_s: 900,
            send_retry_s: 120,
            unreachable_min: 30,
            cooldown_min: 60,
            gone_cooldown_min: 10,
            max_lost: 4,
            lead_wait_s: 900,
            lead_idle_max_s: 43200,
            lead_confirm_s: 20,
            lead_confirm_wakes: 2,
            lead_rewake_s: 300,
            lead_max_wakes: 3,
        }
    }

    fn settings(lead: bool) -> RunSettings {
        RunSettings {
            branch: "feature/x".into(),
            base: None,
            title: "X".into(),
            commits: "per-milestone".into(),
            isolation: "worktree".into(),
            max_parallel: 2,
            setup_inputs: vec![],
            scope: "warn".into(),
            risk_hold: false,
            budget_s: None,
            check_command: String::new(),
            setup_command: "none".into(),
            lead_mode: if lead { "events".into() } else { "off".into() },
            reviewer: None,
        }
    }

    fn agent(name: &str, role: AgentRole) -> RunAgent {
        RunAgent {
            role,
            name: name.into(),
            kind: "claude".into(),
            model: None,
            tier: Some("medium".into()),
            detect: true,
            pane: None,
        }
    }

    fn task(id: &str) -> PlanTask {
        PlanTask {
            id: id.into(),
            title: format!("task {id}"),
            deps: vec![],
            enabled: true,
            open_questions: false,
            tier: Some("medium".into()),
            needs: vec![],
            model_why: String::new(),
            prefer: "any".into(),
            review: false,
            files: vec![],
            parallel: true,
        }
    }

    struct Rig {
        dir: tempfile::TempDir,
        store: Store,
        clock: FkClock,
        runtime: FkRuntime,
        log: Rc<std::cell::RefCell<Vec<String>>>,
    }

    impl Rig {
        fn new(lead: bool) -> Rig {
            let dir = tempfile::tempdir().unwrap();
            let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
            let mut agents = vec![agent("w1", AgentRole::Worker), agent("w2", AgentRole::Worker)];
            if lead {
                agents.push(agent("lead", AgentRole::Lead));
            }
            store
                .apply_sync(&SyncInput {
                    settings: settings(lead),
                    agents,
                    tasks: vec![task("M1"), task("M2")],
                })
                .unwrap();
            Rig {
                dir,
                store,
                clock: FkClock(Rc::new(Cell::new(1_000_000))),
                runtime: FkRuntime { st: std::collections::HashMap::new() },
                log: Rc::new(std::cell::RefCell::new(Vec::new())),
            }
        }

        /// Two milestones in flight, one agent each.
        fn two_in_flight(&mut self) {
            self.store.claim("M1", "w1", None).unwrap();
            self.store.claim("M2", "w2", None).unwrap();
        }

        fn with_manager<R>(&mut self, f: impl FnOnce(&mut Manager<'_>) -> R) -> R {
            let mut say = |m: &str, _: bool| self.log.borrow_mut().push(m.to_string());
            let mut poll = FkPoll;
            let mut work = FkWork;
            let mut deliveries = FkDeliveries;
            let mut m = Manager::new(
                self.dir.path().to_path_buf(),
                &mut self.store,
                &self.runtime,
                &watch_all,
                &mut poll,
                &mut work,
                &FkLead,
                &FkNotes,
                &self.clock,
                &mut deliveries,
                tun(),
                &mut say,
            )
            .unwrap();
            f(&mut m)
        }
    }

    fn watch_all(_: &str) -> bool {
        true
    }

    // AC1: the blocked event names the agent and the pane's prompt, in the log and the
    // lead's queue (what `orch events` and the API read).
    #[test]
    fn a_blocked_agent_is_reported_with_its_name_and_prompt() {
        let mut rig = Rig::new(true);
        rig.two_in_flight();
        rig.with_manager(|m| {
            m.apply_event(&RunEvent::Blocked {
                task: "M1".into(),
                agent: "w1".into(),
                prompt: Some("cargo test --workspace".into()),
            })
            .unwrap();
        });
        let log = rig.log.borrow().join("\n");
        assert!(log.contains("w1 is waiting for a PERMISSION/QUESTION"), "{log}");
        assert!(log.contains("It wants: cargo test --workspace"), "{log}");
        let e = &rig.store.unread_lead_events().unwrap()[0];
        assert_eq!(e.kind, LeadKind::Blocked);
        assert_eq!(e.task.as_deref(), Some("M1"));
        assert_eq!(e.agent.as_deref(), Some("w1"));
        assert_eq!(e.prompt.as_deref(), Some("cargo test --workspace"));
        // and the lead's queue view (what `orch events` prints) shows it as new
        let run = rig.store.load_run().unwrap().unwrap();
        let v = crate::view::events_view(&rig.store, &run, None, None).unwrap();
        assert!(v.new.iter().any(|e| e.agent.as_deref() == Some("w1")
            && e.prompt.as_deref() == Some("cargo test --workspace")));
    }

    // AC6: one agent blocked does not stop the others.
    #[test]
    fn the_other_tasks_keep_running_while_one_agent_is_blocked() {
        let mut rig = Rig::new(false);
        rig.two_in_flight();
        rig.with_manager(|m| {
            m.apply_event(&RunEvent::Blocked {
                task: "M1".into(),
                agent: "w1".into(),
                prompt: Some("rm -rf /".into()),
            })
            .unwrap();
            // the second coder's done file still moves its milestone on
            m.apply_event(&RunEvent::Done { task: "M2".into(), agent: "w2".into() })
                .unwrap();
        });
        assert_eq!(rig.store.get_task("M1").unwrap().status, State::Coding);
        assert_eq!(rig.store.get_task("M2").unwrap().status, State::Verifying);
        // and the blocked agent is not benched: it can still be prompted after an answer
        assert!(rig.store.benches().unwrap().is_empty());
    }
}
