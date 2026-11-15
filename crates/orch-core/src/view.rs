//! Typed read views of a run: what `orch status`, `orch report` and `orch events` print, and
//! what the REST API serves, computed once here. The CLI renders these as text; the API turns
//! them into JSON. Nothing in this module writes: it never marks an event read, never creates
//! a file and never starts a run.

use crate::check::{CheckRecord, Records};
use crate::error::Result;
use crate::integration::Broken;
use crate::lead_queue::{LeadEntry, LeadKind};
use crate::lease::{driver_alive, driver_info, driver_lock_path};
use crate::plan::{Milestone, Plan};
use crate::review;
use crate::run_store::{AgentRole, PlannedTask, Run, RunSettings, is_active};
use crate::scheduler::rank;
use crate::store::Store;
use crate::task::{State, Task, Transition};
use crate::task_contract;
use std::collections::BTreeMap;
use std::path::Path;

// ------------------------------------------------------------------------------- status

/// Where the run stands, in one word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    /// A coder is working a milestone.
    Running,
    /// Nothing is running, but work is ready or waiting on something that will move.
    Idle,
    /// Every milestone landed or was skipped.
    AllDone,
    /// Nothing is running and nothing is ready: paused, failed, held or unanswered
    /// milestones wait for the developer (what `orch status` calls STUCK).
    Stopped,
    /// The run branch fails its check.
    BrokenBranch,
}

impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            RunState::Running => "running",
            RunState::Idle => "idle",
            RunState::AllDone => "all_done",
            RunState::Stopped => "stopped",
            RunState::BrokenBranch => "broken_branch",
        }
    }
}

/// A worker of the team and the milestone it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerView {
    pub name: String,
    pub kind: String,
    pub model: Option<String>,
    pub tier: Option<String>,
    /// Whether herdr watches this kind.
    pub detect: bool,
    /// The milestone it holds, if any.
    pub task: Option<String>,
}

/// One milestone line of the status (plan order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusTask {
    pub id: String,
    pub title: String,
    pub status: State,
    pub deps: Vec<String>,
    pub worker: Option<String>,
    pub tier: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateCount {
    pub state: State,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusView {
    pub settings: RunSettings,
    pub broken: Option<Broken>,
    pub workers: Vec<WorkerView>,
    pub tasks: Vec<StatusTask>,
    pub run_state: RunState,
    /// Every milestone landed or was skipped (`orch status` prints ALL_DONE).
    pub all_done: bool,
    /// Nothing running and nothing ready (`orch status` prints STUCK).
    pub stuck: bool,
    /// The event manager's holder line while it runs.
    pub event_manager: Option<String>,
    pub unread_events: usize,
    pub open_events: usize,
    /// Milestones per state, every state listed (zero included), in `State::ALL` order.
    pub progress: Vec<StateCount>,
}

/// The status of the run. `orch_dir` is the project's `.orchestrate/` folder.
pub fn status_view(store: &Store, run: &Run, orch_dir: &Path) -> Result<StatusView> {
    let tasks = store.list_tasks()?;
    let broken = store.integration_broken()?;
    let workers = run
        .agents
        .iter()
        .filter(|a| a.role == AgentRole::Worker)
        .map(|w| WorkerView {
            name: w.name.clone(),
            kind: w.kind.clone(),
            model: w.model.clone(),
            tier: w.tier.clone(),
            detect: w.detect,
            task: tasks
                .iter()
                .find(|t| t.worker.as_deref() == Some(w.name.as_str()))
                .map(|t| t.id.clone()),
        })
        .collect();
    let find = |id: &str| tasks.iter().find(|t| t.id == id);
    let mut rows = Vec::new();
    for p in &run.plan {
        let Some(t) = find(&p.id) else { continue };
        rows.push(StatusTask {
            id: t.id.clone(),
            title: t.title.clone(),
            status: t.status,
            deps: t.deps.clone(),
            worker: t.worker.clone(),
            tier: p.tier.clone(),
            reason: t.reason.clone(),
        });
    }
    let merged = |id: &str| find(id).is_some_and(|t| t.status == State::Merged);
    let all_done = rows
        .iter()
        .all(|t| matches!(t.status, State::Merged | State::Skipped));
    let running = rows.iter().any(|t| is_active(t.status));
    let ready = rows
        .iter()
        .any(|t| t.status == State::Pending && t.deps.iter().all(|d| merged(d)));
    let stuck = !all_done && !running && !ready;
    let run_state = if broken.is_some() {
        RunState::BrokenBranch
    } else if all_done {
        RunState::AllDone
    } else if running {
        RunState::Running
    } else if stuck {
        RunState::Stopped
    } else {
        RunState::Idle
    };
    let lock = driver_lock_path(orch_dir);
    let event_manager = driver_alive(&lock).then(|| driver_info(&lock).holder_line());
    let progress = State::ALL
        .iter()
        .map(|s| StateCount {
            state: *s,
            count: rows.iter().filter(|t| t.status == *s).count(),
        })
        .collect();
    Ok(StatusView {
        settings: run.settings.clone(),
        broken,
        workers,
        tasks: rows,
        run_state,
        all_done,
        stuck,
        event_manager,
        unread_events: store.unread_lead_events()?.len(),
        open_events: store.still_open_lead_events()?.len(),
        progress,
    })
}

// ------------------------------------------------------------------------------- report

/// One milestone of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRow {
    pub id: String,
    pub status: State,
    pub attempt: u32,
    pub fix_rounds: u32,
    pub review_rounds: u32,
    pub conflict_rounds: u32,
    pub lost: u32,
    /// The model that did (or last did) the work.
    pub model: Option<String>,
    /// The tier the plan asked for.
    pub planned_tier: Option<String>,
    /// When the first attempt started, when it merged (epoch seconds).
    pub started: Option<i64>,
    pub finished: Option<i64>,
}

