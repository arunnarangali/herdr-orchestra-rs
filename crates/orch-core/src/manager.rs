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
use crate::events::{PollOutcome, PollSource, RunEvent, RuntimeView};
use crate::error::Result;
use crate::lead_queue::LeadKind;
use crate::reconcile::park_with_lost;
use crate::run_store::AgentRole;
use crate::scheduler::{self, Assignment, Escalation, Job};
use crate::store::{Counter, Delivery, DeliveryKind, Res, Store, TransitionRequest};
use crate::task::{check_transition, Mode, State};
use crate::wake::{wake_tick, Clock, LeadRuntime, Notifier, WakeEnv, WakeTunables};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    /// checked every cycle (`orch autopilot --once` stops after one). SIGTERM kills
    /// the process; the OS then releases the driver lease, and every state write is
    /// one transaction, so nothing is corrupted.
    pub fn run(&mut self, stop: &dyn Fn() -> bool) -> i32 {
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
        if let Some(code) = self.banner() {
            return code;
        }
        loop {
            if stop() {
                self.log("stopped by request — the run is kept", false);
                return EXIT_ALL_DONE;
            }
            match self.cycle() {
                Cycle::Continue => {}
                Cycle::Exit(code) => return code,
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
        let now = self.now();
        let mut el = (now - self.last_tick) as f64;
        self.last_tick = now;
        if self.state.waiting_since.is_some() {
            el = 0.0;
        }
        self.state.driven_s += el;
        let orch = self.root.join(crate::paths::ORCH_DIR);
        if let Err(e) = self.state.save(&orch) {
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

        // 1. act on tasks waiting for us (checks, reviews, commits)
        let tasks = match self.store.list_tasks() {
            Ok(t) => t,
            Err(e) => {
                self.log(&format!("could not list tasks: {e}"), true);
                return Cycle::Exit(EXIT_STOPPED);
            }
        };
        for t in tasks {
            if let Err(e) = self.handle_in_flight(&t.id) {
                self.log(&format!("{}: {e}", t.id), false);
            }
        }

        // 2. dispatch: the scheduler assigns queued fix jobs and new milestones
        if let Err(e) = self.dispatch_pass() {
            self.log(&format!("dispatch failed: {e}"), false);
        }
        if let Err(e) = self.lead_ready() {
            self.log(&format!("lead dispatch check failed: {e}"), false);
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

        // 3. finished or stuck?
        self.finish_or_stop_check()
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
                    self.log(
                        &format!("{id}: the coder reports {} not met -> fix round", ids.join(", ")),
                        false,
                    );
                    return self.fix_round(
                        id,
                        &format!("acceptance criteria still not met: {}", ids.join(", ")),
                        "",
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
                    let body = match self.work.fix_body(id, 1, "result file") {
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
                    &tail,
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

    /// Count a fix round (capped at `MAX_FIX`), then queue a fix job the scheduler
    /// hands to a coder — preferably the builder (M2).
    fn fix_round(&mut self, id: &str, cap_reason: &str, tail: &str) -> Result<()> {
        let t = self.store.get_task(id)?;
        if t.fix_rounds >= MAX_FIX {
            return self.out_of_rounds(id, cap_reason);
        }
        let n = self.store.bump(id, Counter::Fix)?;
        let body = match self.work.fix_body(id, n, tail) {
            Ok(b) => b,
            Err(e) => return self.park(id, State::Paused, &format!("orch error: {e}"), false),
        };
        self.store.transition(
            id,
            &TransitionRequest::auto(State::NeedsFix, Some(&format!("fix round {n}"))),
        )?;
        self.state.jobs.push(QueuedJob {
            task: id.to_string(),
            file: format!("fix-{n}.md"),
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
            let reviewer = self.reviewer_of()?;
            self.bench_after_delivery_failure(&reviewer, id, &e)?;
        }
        Ok(())
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
        self.store.transition(
            id,
            &TransitionRequest::auto(State::NeedsFix, Some(&format!("{count} review blocker(s)"))),
        )?;
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
            CommitOutcome::Conflict { line } | CommitOutcome::BranchFails { line } => {
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
                self.store.transition(
                    id,
                    &TransitionRequest::auto(
                        State::Conflict,
                        Some(&format!("{line} (conflict round {n})")),
                    ),
                )?;
                self.log(&format!("{line} -> queued for the next free coder"), false);
                Ok(())
            }
        }
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
        let rel = format!(".orch-task/{task}");
        let text = format!(
            "Read {rel}/{file} and carry it out exactly (the milestone's contract is {rel}/task.json). When \
             completely finished, write {rel}/result-{attempt}.json in the `result_format` shape from task.json, \
             and a short report {rel}/report-{attempt}.md (max 20 lines: files changed, what you did, check \
             result; if something was ambiguous, status `question` in the result, a line starting QUESTION: \
             here, and stop; last line `COMMIT: <one-line commit message for the change>`), then create the \
             empty file {rel}/done-{attempt} as your very last action."
        );
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
            let ok = self.work.check("(final check)") == Ok(CheckOutcome::Pass);
            let branch = self.settings().ok().flatten().map_or_else(String::new, |s| s.branch);
            let _ = self.store.enqueue_lead_event(
                LeadKind::AllDone,
                None,
                &format!("final check {} on {branch}", if ok { "PASS" } else { "FAIL" }),
            );
            self.log(
                &format!(
                    "ALL_DONE on {branch} — final check {}. Run `orch finish` to push and open the PR.",
                    if ok { "PASS" } else { "FAIL" }
                ),
                true,
            );
            return Cycle::Exit(if ok { EXIT_ALL_DONE } else { EXIT_STOPPED });
        }
        let active = tasks.iter().any(|t| {
            matches!(
                t.status,
                State::Coding | State::Reviewing | State::Verifying | State::Committed
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
        if self.applied.contains(&key) {
            self.log(&format!("duplicate: {key} — nothing changed"), false);
            return Ok(());
        }
        if self.store.get_task(ev.task()).is_err() {
            self.log(&format!("unknown milestone {}: event dropped", ev.task()), false);
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
            RunEvent::Blocked { task, agent } => {
                self.applied.insert(key.clone());
                if !self.state.notified.contains(&key) {
                    self.state.notified.push(key.clone());
                    self.log(
                        &format!(
                            "{agent} is waiting for a PERMISSION/QUESTION in its pane (task {task}). \
                             Answer it there; autopilot keeps going."
                        ),
                        true,
                    );
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
