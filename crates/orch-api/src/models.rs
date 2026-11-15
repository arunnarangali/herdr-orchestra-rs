//! The JSON shapes of the read endpoints. Field names are part of the contract: the GUI and
//! the live stream (M4) rely on them. Every shape is built from a typed view in
//! `orch_core::view` (or the registries in `orch-config`), never from printed text.
//! States and kinds are their CLI spellings (`awaiting-review`, `needs-answer`, …), except
//! `run_state`, which is snake_case (`all_done`, `broken_branch`).
//!
//! Text that came from an agent or the repository (results, review findings, log lines) is
//! data: it is passed through as plain strings and never interpreted.

use orch_config::agents::Registry;
use orch_config::discovery::{self, AuthState, skills};
use orch_config::models::Catalog;
use orch_core::check::CheckRecord;
use orch_core::run_store::Run;
use orch_core::task::Transition;
use orch_core::view;
use serde::Serialize;
use utoipa::ToSchema;

// ------------------------------------------------------------------------------- status

#[derive(Debug, Serialize, ToSchema)]
pub struct StatusResponse {
    pub title: String,
    pub branch: String,
    pub base: Option<String>,
    pub isolation: String,
    pub max_parallel: i64,
    pub reviewer: Option<String>,
    pub lead_mode: String,
    /// `running`, `idle`, `all_done`, `stopped` (nothing running or ready: the developer is
    /// needed) or `broken_branch` (the run branch fails its check).
    pub run_state: String,
    pub all_done: bool,
    pub stuck: bool,
    /// Why the run branch is broken, when it is.
    pub broken_branch: Option<BrokenInfo>,
    pub event_manager: EventManagerInfo,
    pub events: EventCounts,
    /// Milestones per state; every state is listed.
    pub progress: Vec<StateCount>,
    pub workers: Vec<WorkerInfo>,
    pub tasks: Vec<TaskBrief>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BrokenInfo {
    pub reason: String,
    pub since: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EventManagerInfo {
    pub running: bool,
    /// `orch autopilot (pid …, started …)` while it runs.
    pub holder: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EventCounts {
    pub unread: usize,
    pub open: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StateCount {
    pub state: String,
    pub count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WorkerInfo {
    pub name: String,
    pub kind: String,
    pub model: Option<String>,
    pub tier: Option<String>,
    pub detect: bool,
    /// The milestone it holds; `null` when free.
    pub task: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TaskBrief {
    pub id: String,
    pub title: String,
    pub state: String,
    pub deps: Vec<String>,
    pub worker: Option<String>,
    pub tier: Option<String>,
    pub reason: String,
}

impl From<view::StatusView> for StatusResponse {
    fn from(v: view::StatusView) -> Self {
        StatusResponse {
            title: v.settings.title,
            branch: v.settings.branch,
            base: v.settings.base,
            isolation: v.settings.isolation,
            max_parallel: v.settings.max_parallel,
            reviewer: v.settings.reviewer,
            lead_mode: v.settings.lead_mode,
            run_state: v.run_state.as_str().into(),
            all_done: v.all_done,
            stuck: v.stuck,
            broken_branch: v.broken.map(|b| BrokenInfo {
                reason: b.reason,
                since: b.since,
            }),
            event_manager: EventManagerInfo {
                running: v.event_manager.is_some(),
                holder: v.event_manager,
            },
            events: EventCounts {
                unread: v.unread_events,
                open: v.open_events,
            },
            progress: v
                .progress
                .into_iter()
                .map(|c| StateCount {
                    state: c.state.as_str().into(),
                    count: c.count,
                })
                .collect(),
            workers: v
                .workers
                .into_iter()
                .map(|w| WorkerInfo {
                    name: w.name,
                    kind: w.kind,
                    model: w.model,
                    tier: w.tier,
                    detect: w.detect,
                    task: w.task,
                })
                .collect(),
            tasks: v
                .tasks
                .into_iter()
                .map(|t| TaskBrief {
                    id: t.id,
                    title: t.title,
                    state: t.status.as_str().into(),
                    deps: t.deps,
                    worker: t.worker,
                    tier: t.tier,
                    reason: t.reason,
                })
                .collect(),
        }
    }
}

// ------------------------------------------------------------------------------- plan

#[derive(Debug, Serialize, ToSchema)]
pub struct PlanResponse {
    pub title: Option<String>,
    pub header: PlanHeader,
    pub feature_breakdown: String,
    pub milestones: Vec<PlanMilestone>,
}

/// The plan header lines as written.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlanHeader {
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
    /// Other header lines as `[name, value]` pairs.
    pub extra: Vec<Vec<String>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlanDependency {
    pub id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlanCriterion {
    /// `AC1`, `AC2`, … in plan order.
    pub id: String,
    pub text: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlanMilestone {
    pub id: String,
    pub title: String,
    pub enabled: bool,
    pub goal: String,
    pub why: String,
    pub why_separate: Option<String>,
    pub depends_on: Vec<PlanDependency>,
    pub prefer: Option<String>,
    pub tier: Option<String>,
    pub needs: Vec<String>,
    pub model_why: Option<String>,
    pub estimate: Option<String>,
    pub role: Option<String>,
    pub review: Option<String>,
    pub parallel: bool,
    pub likely_files: Vec<String>,
    pub acceptance_criteria: Vec<PlanCriterion>,
    pub constraints: String,
    pub interfaces: String,
    pub context: String,
    pub out_of_scope: String,
    pub open_questions: String,
    pub has_open_questions: bool,
    pub notes_from_user: String,
}

impl From<view::PlanView> for PlanResponse {
    fn from(v: view::PlanView) -> Self {
        let h = v.header;
        PlanResponse {
            title: v.title,
            header: PlanHeader {
                branch: h.branch,
                base: h.base,
                check_command: h.check_command,
                setup_command: h.setup_command,
                isolation: h.isolation,
                max_parallel: h.max_parallel,
                scope: h.scope,
                budget: h.budget,
                commits: h.commits,
                lead: h.lead,
                lead_mode: h.lead_mode,
                workers: h.workers,
                reviewer: h.reviewer,
                risk_hold: h.risk_hold,
                setup_inputs: h.setup_inputs,
                extra: h.extra.into_iter().map(|(k, v)| vec![k, v]).collect(),
            },
            feature_breakdown: v.feature_breakdown,
            milestones: v
                .milestones
                .into_iter()
                .map(|m| PlanMilestone {
                    id: m.id,
                    title: m.title,
                    enabled: m.enabled,
                    goal: m.goal,
                    why: m.why,
                    why_separate: m.why_separate,
                    depends_on: m
                        .depends_on
                        .into_iter()
                        .map(|d| PlanDependency {
                            id: d.id,
                            reason: d.reason,
                        })
                        .collect(),
                    prefer: m.prefer,
                    tier: m.tier,
                    needs: m.needs,
                    model_why: m.model_why,
                    estimate: m.estimate,
                    role: m.role,
                    review: m.review,
                    parallel: m.parallel,
                    likely_files: m.likely_files,
                    acceptance_criteria: m
                        .acceptance_criteria
                        .into_iter()
                        .map(|c| PlanCriterion {
                            id: c.id,
                            text: c.text,
                        })
                        .collect(),
                    constraints: m.constraints,
                    interfaces: m.interfaces,
                    context: m.context,
                    out_of_scope: m.out_of_scope,
                    open_questions: m.open_questions,
                    has_open_questions: m.has_open_questions,
                    notes_from_user: m.notes_from_user,
                })
                .collect(),
        }
    }
}

// ------------------------------------------------------------------------------- tasks

/// One milestone as stored: its state, who has it and its counters.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskInfo {
    pub id: String,
    pub title: String,
    pub state: String,
    pub reason: String,
    pub deps: Vec<String>,
    pub worker: Option<String>,
    pub last_worker: Option<String>,
    pub model: Option<String>,
    pub tier: Option<String>,
    pub needs: Vec<String>,
    /// Pending with every dependency merged.
    pub ready: bool,
    pub attempt: u32,
    pub fix_rounds: u32,
    pub review_rounds: u32,
    pub conflict_rounds: u32,
    pub lost: u32,
    pub escalated: bool,
}

impl From<view::TaskRow> for TaskInfo {
    fn from(r: view::TaskRow) -> Self {
        let t = r.task;
        TaskInfo {
            id: t.id,
            title: t.title,
            state: t.status.as_str().into(),
            reason: t.reason,
            deps: t.deps,
            worker: t.worker,
            last_worker: t.last_worker,
            model: t.model,
            tier: r.tier,
            needs: r.needs,
            ready: r.ready,
            attempt: t.attempt,
            fix_rounds: t.fix_rounds,
            review_rounds: t.review_rounds,
            conflict_rounds: t.conflict_rounds,
            lost: t.lost,
            escalated: t.escalated,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TasksResponse {
    pub tasks: Vec<TaskInfo>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HistoryEntry {
    pub ts: i64,
    pub from: String,
    pub to: String,
    pub why: String,
    pub worker: Option<String>,
    pub model: Option<String>,
    pub attempt: u32,
    pub forced: bool,
}

impl From<Transition> for HistoryEntry {
    fn from(t: Transition) -> Self {
        HistoryEntry {
            ts: t.ts,
            from: t.from.as_str().into(),
            to: t.to.as_str().into(),
            why: t.why,
            worker: t.worker,
            model: t.model,
            attempt: t.attempt,
            forced: t.forced,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CheckInfo {
    /// `worktree` (the coder's folder) or `branch` (the run branch).
    #[serde(rename = "where")]
    pub place: String,
    pub ok: bool,
    pub secs: f64,
    pub ts: i64,
}

impl From<CheckRecord> for CheckInfo {
    fn from(c: CheckRecord) -> Self {
        CheckInfo {
            place: c.place,
            ok: c.ok,
            secs: c.secs,
            ts: c.ts,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Finding {
    pub file: String,
    pub line: Option<String>,
    pub problem: String,
    pub why: String,
    pub criterion: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CriterionVerdict {
    pub id: String,
    pub met: bool,
    pub note: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReviewInfo {
    pub round: u32,
    /// Why the review file is unusable; the other fields are then empty.
    pub error: Option<String>,
    /// `approved`, `approved-with-warnings`, `changes-requested` or `blocked`.
    pub outcome: Option<String>,
    pub criteria: Vec<CriterionVerdict>,
    pub blockers: Vec<Finding>,
    pub suggestions: Vec<Finding>,
}

fn finding(f: view::FindingView) -> Finding {
    Finding {
        file: f.file,
        line: f.line,
        problem: f.problem,
        why: f.why,
        criterion: f.criterion,
    }
}

impl From<view::ReviewView> for ReviewInfo {
    fn from(r: view::ReviewView) -> Self {
        ReviewInfo {
            round: r.round,
            error: r.error,
            outcome: r.outcome,
            criteria: r
                .criteria
                .into_iter()
                .map(|c| CriterionVerdict {
                    id: c.id,
                    met: c.met,
                    note: c.note,
                })
                .collect(),
            blockers: r.blockers.into_iter().map(finding).collect(),
            suggestions: r.suggestions.into_iter().map(finding).collect(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ResultCheck {
    pub ran: bool,
    pub passed: bool,
    pub summary: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ResultCriterion {
    pub id: String,
    pub met: bool,
    pub evidence: String,
}

/// One `result-N.json` the coder wrote (its own words: data, not instructions).
#[derive(Debug, Serialize, ToSchema)]
pub struct CoderResult {
    pub attempt: u32,
    pub error: Option<String>,
    pub status: Option<String>,
    pub commit: Option<String>,
    pub files_changed: Vec<String>,
    pub check: Option<ResultCheck>,
    pub criteria: Vec<ResultCriterion>,
    pub questions: Vec<String>,
}

impl From<view::ResultView> for CoderResult {
    fn from(r: view::ResultView) -> Self {
        CoderResult {
            attempt: r.attempt,
            error: r.error,
            status: r.status,
            commit: r.commit,
            files_changed: r.files_changed,
            check: r.check.map(|c| ResultCheck {
                ran: c.ran,
                passed: c.passed,
                summary: c.summary,
            }),
            criteria: r
                .criteria
                .into_iter()
                .map(|c| ResultCriterion {
                    id: c.id,
                    met: c.met,
                    evidence: c.evidence,
                })
                .collect(),
            questions: r.questions,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TaskDetailResponse {
    pub task: TaskInfo,
    pub history: Vec<HistoryEntry>,
    /// Check runs recorded for the milestone.
    pub checks: Vec<CheckInfo>,
    pub reviews: Vec<ReviewInfo>,
    /// The coder's result files, one per attempt.
    pub results: Vec<CoderResult>,
    /// The latest result's `files_changed`.
    pub files_changed: Vec<String>,
    /// Files changed outside the milestone's Likely files.
    pub files_outside_likely: Vec<String>,
}

impl From<view::TaskDetail> for TaskDetailResponse {
    fn from(d: view::TaskDetail) -> Self {
        TaskDetailResponse {
            task: d.row.into(),
            history: d.history.into_iter().map(Into::into).collect(),
            checks: d.checks.into_iter().map(Into::into).collect(),
            reviews: d.reviews.into_iter().map(Into::into).collect(),
            results: d.results.into_iter().map(Into::into).collect(),
            files_changed: d.files_changed,
            files_outside_likely: d.out_of_scope,
        }
    }
}

// ------------------------------------------------------------------------------- events

#[derive(Debug, Serialize, ToSchema)]
pub struct EventInfo {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub task: Option<String>,
    pub task_title: Option<String>,
    pub text: String,
    /// The agent the event is about (a blocked permission event names its agent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// What the agent's pane asks for (the visible prompt text, when read).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// The lead has read it (`orch events`).
    pub read: bool,
    /// Its milestone is still in the state it announced.
    pub open: bool,
    /// What the lead is told to do about it.
    pub help: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReadyInfo {
    pub task: String,
    pub title: String,
    pub tier: Option<String>,
    /// Workers that may take it.
    pub workers: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EventsResponse {
    pub lead_on: bool,
    /// Unread and still open.
    pub new: Vec<EventInfo>,
    /// Read, but the milestone is still blocked.
    pub still_open: Vec<EventInfo>,
    /// Milestones waiting for the lead's dispatch.
    pub ready: Vec<ReadyInfo>,
}

fn event(e: view::EventView) -> EventInfo {
    EventInfo {
        id: e.entry.id,
        ts: e.entry.ts,
        kind: e.entry.kind.as_str().into(),
        task: e.entry.task,
        task_title: e.task_title,
        text: e.entry.text,
        agent: e.agent,
        prompt: e.prompt,
        read: e.entry.read_at.is_some(),
        open: e.open,
        help: e.help,
    }
}

impl From<view::EventsView> for EventsResponse {
    fn from(v: view::EventsView) -> Self {
        EventsResponse {
            lead_on: v.lead_on,
            new: v.new.into_iter().map(event).collect(),
            still_open: v.still_open.into_iter().map(event).collect(),
            ready: v
                .ready
                .into_iter()
                .map(|r| ReadyInfo {
                    task: r.task,
                    title: r.title,
                    tier: r.tier,
                    workers: r.workers,
                })
                .collect(),
        }
    }
}

// ------------------------------------------------------------------------------- report

#[derive(Debug, Serialize, ToSchema)]
pub struct ReportRowInfo {
    pub id: String,
    pub state: String,
    pub tries: u32,
    pub fix_rounds: u32,
    pub review_rounds: u32,
    pub conflict_rounds: u32,
    pub lost: u32,
    /// Tier the plan asked for.
    pub planned_tier: Option<String>,
    /// The model that did the work.
    pub model: Option<String>,
    /// The model's tier in the catalog (`null` when unrated or unknown).
    pub actual_tier: Option<String>,
    /// The plan asked for more than the model's tier.
    pub below_planned_tier: bool,
    pub started: Option<i64>,
    pub finished: Option<i64>,
    pub seconds: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ModelUse {
    pub model: String,
    pub tier: Option<String>,
    pub tasks: usize,
    pub merged: usize,
    pub tries: u32,
    pub fix_rounds: u32,
    pub review_rounds: u32,
    pub conflict_rounds: u32,
    pub lost: u32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WastedWork {
    pub lost_attempts: u32,
    pub fix_rounds: u32,
    pub conflict_rounds: u32,
    pub extra_attempts: u32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReportResponse {
    pub branch: String,
    pub merged: usize,
    pub total: usize,
    pub failed: usize,
    pub conflicts: u32,
    pub lost: u32,
    pub fix_rounds: u32,
    pub broken_branch: Option<BrokenInfo>,
    pub started: Option<i64>,
    pub finished: Option<i64>,
    pub elapsed_s: Option<i64>,
    /// Merged milestones that needed no fix, conflict, lost attempt or second try.
    pub first_pass: usize,
    /// `first_pass / merged`, `null` while nothing merged.
    pub first_pass_rate: Option<f64>,
    pub wasted: WastedWork,
    pub tasks: Vec<ReportRowInfo>,
    pub models: Vec<ModelUse>,
}

fn tier_rank(t: Option<&str>) -> usize {
    orch_core::scheduler::rank(t)
}

/// The report, with each model's tier looked up in `catalog` (when it could be loaded).
pub fn report_response(v: view::ReportView, catalog: Option<&Catalog>) -> ReportResponse {
    let tier_of = |m: &Option<String>| -> Option<String> {
        let (m, c) = (m.as_deref()?, catalog?);
        c.model_tier(m).map(String::from)
    };
    let total = v.rows.len();
    ReportResponse {
        branch: v.branch,
        merged: v.merged,
        total,
        failed: v.failed,
        conflicts: v.conflicts,
        lost: v.lost,
        fix_rounds: v.fix_rounds,
        broken_branch: v.broken.map(|b| BrokenInfo {
            reason: b.reason,
            since: b.since,
        }),
        started: v.started,
        finished: v.finished,
        elapsed_s: v.elapsed_s,
        first_pass: v.first_pass,
        first_pass_rate: v.first_pass_rate,
        wasted: WastedWork {
            lost_attempts: v.wasted.lost_attempts,
            fix_rounds: v.wasted.fix_rounds,
            conflict_rounds: v.wasted.conflict_rounds,
            extra_attempts: v.wasted.extra_attempts,
        },
        tasks: v
            .rows
            .into_iter()
            .map(|r| {
                let actual = tier_of(&r.model);
                ReportRowInfo {
                    below_planned_tier: actual.is_some()
                        && tier_rank(actual.as_deref()) < tier_rank(r.planned_tier.as_deref()),
                    id: r.id,
                    state: r.status.as_str().into(),
                    tries: r.attempt,
                    fix_rounds: r.fix_rounds,
                    review_rounds: r.review_rounds,
                    conflict_rounds: r.conflict_rounds,
                    lost: r.lost,
                    planned_tier: r.planned_tier,
                    actual_tier: actual,
                    model: r.model,
                    started: r.started,
                    finished: r.finished,
                    seconds: r.started.zip(r.finished).map(|(a, b)| (b - a).max(0)),
                }
            })
            .collect(),
        models: v
            .models
            .into_iter()
            .map(|m| ModelUse {
                tier: tier_of(&Some(m.model.clone())),
                model: m.model,
                tasks: m.tasks,
                merged: m.merged,
                tries: m.attempts,
                fix_rounds: m.fix_rounds,
                review_rounds: m.review_rounds,
                conflict_rounds: m.conflict_rounds,
                lost: m.lost,
            })
            .collect(),
    }
}

// ------------------------------------------------------------------------------- agents

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginInfo {
    /// `authenticated`, `not authenticated` or `unknown`.
    pub state: String,
    pub reason: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SkillsInfo {
    /// `current`, `outdated`, `missing`, or `none` when the kind has no skills folder.
    pub status: String,
    /// Files of the orch skills/commands that are missing or outdated.
    pub problems: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AgentKindInfo {
    pub kind: String,
    pub display_name: Option<String>,
    pub installed: bool,
    pub roles: Vec<String>,
    /// herdr can tell idle / working / blocked for this kind.
    pub herdr_detect: bool,
    /// Launch option and skills folder were checked against the real tool.
    pub verified: bool,
    pub launch: String,
    pub skills: SkillsInfo,
    pub login: LoginInfo,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TeamMember {
    pub name: String,
    /// `worker`, `reviewer` or `lead`.
    pub role: String,
    pub kind: String,
    pub model: Option<String>,
    pub tier: Option<String>,
    pub detect: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AgentsResponse {
    /// Every kind in the registry.
    pub agents: Vec<AgentKindInfo>,
    /// The run's team; empty when there is no run.
    pub team: Vec<TeamMember>,
}

/// The registry as data. Login is read from the kind's `auth_file` only: a kind that needs
/// a command to find out is `unknown` here (run `orch doctor`), so a GET never starts a tool.
pub fn agents_response(reg: &Registry, run: Option<&Run>) -> AgentsResponse {
    let home = skills::home();
    let agents = reg
        .0
        .iter()
        .map(|(kind, ak)| {
            let installed = ak.is_installed();
            let assets = skills::agent_assets(ak, &home);
            let bad = |s: skills::AssetState| assets.iter().filter(move |a| a.state == s);
            let problems: Vec<String> = bad(skills::AssetState::Missing)
                .chain(bad(skills::AssetState::Outdated))
                .map(|a| skills::short(&a.path, &home))
                .collect();
            let status = if assets.is_empty() {
                "none"
            } else if bad(skills::AssetState::Missing).next().is_some() {
                "missing"
            } else if bad(skills::AssetState::Outdated).next().is_some() {
                "outdated"
            } else {
                "current"
            };
            let login = if ak.auth_cmd.as_deref().is_some_and(|c| !c.is_empty()) {
                LoginInfo {
                    state: AuthState::Unknown.label().into(),
                    reason: "needs the tool's own status command: run `orch doctor`".into(),
                }
            } else {
                let a = discovery::auth(ak, &home, std::time::Duration::from_secs(1));
                LoginInfo {
                    state: a.state.label().into(),
                    reason: a.reason,
                }
            };
            AgentKindInfo {
                kind: kind.clone(),
                display_name: ak.display_name.clone(),
                installed,
                roles: ak.roles.iter().map(|r| r.as_str().to_string()).collect(),
                herdr_detect: ak.herdr_detect,
                verified: ak.verified,
                launch: if ak.launch.is_empty() {
                    ak.binary.clone()
                } else {
                    ak.launch.clone()
                },
                skills: SkillsInfo {
                    status: status.into(),
                    problems,
                },
                login,
            }
        })
        .collect();
    let team = run
        .map(|r| {
            r.agents
                .iter()
                .map(|a| TeamMember {
                    name: a.name.clone(),
                    role: a.role.as_str().into(),
                    kind: a.kind.clone(),
                    model: a.model.clone(),
                    tier: a.tier.clone(),
                    detect: a.detect,
                })
                .collect()
        })
        .unwrap_or_default();
    AgentsResponse { agents, team }
}

// ------------------------------------------------------------------------------- models

#[derive(Debug, Serialize, ToSchema)]
pub struct ModelInfo {
    pub id: String,
    /// The tier it serves (after the reasoning floor); `null` when unrated.
    pub tier: Option<String>,
    pub kinds: Vec<String>,
    pub reasoning: Option<i64>,
    pub coding: Option<i64>,
    pub architecture: Option<i64>,
    pub instructions: Option<i64>,
    pub tools: Option<i64>,
    pub planning: Option<i64>,
    pub speed: Option<i64>,
    pub context_k: Option<i64>,
    pub open_weights: Option<bool>,
    pub evidence: Option<String>,
    /// The policy forbids it in runs.
    pub denied: bool,
    /// The policy pattern that forbids it.
    pub denied_by: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TierInfo {
    pub name: String,
    /// Reasoning rating a model needs to serve the tier.
    pub reasoning_floor: i64,
    pub purpose: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PolicyInfo {
    pub deny: Vec<String>,
    pub deny_why: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ModelsResponse {
    pub models: Vec<ModelInfo>,
    pub tiers: Vec<TierInfo>,
    pub policy: PolicyInfo,
}

pub fn models_response(cat: &Catalog) -> ModelsResponse {
    let models = cat
        .models
        .iter()
        .map(|(id, m)| {
            let denied_by = cat.denied(id).cloned();
            ModelInfo {
                id: id.clone(),
                tier: cat.model_tier(id).map(String::from),
                kinds: m.kinds.clone(),
                reasoning: m.reasoning,
                coding: m.coding,
                architecture: m.architecture,
                instructions: m.instructions,
                tools: m.tools,
                planning: m.planning,
                speed: m.speed,
                context_k: m.context_k,
                open_weights: m.open_weights,
                evidence: m.evidence.clone(),
                denied: denied_by.is_some(),
                denied_by,
            }
        })
        .collect();
    let tiers = ["small", "medium", "high"]
        .into_iter()
        .filter_map(|n| {
            cat.tiers.get(n).map(|t| TierInfo {
                name: n.into(),
                reasoning_floor: t.reasoning,
                purpose: t.use_.clone(),
            })
        })
        .collect();
    ModelsResponse {
        models,
        tiers,
        policy: PolicyInfo {
            deny: cat.policy.deny.clone(),
            deny_why: cat.policy.deny_why.clone(),
        },
    }
}

// ------------------------------------------------------------------------------- logs

#[derive(Debug, Serialize, ToSchema)]
pub struct LogsResponse {
    /// The last lines of `autopilot.log`, oldest first, as plain text.
    pub lines: Vec<String>,
}