/// One model's share of the work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRow {
    pub model: String,
    pub tasks: usize,
    pub merged: usize,
    pub attempts: u32,
    pub fix_rounds: u32,
    pub review_rounds: u32,
    pub conflict_rounds: u32,
    pub lost: u32,
}

/// Work that had to be redone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wasted {
    pub lost_attempts: u32,
    pub fix_rounds: u32,
    pub conflict_rounds: u32,
    /// Attempts beyond the first, over all milestones.
    pub extra_attempts: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReportView {
    pub rows: Vec<ReportRow>,
    pub merged: usize,
    pub failed: usize,
    pub conflicts: u32,
    pub lost: u32,
    pub fix_rounds: u32,
    pub broken: Option<Broken>,
    pub branch: String,
    /// First milestone start and last landing (or last state change), epoch seconds.
    pub started: Option<i64>,
    pub finished: Option<i64>,
    pub elapsed_s: Option<i64>,
    /// Merged milestones that needed no fix, conflict, lost attempt or second attempt.
    pub first_pass: usize,
    /// `first_pass / merged`; `None` while nothing merged.
    pub first_pass_rate: Option<f64>,
    pub wasted: Wasted,
    pub models: Vec<ModelRow>,
}

pub fn report_view(store: &Store, run: &Run) -> Result<ReportView> {
    let tasks = store.list_tasks()?;
    let broken = store.integration_broken()?;
    let mut rows = Vec::new();
    for p in &run.plan {
        let Some(t) = tasks.iter().find(|t| t.id == p.id) else {
            continue;
        };
        let hist = store.history(&t.id)?;
        rows.push(ReportRow {
            id: t.id.clone(),
            status: t.status,
            attempt: t.attempt,
            fix_rounds: t.fix_rounds,
            review_rounds: t.review_rounds,
            conflict_rounds: t.conflict_rounds,
            lost: t.lost,
            model: t.model.clone(),
            planned_tier: p.tier.clone(),
            started: hist.iter().find(|h| h.to == State::Coding).map(|h| h.ts),
            finished: hist
                .iter()
                .rev()
                .find(|h| h.to == State::Merged)
                .map(|h| h.ts),
        });
    }
    let merged = rows.iter().filter(|r| r.status == State::Merged).count();
    let failed = rows.iter().filter(|r| r.status == State::Failed).count();
    let sum = |f: fn(&ReportRow) -> u32| rows.iter().map(f).sum::<u32>();
    let first_pass = rows
        .iter()
        .filter(|r| {
            r.status == State::Merged
                && r.fix_rounds == 0
                && r.conflict_rounds == 0
                && r.lost == 0
                && r.attempt <= 1
        })
        .count();
    let started = rows.iter().filter_map(|r| r.started).min();
    let finished = rows.iter().filter_map(|r| r.finished).max();
    let mut by_model: BTreeMap<String, ModelRow> = BTreeMap::new();
    for r in &rows {
        let Some(m) = &r.model else { continue };
        let e = by_model.entry(m.clone()).or_insert_with(|| ModelRow {
            model: m.clone(),
            tasks: 0,
            merged: 0,
            attempts: 0,
            fix_rounds: 0,
            review_rounds: 0,
            conflict_rounds: 0,
            lost: 0,
        });
        e.tasks += 1;
        e.merged += usize::from(r.status == State::Merged);
        e.attempts += r.attempt;
        e.fix_rounds += r.fix_rounds;
        e.review_rounds += r.review_rounds;
        e.conflict_rounds += r.conflict_rounds;
        e.lost += r.lost;
    }
    Ok(ReportView {
        merged,
        failed,
        conflicts: sum(|r| r.conflict_rounds),
        lost: sum(|r| r.lost),
        fix_rounds: sum(|r| r.fix_rounds),
        broken,
        branch: run.settings.branch.clone(),
        started,
        finished,
        elapsed_s: started.zip(finished).map(|(a, b)| (b - a).max(0)),
        first_pass,
        first_pass_rate: (merged > 0).then(|| first_pass as f64 / merged as f64),
        wasted: Wasted {
            lost_attempts: sum(|r| r.lost),
            fix_rounds: sum(|r| r.fix_rounds),
            conflict_rounds: sum(|r| r.conflict_rounds),
            extra_attempts: rows.iter().map(|r| r.attempt.saturating_sub(1)).sum(),
        },
        models: by_model.into_values().collect(),
        rows,
    })
}

// ------------------------------------------------------------------------------- events

/// What the lead is told to do about each kind of event (`{t}` is the milestone).
pub fn lead_event_help(kind: LeadKind) -> &'static str {
    match kind {
        LeadKind::Paused => {
            "a coder's question, or an agent that stopped. A question only the developer can settle: ask them, then `orch answer {t} \"<their answer>\"`. An agent problem: tell the developer what to look at, then `orch dispatch {t}`."
        }
        LeadKind::Failed => {
            "still failing after its rounds. Read the last report and check or review output, add the missing FACT or constraint to the plan (never a how-to), then `orch dispatch {t}` (add `--tier high` or `--to <worker>` to give it a stronger model)."
        }
        LeadKind::Held => {
            "risky changes wait for the developer: show them the reasons; they run `orch approve {t}`. Never approve it yourself."
        }
        LeadKind::NeedsAnswer => {
            "open questions in plan.md: ask the developer, write the answers under \"Notes from user\", set \"Open questions:\" to `- none`, `orch sync`, then `orch dispatch {t}`."
        }
        LeadKind::Merged => {
            "landed on the feature branch. Nothing to do unless it unlocked work listed under READY."
        }
        LeadKind::Ready => "milestones are ready for you: `orch dispatch <Mn>...`.",
        LeadKind::Escalated => "moved to a stronger model after its rounds ran out. Nothing to do.",
        LeadKind::AutoDispatched => {
            "started without you because you did not dispatch it in time. Nothing to do."
        }
        LeadKind::AllDone => {
            "every milestone landed. Finish .orchestrate/summary.md (see `orch lead run`, section ALL_DONE)."
        }
        LeadKind::Stopped => {
            "the event manager stopped. Tell the developer why; they run `orch autopilot` again."
        }
        LeadKind::AnswerProvided => {
            "an answer for {t} was given (by you or by the developer) and goes to its coder. Nothing to do unless it changes what you told the developer."
        }
        LeadKind::Blocked => {
            "an agent sits at a permission prompt or question in its pane (its name and the prompt are in the entry). Answer it there; the run keeps going while it waits."
        }
    }
}

/// Workers allowed to take a milestone of `tier`: its tier or higher (unrated workers count as
/// any tier); when nobody reaches it, the strongest workers (Python `eligible_workers`).
pub fn eligible_workers(run: &Run, tier: Option<&str>) -> Vec<String> {
    let need = rank(tier);
    let workers: Vec<(&str, usize)> = run
        .agents
        .iter()
        .filter(|a| a.role == AgentRole::Worker)
        .map(|a| (a.name.as_str(), rank(a.tier.as_deref())))
        .collect();
    let ok: Vec<String> = workers
        .iter()
        .filter(|(_, r)| need == 0 || *r == 0 || *r >= need)
        .map(|(n, _)| n.to_string())
        .collect();
    if !ok.is_empty() {
        return ok;
    }
    let top = workers.iter().map(|(_, r)| *r).max().unwrap_or(0);
    workers
        .iter()
        .filter(|(_, r)| *r == top)
        .map(|(n, _)| n.to_string())
        .collect()
}

/// One queue entry with what a screen needs next to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventView {
    pub entry: LeadEntry,
    /// The milestone's title, when the entry names one that exists.
    pub task_title: Option<String>,
    /// The entry is still open (see `Store::lead_entry_open`).
    pub open: bool,
    /// What the lead is told to do about it.
    pub help: String,
    /// The blocked agent the entry names (only `blocked` entries).
    pub agent: Option<String>,
    /// The pane's visible prompt text the blocked agent wants answered (§15A).
    pub prompt: Option<String>,
}

/// A milestone waiting for the lead's dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyView {
    pub task: String,
    pub title: String,
    pub tier: Option<String>,
    pub workers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventsView {
    pub lead_on: bool,
    /// Unread and still open: what the lead is woken for.
    pub new: Vec<EventView>,
    /// Read, but their milestone is still in the blocking state.
    pub still_open: Vec<EventView>,
    pub ready: Vec<ReadyView>,
}

fn event_view(store: &Store, e: &LeadEntry) -> Result<EventView> {
    let task_title = match &e.task {
        Some(t) => store.get_task(t).ok().map(|t| t.title),
        None => None,
    };
    Ok(EventView {
        entry: e.clone(),
        task_title,
        open: store.lead_entry_open(e)?,
        help: lead_event_help(e.kind).replace("{t}", e.task.as_deref().unwrap_or("<Mn>")),
        agent: e.agent.clone(),
        prompt: e.prompt.clone(),
    })
}

/// The lead's queue, read without acknowledging anything. `since` keeps entries with a larger
/// id; `limit` caps each of `new` and `still_open` (oldest first, as `orch events` lists them).
pub fn events_view(
    store: &Store,
    run: &Run,
    since: Option<i64>,
    limit: Option<usize>,
) -> Result<EventsView> {
    let keep = |e: &&LeadEntry| since.is_none_or(|s| e.id > s);
    let cap = limit.unwrap_or(usize::MAX);
    let pick = |entries: Vec<LeadEntry>| -> Result<Vec<EventView>> {
        entries
            .iter()
            .filter(keep)
            .take(cap)
            .map(|e| event_view(store, e))
            .collect()
    };
    let mut ready = Vec::new();
    for tid in store.undispatched()? {
        let t = store.get_task(&tid)?;
        let tier = run
            .plan
            .iter()
            .find(|p| p.id == tid)
            .and_then(|p| p.tier.clone());
        ready.push(ReadyView {
            workers: eligible_workers(run, tier.as_deref()),
            task: tid,
            title: t.title,
            tier,
        });
    }
    Ok(EventsView {
        lead_on: store.lead_on()?,
        new: pick(store.unread_lead_events()?)?,
        still_open: pick(store.still_open_lead_events()?)?,
        ready,
    })
}

// ------------------------------------------------------------------------------- plan

/// The plan header as written (raw values).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderView {
    pub branch: Option<String>,
    pub base: Option<String>,
    pub check_command: Option<String>,
    pub setup_command: Option<String>,
    pub isolation: Option<String>,
    pub max_parallel: Option<String>,
    pub scope: Option<String>,
    pub budget: Option<String>,
    pub commits: Option<String>,
    pub lead: Option<String>,
    pub lead_mode: Option<String>,
    pub workers: Option<String>,
    pub reviewer: Option<String>,
    pub risk_hold: Option<String>,
    pub setup_inputs: Option<String>,
    /// Any other header line, in file order.
    pub extra: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyView {
    pub id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriterionView {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneView {
    pub id: String,
    pub title: String,
    pub enabled: bool,
    pub goal: String,
    pub why: String,
    pub why_separate: Option<String>,
    pub depends_on: Vec<DependencyView>,
    pub prefer: Option<String>,
    pub tier: Option<String>,
    pub needs: Vec<String>,
    pub model_why: Option<String>,
    pub estimate: Option<String>,
    pub role: Option<String>,
    pub review: Option<String>,
    pub parallel: bool,
    pub likely_files: Vec<String>,
    pub acceptance_criteria: Vec<CriterionView>,
    pub constraints: String,
    pub interfaces: String,
    pub context: String,
    pub out_of_scope: String,
    pub open_questions: String,
    pub has_open_questions: bool,
    pub notes_from_user: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanView {
    pub title: Option<String>,
    pub header: HeaderView,
    pub feature_breakdown: String,
    pub milestones: Vec<MilestoneView>,
}

/// A field kept as an extra `Name: value` line of the milestone (`Estimate`, `Role`).
fn extra_field(m: &Milestone, key: &str) -> Option<String> {
    m.extra
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, s)| task_contract::field_text(s))
        .filter(|s| !s.is_empty())
}

fn words(s: Option<&str>) -> Vec<String> {
    s.unwrap_or("")
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(|w| w.trim().trim_matches('`').to_string())
        .filter(|w| !w.is_empty() && !w.eq_ignore_ascii_case("none"))
        .collect()
}

pub fn milestone_view(m: &Milestone) -> MilestoneView {
    MilestoneView {
        id: m.id.clone(),
        title: m.title.clone(),
        enabled: m.enabled,
        goal: m.goal.text(),
        why: m.why.text(),
        why_separate: m.why_separate.clone(),
        depends_on: m
            .depends_on
            .iter()
            .map(|d| DependencyView {
                id: d.id.clone(),
                reason: d.reason.clone(),
            })
            .collect(),
        prefer: m.prefer.clone(),
        tier: m.tier.clone(),
        needs: words(m.needs.as_deref()),
        model_why: m.model_why.clone(),
        estimate: extra_field(m, "estimate"),
        role: extra_field(m, "role"),
        review: m.review.clone(),
        parallel: m.parallel_allowed(),
        likely_files: m.likely_file_list(),
        acceptance_criteria: task_contract::acceptance_criteria(m)
            .into_iter()
            .map(|c| CriterionView {
                id: c.id,
                text: c.text,
            })
            .collect(),
        constraints: m.constraints.text(),
        interfaces: m.interfaces.text(),
        context: m.context.text(),
        out_of_scope: m.out_of_scope.text(),
        open_questions: m.open_questions.text(),
        has_open_questions: m.has_open_questions(),
        notes_from_user: m.notes_from_user.text(),
    }
}

pub fn plan_view(plan: &Plan) -> PlanView {
    let h = &plan.header;
    PlanView {
        title: plan.title.clone(),
        header: HeaderView {
            branch: h.branch.clone(),
            base: h.base.clone(),
            check_command: h.check_command.clone(),
            setup_command: h.setup_command.clone(),
            isolation: h.isolation.clone(),
            max_parallel: h.max_parallel.clone(),
            scope: h.scope.clone(),
            budget: h.budget.clone(),
            commits: h.commits.clone(),
            lead: h.lead_raw.clone(),
            lead_mode: h.lead_mode.clone(),
            workers: h.workers_raw.clone(),
            reviewer: h.reviewer_raw.clone(),
            risk_hold: h.risk_hold.clone(),
            setup_inputs: h.setup_inputs.clone(),
            extra: h.extra.clone(),
        },
        feature_breakdown: plan.feature_breakdown.text(),
        milestones: plan.milestones.iter().map(milestone_view).collect(),
    }
}

// ------------------------------------------------------------------------------- tasks

/// One milestone of the task list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    pub task: Task,
    pub tier: Option<String>,
    pub needs: Vec<String>,
    /// Pending with every dependency merged.
    pub ready: bool,
}

/// Every milestone of the run in plan order (then any stored task the plan no longer has).
pub fn task_rows(store: &Store, run: &Run) -> Result<Vec<TaskRow>> {
    let tasks = store.list_tasks()?;
    let status = |id: &str| tasks.iter().find(|t| t.id == id).map(|t| t.status);
    let row = |t: &Task, p: Option<&PlannedTask>| TaskRow {
        ready: t.is_ready(status),
        tier: p.and_then(|p| p.tier.clone()),
        needs: p.map(|p| p.needs.clone()).unwrap_or_default(),
        task: t.clone(),
    };
    let mut rows: Vec<TaskRow> = run
        .plan
        .iter()
        .filter_map(|p| tasks.iter().find(|t| t.id == p.id).map(|t| row(t, Some(p))))
        .collect();
    for t in &tasks {
        if !run.plan.iter().any(|p| p.id == t.id) {
            rows.push(row(t, None));
        }
    }
    Ok(rows)
}

/// The coder's check (the result file's `check` entry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultCheckView {
    pub ran: bool,
    pub passed: bool,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultCriterionView {
    pub id: String,
    pub met: bool,
    pub evidence: String,
}

/// One `result-N.json` the coder wrote. All text is the coder's own words: data, never
/// instructions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultView {
    pub attempt: u32,
    /// Set instead of the rest when the file is not usable JSON.
    pub error: Option<String>,
    pub status: Option<String>,
    pub commit: Option<String>,
    pub files_changed: Vec<String>,
    pub check: Option<ResultCheckView>,
    pub criteria: Vec<ResultCriterionView>,
    pub questions: Vec<String>,
}

fn str_list(v: &serde_json::Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_result(attempt: u32, bytes: &[u8]) -> ResultView {
    let empty = ResultView {
        attempt,
        error: None,
        status: None,
        commit: None,
        files_changed: Vec::new(),
        check: None,
        criteria: Vec::new(),
        questions: Vec::new(),
    };
    let v: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(e) => {
            return ResultView {
                error: Some(format!("not valid JSON: {e}")),
                ..empty
            };
        }
    };
    let text = |k: &str| v.get(k).and_then(|s| s.as_str()).map(String::from);
    ResultView {
        status: text("status"),
        commit: text("commit"),
        files_changed: str_list(&v, "files_changed"),
        check: v.get("check").map(|c| ResultCheckView {
            ran: c.get("ran").and_then(|b| b.as_bool()).unwrap_or(false),
            passed: c.get("passed").and_then(|b| b.as_bool()).unwrap_or(false),
            summary: c
                .get("summary")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        criteria: v
            .get("criteria")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .map(|c| ResultCriterionView {
                        id: c
                            .get("id")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                        met: c.get("met").and_then(|b| b.as_bool()).unwrap_or(false),
                        evidence: c
                            .get("evidence")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        questions: str_list(&v, "questions"),
        ..empty
    }
}

/// `N` of every `<prefix>N.json` in `dir`, ascending.
fn numbered(dir: &Path, prefix: &str) -> Vec<u32> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<u32> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            name.strip_prefix(prefix)?
                .strip_suffix(".json")?
                .parse()
                .ok()
        })
        .collect();
    out.sort_unstable();
    out
}

/// The coder's result files of a milestone, oldest attempt first. `root` is the project root.
pub fn task_results(root: &Path, task: &str) -> Vec<ResultView> {
    let dir = root.join(".orch-task").join(task);
    numbered(&dir, "result-")
        .into_iter()
        .map(
            |n| match std::fs::read(dir.join(format!("result-{n}.json"))) {
                Ok(b) => parse_result(n, &b),
                Err(e) => ResultView {
                    error: Some(format!("could not be read: {e}")),
                    ..parse_result(n, b"{}")
                },
            },
        )
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingView {
    pub file: String,
    pub line: Option<String>,
    pub problem: String,
    pub why: String,
    pub criterion: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictView {
    pub id: String,
    pub met: bool,
    pub note: String,
}

/// One review round. `error` is set (and the rest empty) when the file is unusable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewView {
    pub round: u32,
    pub error: Option<String>,
    pub outcome: Option<String>,
    pub criteria: Vec<VerdictView>,
    pub blockers: Vec<FindingView>,
    pub suggestions: Vec<FindingView>,
}

fn finding_view(f: &review::Finding) -> FindingView {
    FindingView {
        file: f.file.clone(),
        line: f.line.clone(),
        problem: f.problem.clone(),
        why: f.why.clone(),
        criterion: f.criterion.clone(),
    }
}

/// The review rounds on disk for a milestone, round 1 first.
pub fn task_reviews(orch_dir: &Path, task: &str) -> Vec<ReviewView> {
    numbered(&review::review_dir(orch_dir, task), "review-")
        .into_iter()
        .map(|round| match review::read_round(orch_dir, task, round) {
            Ok(r) => ReviewView {
                round,
                error: None,
                outcome: Some(r.outcome.as_str().to_string()),
                criteria: r
                    .criteria
                    .iter()
                    .map(|c| VerdictView {
                        id: c.id.clone(),
                        met: c.met,
                        note: c.note.clone(),
                    })
                    .collect(),
                blockers: r.blockers.iter().map(finding_view).collect(),
                suggestions: r.suggestions.iter().map(finding_view).collect(),
            },
            Err(why) => ReviewView {
                round,
                error: Some(why),
                outcome: None,
                criteria: Vec::new(),
                blockers: Vec::new(),
                suggestions: Vec::new(),
            },
        })
        .collect()
}

/// One milestone inspected.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskDetail {
    pub row: TaskRow,
    pub history: Vec<Transition>,
    pub checks: Vec<CheckRecord>,
    pub reviews: Vec<ReviewView>,
    pub results: Vec<ResultView>,
    /// The latest result's `files_changed`.
    pub files_changed: Vec<String>,
    /// Files changed outside the milestone's `Likely files`.
    pub out_of_scope: Vec<String>,
}

/// `None` when the run has no such milestone.
pub fn task_detail(store: &Store, run: &Run, root: &Path, id: &str) -> Result<Option<TaskDetail>> {
    let Some(row) = task_rows(store, run)?.into_iter().find(|r| r.task.id == id) else {
        return Ok(None);
    };
    let rec = Records::load(root).0.remove(id).unwrap_or_default();
    let results = task_results(root, id);
    Ok(Some(TaskDetail {
        history: store.history(id)?,
        checks: rec.checks,
        reviews: task_reviews(&root.join(crate::paths::ORCH_DIR), id),
        files_changed: results
            .last()
            .map(|r| r.files_changed.clone())
            .unwrap_or_default(),
        results,
        out_of_scope: rec.out_of_scope,
        row,
    }))
}

// ------------------------------------------------------------------------------- logs

/// Most of the log file that is read (the tail).
const LOG_WINDOW: u64 = 2 * 1024 * 1024;

fn strip_escapes(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else if !c.is_control() || c == '\t' {
            out.push(c);
        }
    }
    out
}

/// `word` appears in `line` as a whole token (not inside a longer name).
fn has_token(line: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let ok = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-'));
    line.match_indices(word).any(|(i, _)| {
        ok(line[..i].chars().next_back()) && ok(line[i + word.len()..].chars().next())
    })
}

/// The last `lines` lines of `autopilot.log` in `orch_dir`, oldest first, optionally only the
/// lines naming `milestone` or `agent`. Control characters and terminal escapes are removed:
/// the lines are text for display, whatever an agent printed. A missing log is empty.
pub fn log_tail(
    orch_dir: &Path,
    lines: usize,
    milestone: Option<&str>,
    agent: Option<&str>,
) -> std::io::Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = match std::fs::File::open(orch_dir.join("autopilot.log")) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let len = f.metadata()?.len();
    let start = len.saturating_sub(LOG_WINDOW);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.take(LOG_WINDOW).read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let mut it = text.lines();
    if start > 0 {
        it.next(); // the first line is cut in the middle
    }
    let mut out: Vec<String> = it
        .map(strip_escapes)
        .filter(|l| milestone.is_none_or(|m| has_token(l, m)))
        .filter(|l| agent.is_none_or(|a| has_token(l, a)))
        .collect();
    let drop = out.len().saturating_sub(lines);
    out.drain(..drop);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_match_whole_names_only() {
        assert!(has_token("[10:00] M2 held", "M2"));
        assert!(has_token("M2: held", "M2"));
        assert!(!has_token("M12 held", "M2"));
        assert!(!has_token("claude-10 ran", "claude-1"));
        assert!(has_token("claude-1 ran", "claude-1"));
        assert!(!has_token("anything", ""));
    }

    #[test]
    fn escapes_and_control_characters_are_removed() {
        assert_eq!(strip_escapes("a\u{1b}[31mred\u{1b}[0m\u{7}\tb"), "ared\tb");
    }

    #[test]
    fn log_tail_cuts_to_the_last_lines_and_tolerates_no_log() {
        let t = tempfile::tempdir().unwrap();
        assert!(log_tail(t.path(), 5, None, None).unwrap().is_empty());
        std::fs::write(t.path().join("autopilot.log"), "a M1\nb M2\nc M1\n").unwrap();
        assert_eq!(log_tail(t.path(), 2, None, None).unwrap(), ["b M2", "c M1"]);
        assert_eq!(
            log_tail(t.path(), 9, Some("M1"), None).unwrap(),
            ["a M1", "c M1"]
        );
    }

    #[test]
    fn eligible_workers_follow_the_tier() {
        let agent = |name: &str, tier: Option<&str>| crate::run_store::RunAgent {
            role: AgentRole::Worker,
            name: name.into(),
            kind: "k".into(),
            model: None,
            tier: tier.map(String::from),
            detect: true,
            pane: None,
        };
        let run = Run {
            settings: crate::run_store::RunSettings {
                branch: "b".into(),
                base: None,
                title: String::new(),
                commits: String::new(),
                isolation: String::new(),
                max_parallel: 1,
                setup_inputs: vec![],
                scope: String::new(),
                risk_hold: false,
                budget_s: None,
                check_command: String::new(),
                setup_command: String::new(),
                lead_mode: "off".into(),
                reviewer: None,
            },
            agents: vec![
                agent("s", Some("small")),
                agent("h", Some("high")),
                agent("u", None),
            ],
            plan: vec![],
        };
        assert_eq!(eligible_workers(&run, Some("high")), ["h", "u"]);
        assert_eq!(eligible_workers(&run, None), ["s", "h", "u"]);
    }
}
