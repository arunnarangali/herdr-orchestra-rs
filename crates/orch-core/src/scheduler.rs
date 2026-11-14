//! §10: the deterministic scheduler. A pure function decides, from a stored-state
//! snapshot alone, which ready milestone goes to which free coder for one pass — tier
//! aware, fix and conflict jobs first, serialized on shared `Likely files` — and says why
//! every ready milestone it could not assign stayed back (the `orch events` READY lines and
//! `orch status`). It never calls herdr, the clock or the filesystem: time and the run
//! settings come in through [`Snapshot`], so every rule is a plain unit test.
//!
//! Eligibility follows Python v4.10.1's `eligible_workers` / `need_rank` / `cmd_pick`
//! (with the one difference the spec asks for: a busy coder never blocks the other
//! assignments of the same pass, which `cmd_pick` cannot express because it picks one
//! coder for one milestone).
//!
//! The dispatch intent and the benches are stored in migration 5 and read and written
//! through the [`Store`] functions at the bottom of this file.

use crate::run_store::is_active;
use crate::store::{Res, Store, StoreError};
use crate::task::{State, Task};
use rusqlite::{OptionalExtension, params};
use std::collections::{HashMap, HashSet};

/// The tier order (`small < medium < high`); a worker without a tier is "unrated".
pub const TIERS: [&str; 3] = ["small", "medium", "high"];

/// 0 for no tier (unrated, or a plan written before tiers), else 1..3 (Python `rank`).
pub fn rank(tier: Option<&str>) -> usize {
    tier.and_then(|t| TIERS.iter().position(|x| *x == t))
        .map_or(0, |i| i + 1)
}

/// What a milestone asks of its coder: its plan `Tier:`, raised by the lead's `--tier`
/// minimum (Python `need_rank`).
pub fn need_rank(task_tier: Option<&str>, min_tier: Option<&str>) -> usize {
    rank(task_tier).max(rank(min_tier))
}

/// The lead's stored request for one milestone (`orch dispatch Mn [--to W] [--tier T]`,
/// and the `ready_ts` the event manager sets while the milestone waits for the lead).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Intent {
    /// The lead said go: it is not held any more.
    pub go: bool,
    /// `--to`: this exact coder, for one dispatch.
    pub assign: Option<String>,
    /// `--tier`: a minimum tier for the coder.
    pub min_tier: Option<String>,
    /// When the milestone first stood ready for the lead (the auto-dispatch wait).
    pub ready_ts: Option<i64>,
}

/// One bench (cooldown): why an agent is set aside and until when (epoch seconds).
/// A `gone` bench also ends when the run is told the agent is back (`Store::end_bench`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bench {
    pub agent: String,
    /// `'rate-limit'`, `'gone'`, `'unreachable'`, ...
    pub reason: String,
    pub until: i64,
}

/// One coder of the run's team (its tier comes from the synced run).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedWorker {
    pub name: String,
    pub kind: String,
    pub tier: Option<String>,
}

/// One milestone as the scheduler sees it: the task record plus its plan fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedTask {
    pub task: Task,
    /// Plan order (ties break in plan order).
    pub position: usize,
    pub tier: Option<String>,
    /// The plan's `Prefer:` field (first word, lowercase; `any` by default).
    pub prefer: String,
    pub files: Vec<String>,
    pub parallel: bool,
}

/// Everything one scheduling pass needs, all of it stored state or arguments:
/// the tasks, the team, the intents, the benches, the run settings and `now`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub tasks: Vec<SchedTask>,
    pub workers: Vec<SchedWorker>,
    pub intents: HashMap<String, Intent>,
    pub benches: Vec<Bench>,
    /// `Max parallel:` of the run (1 in shared mode).
    pub max_parallel: usize,
    /// `Isolation: shared` — one milestone at a time, no file serialization.
    pub shared: bool,
    /// A lead is on (`Lead mode: events`): only milestones with `go` are free to dispatch.
    pub lead_on: bool,
}

/// Which kind of work an assignment starts: a queued fix/conflict job, or a new milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Fix,
    New,
}

impl Job {
    pub fn as_str(self) -> &'static str {
        match self {
            Job::Fix => "fix",
            Job::New => "new",
        }
    }
}

/// One (milestone, worker) assignment of this pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub task: String,
    pub worker: String,
    pub job: Job,
}

/// Why a ready milestone was not assigned this pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hold {
    /// The lead has not dispatched it yet (`orch dispatch Mn`).
    LeadHold,
    /// It shares `Likely files` with running milestones (unless `Parallel: allow`).
    FilesBusy(Vec<String>),
    /// No coder on the team reaches the tier it needs (`need` = 1 small .. 3 high).
    TierUnstaffed(usize),
    /// Its coders are all busy or benched.
    CodersBusy,
    /// `Max parallel` is used up.
    NoSlots(usize),
}

impl Hold {
    /// The line `orch events` / `orch status` print for this hold (Python `cmd_ready`).
    pub fn line(&self, task: &str) -> String {
        match self {
            Hold::LeadHold => {
                format!("HOLD {task} is ready and waits for the lead: orch dispatch {task}")
            }
            Hold::FilesBusy(others) => {
                format!("WAIT {task} shares files with {}", others.join(", "))
            }
            Hold::TierUnstaffed(need) => match TIERS.get(need.saturating_sub(1)) {
                Some(tier) => format!("{task}: no coder on the team reaches tier {tier}"),
                None => format!("{task}: no coder on the team"),
            },
            Hold::CodersBusy => format!("{task}: its coders are busy or benched"),
            Hold::NoSlots(max) => format!("{task}: no free slot (max parallel {max})"),
        }
    }
}

/// One ready milestone that stayed back this pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unassigned {
    pub task: String,
    pub hold: Hold,
}

/// What one scheduling pass decided.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Schedule {
    /// In assignment order: fix/conflict jobs first, then new milestones, plan order inside.
    pub assignments: Vec<Assignment>,
    /// Ready milestones that stayed back, in candidate order, with why.
    pub unassigned: Vec<Unassigned>,
}

/// Workers allowed to take the task (Python `eligible_workers`): the `--to` coder when it
/// is on the team, else workers of the tier it needs or stronger (unrated workers count as
/// any tier); when nobody reaches the tier, the strongest workers the team has, so a run
/// never waits for a model nobody started. Also says whether the tier was actually
/// reached (`false` = the fallback to the strongest workers is in effect).
fn eligible_workers<'a>(
    workers: &'a [SchedWorker],
    intent: Option<&Intent>,
    need: usize,
) -> (Vec<&'a SchedWorker>, bool) {
    if let Some(w) = intent.and_then(|i| i.assign.as_deref())
        && let Some(a) = workers.iter().find(|x| x.name == w)
    {
        return (vec![a], true);
    }
    let ok: Vec<&SchedWorker> = workers
        .iter()
        .filter(|w| {
            let r = rank(w.tier.as_deref());
            need == 0 || r == 0 || r >= need
        })
        .collect();
    let staffed = !ok.is_empty();
    if staffed {
        return (ok, true);
    }
    let top = workers
        .iter()
        .map(|w| rank(w.tier.as_deref()))
        .max()
        .unwrap_or(0);
    let strongest: Vec<&SchedWorker> = workers
        .iter()
        .filter(|w| rank(w.tier.as_deref()) == top)
        .collect();
    (strongest, false)
}

/// Python's `(rank(n) or nr) - nr`: an unrated worker costs the same as the asked tier.
fn rank_or_need(rank: usize, need: usize) -> usize {
    if rank == 0 { need } else { rank }
}

/// The pick order of Python `cmd_pick`: the last coder first (its work is in its folder),
/// then the tier asked for before a stronger (dearer) one, then a `Prefer:` match; stable,
/// so ties keep the team order.
fn pick_order(free: &mut [&SchedWorker], last: Option<&str>, need: usize, prefer: &str) {
    free.sort_by_key(|w| {
        let r = rank(w.tier.as_deref());
        (
            usize::from(last != Some(w.name.as_str())),
            if need == 0 {
                0isize
            } else {
                rank_or_need(r, need) as isize - need as isize
            },
            usize::from(!(prefer == "any" || prefer == w.kind || prefer == w.name)),
        )
    });
}

/// The one scheduling pass (§10, Python `cmd_ready` + the autopilot dispatch loop).
///
/// * fix and conflict jobs without a coder go first, then ready new milestones; ties break
///   in plan order;
/// * one busy, benched or unsuitable coder never blocks the other assignments of the pass;
/// * a free coder of the milestone's tier or stronger is chosen (never a weaker one while
///   a suitable one exists), honouring the dispatch intent's `--to` / `--tier`, preferring
///   the coder who last worked on the milestone for fix, conflict and follow-up work;
/// * benched agents (bench not expired at `now`) and agents already on a task are never
///   assigned;
/// * milestones that share `Likely files` with a running milestone (or with one assigned
///   earlier in this pass) run one after the other unless `Parallel: allow`; shared mode
///   has no file serialization;
/// * at most `max_parallel` coders are put to work (in shared mode a queued fix job is the
///   active milestone and still gets its coder).
pub fn schedule(snap: &Snapshot, now: i64) -> Schedule {
    let mut out = Schedule::default();
    let status_of: HashMap<&str, State> = snap
        .tasks
        .iter()
        .map(|t| (t.task.id.as_str(), t.task.status))
        .collect();
    let benched: HashSet<&str> = snap
        .benches
        .iter()
        .filter(|b| b.until > now)
        .map(|b| b.agent.as_str())
        .collect();
    let mut busy: HashSet<&str> = snap
        .tasks
        .iter()
        .filter(|t| is_active(t.task.status))
        .filter_map(|t| t.task.worker.as_deref())
        .collect();
    // Which running milestone holds which file.
    let mut files_busy: HashMap<&str, Vec<&str>> = HashMap::new();
    for t in &snap.tasks {
        if is_active(t.task.status) && !t.parallel {
            for f in &t.files {
                files_busy
                    .entry(f.as_str())
                    .or_default()
                    .push(t.task.id.as_str());
            }
        }
    }
    let active_count = snap
        .tasks
        .iter()
        .filter(|t| is_active(t.task.status))
        .count();

    // The candidates: queued fix/conflict jobs first (reviewed work merges sooner and
    // unlocks dependents), then ready new milestones; both in plan order.
    let mut cands: Vec<(&SchedTask, Job)> = Vec::new();
    let mut fix_count = 0;
    for t in &snap.tasks {
        if matches!(t.task.status, State::NeedsFix | State::Conflict) && t.task.worker.is_none() {
            cands.push((t, Job::Fix));
            fix_count += 1;
        }
    }
    for t in &snap.tasks {
        if t.task.status != State::Pending {
            continue;
        }
        if t.task
            .deps
            .iter()
            .all(|d| status_of.get(d.as_str()) == Some(&State::Merged))
        {
            cands.push((t, Job::New));
        }
    }

    let mut slots = snap.max_parallel.saturating_sub(busy.len());
    if snap.shared && slots == 0 && active_count == fix_count && fix_count > 0 {
        slots = 1; // shared mode: a queued fix job IS the active milestone, and it needs a coder
    }

    for (t, job) in cands {
        let intent = snap.intents.get(t.task.id.as_str());
        if job == Job::New && snap.lead_on && !intent.is_some_and(|i| i.go) {
            out.unassigned.push(Unassigned {
                task: t.task.id.clone(),
                hold: Hold::LeadHold,
            });
            continue;
        }
        if !snap.shared && !t.parallel && job == Job::New {
            let mut clash: Vec<&str> = t
                .files
                .iter()
                .filter_map(|f| files_busy.get(f.as_str()))
                .flatten()
                .copied()
                .collect();
            clash.sort_unstable();
            clash.dedup();
            if !clash.is_empty() {
                out.unassigned.push(Unassigned {
                    task: t.task.id.clone(),
                    hold: Hold::FilesBusy(clash.into_iter().map(String::from).collect()),
                });
                continue;
            }
        }
        if slots == 0 {
            out.unassigned.push(Unassigned {
                task: t.task.id.clone(),
                hold: Hold::NoSlots(snap.max_parallel),
            });
            continue;
        }
        let need = need_rank(
            t.tier.as_deref(),
            intent.and_then(|i| i.min_tier.as_deref()),
        );
        let (eligible, staffed) = eligible_workers(&snap.workers, intent, need);
        if eligible.is_empty() {
            out.unassigned.push(Unassigned {
                task: t.task.id.clone(),
                hold: Hold::TierUnstaffed(need),
            });
            continue;
        }
        let mut free: Vec<&SchedWorker> = eligible
            .into_iter()
            .filter(|w| !busy.contains(w.name.as_str()) && !benched.contains(w.name.as_str()))
            .collect();
        if free.is_empty() {
            out.unassigned.push(Unassigned {
                task: t.task.id.clone(),
                hold: if staffed {
                    Hold::CodersBusy
                } else {
                    Hold::TierUnstaffed(need)
                },
            });
            continue;
        }
        pick_order(&mut free, t.task.last_worker.as_deref(), need, &t.prefer);
        let w = free[0];
        out.assignments.push(Assignment {
            task: t.task.id.clone(),
            worker: w.name.clone(),
            job,
        });
        busy.insert(w.name.as_str());
        slots -= 1;
        if !t.parallel && job == Job::New {
            for f in &t.files {
                files_busy
                    .entry(f.as_str())
                    .or_default()
                    .push(t.task.id.as_str());
            }
        }
    }
    out
}

/// `orch pick Mn` as a pure function (Python `cmd_pick`): the free coder the milestone
/// would go to, or `None` when nobody of its tier is free (the command answers `NONE`,
/// exit 3). Slots are not considered (`prepare` checks those).
pub fn pick(snap: &Snapshot, task_id: &str, now: i64) -> Option<String> {
    let t = snap.tasks.iter().find(|t| t.task.id == task_id)?;
    let intent = snap.intents.get(task_id);
    let need = need_rank(
        t.tier.as_deref(),
        intent.and_then(|i| i.min_tier.as_deref()),
    );
    let (eligible, _) = eligible_workers(&snap.workers, intent, need);
    let benched: HashSet<&str> = snap
        .benches
        .iter()
        .filter(|b| b.until > now)
        .map(|b| b.agent.as_str())
        .collect();
    let busy: HashSet<&str> = snap
        .tasks
        .iter()
        .filter(|x| is_active(x.task.status))
        .filter_map(|x| x.task.worker.as_deref())
        .collect();
    let mut free: Vec<&SchedWorker> = eligible
        .into_iter()
        .filter(|w| !busy.contains(w.name.as_str()) && !benched.contains(w.name.as_str()))
        .collect();
    pick_order(&mut free, t.task.last_worker.as_deref(), need, &t.prefer);
    free.first().map(|w| w.name.clone())
}

/// What `orch escalate Mn` decides (Python `cmd_escalate`): a milestone that ran out of
/// rounds can move once to the next stronger tier the team has; a second request, no
/// stronger coder, or no current coder tier at all answers "no escalation" (exit 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escalation {
    /// Escalate to this tier (the smallest team tier above the current one).
    ToTier(&'static str),
    NoEscalation,
}

/// `escalated`: whether the milestone already used its one escalation; `cur_tier`: the
/// tier of its worker or last worker (`None` = no coder, or an unrated one); `need`: the
/// milestone's `need_rank`; `team`: every coder's tier (unrated = `None`).
pub fn escalate(
    escalated: bool,
    cur_tier: Option<&str>,
    need: usize,
    team: &[Option<&str>],
) -> Escalation {
    let cur = rank(cur_tier);
    let mut above: Vec<usize> = team
        .iter()
        .map(|t| rank(*t))
        .filter(|r| *r > cur.max(need))
        .collect();
    above.sort_unstable();
    above.dedup();
    if escalated || cur == 0 || above.is_empty() {
        return Escalation::NoEscalation;
    }
    Escalation::ToTier(TIERS[above[0] - 1])
}

// ---- storage (§16): the dispatch intent and the benches, migration 5.

impl Store {
    /// Store the lead's dispatch intent for a milestone (`orch dispatch`); replaces any
    /// previous intent. The task must exist.
    pub fn set_intent(&mut self, task: &str, intent: &Intent) -> Res<()> {
        self.require_task(task)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO dispatch_intent (task_id, go, assign, min_tier, ready_ts)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                task,
                intent.go,
                intent.assign,
                intent.min_tier,
                intent.ready_ts
            ],
        )?;
        Ok(())
    }

    /// The stored dispatch intent for a milestone, `None` when the lead asked nothing.
    pub fn intent(&self, task: &str) -> Res<Option<Intent>> {
        Ok(self
            .conn
            .query_row(
                "SELECT go, assign, min_tier, ready_ts FROM dispatch_intent WHERE task_id=?1",
                [task],
                |r| {
                    Ok(Intent {
                        go: r.get::<_, i64>(0)? != 0,
                        assign: r.get(1)?,
                        min_tier: r.get(2)?,
                        ready_ts: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    /// Drop the intent (after the dispatch was carried out, or on a re-sync).
    /// The task must exist; a missing row is fine.
    pub fn clear_intent(&mut self, task: &str) -> Res<()> {
        self.require_task(task)?;
        self.conn
            .execute("DELETE FROM dispatch_intent WHERE task_id=?1", [task])?;
        Ok(())
    }

    /// Bench an agent with a reason and an expiry (epoch seconds); replaces the agent's
    /// previous bench. The agent need not be on the team (a pane may have left).
    pub fn bench_agent(&mut self, agent: &str, reason: &str, until: i64) -> Res<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO benches (agent, reason, until) VALUES (?1, ?2, ?3)",
            params![agent, reason, until],
        )?;
        Ok(())
    }

    /// End an agent's bench (`gone` benches end when the agent is seen again). A missing
    /// bench is fine.
    pub fn end_bench(&mut self, agent: &str) -> Res<()> {
        self.conn
            .execute("DELETE FROM benches WHERE agent=?1", [agent])?;
        Ok(())
    }

    /// Every stored bench, oldest expiry first (expired ones included; filter with
    /// [`Store::active_benches`]).
    pub fn benches(&self) -> Res<Vec<Bench>> {
        let mut stmt = self
            .conn
            .prepare("SELECT agent, reason, until FROM benches ORDER BY until, agent")?;
        let rows = stmt.query_map([], row_bench)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The benches still standing at `now`.
    pub fn active_benches(&self, now: i64) -> Res<Vec<Bench>> {
        Ok(self
            .benches()?
            .into_iter()
            .filter(|b| b.until > now)
            .collect())
    }

    /// The [`Snapshot`] one scheduling pass needs, from the stored state alone.
    /// `None` before the first `orch sync` (there is no run to schedule).
    pub fn scheduler_snapshot(&self) -> Res<Option<Snapshot>> {
        let Some(run) = self.load_run()? else {
            return Ok(None);
        };
        let mut tasks = Vec::new();
        for (pos, t) in self.list_tasks()?.into_iter().enumerate() {
            let plan = run.plan.iter().find(|p| p.id == t.id);
            tasks.push(SchedTask {
                task: t,
                position: plan.map_or(pos, |p| p.position.max(0) as usize),
                tier: plan.and_then(|p| p.tier.clone()),
                prefer: plan.map_or_else(|| "any".into(), |p| p.prefer.clone()),
                files: plan.map_or_else(Vec::new, |p| p.files.clone()),
                parallel: plan.is_some_and(|p| p.parallel),
            });
        }
        let workers = run
            .agents
            .iter()
            .filter(|a| a.role == crate::run_store::AgentRole::Worker)
            .map(|a| SchedWorker {
                name: a.name.clone(),
                kind: a.kind.clone(),
                tier: a.tier.clone(),
            })
            .collect();
        let mut intents = HashMap::new();
        let mut stmt = self
            .conn
            .prepare("SELECT task_id, go, assign, min_tier, ready_ts FROM dispatch_intent")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Intent {
                    go: r.get::<_, i64>(1)? != 0,
                    assign: r.get(2)?,
                    min_tier: r.get(3)?,
                    ready_ts: r.get(4)?,
                },
            ))
        })?;
        for (task, intent) in rows.collect::<rusqlite::Result<Vec<_>>>()? {
            intents.insert(task, intent);
        }
        Ok(Some(Snapshot {
            tasks,
            workers,
            intents,
            benches: self.benches()?,
            max_parallel: run.settings.max_parallel.max(0) as usize,
            shared: run.settings.isolation == "shared",
            lead_on: run.settings.lead_mode == "events",
        }))
    }

    fn require_task(&self, task: &str) -> Res<()> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
            [task],
            |r| r.get(0),
        )?;
        if exists {
            Ok(())
        } else {
            Err(StoreError::NoSuchTask(task.to_string()))
        }
    }
}

fn row_bench(r: &rusqlite::Row<'_>) -> rusqlite::Result<Bench> {
    Ok(Bench {
        agent: r.get(0)?,
        reason: r.get(1)?,
        until: r.get(2)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{NewTask, Store, TransitionRequest};
    use State::*;

    fn worker(name: &str, tier: Option<&str>) -> SchedWorker {
        SchedWorker {
            name: name.into(),
            kind: name.split('-').next().unwrap_or(name).into(),
            tier: tier.map(String::from),
        }
    }

    fn task(
        id: &str,
        status: State,
        deps: &[&str],
        worker: Option<&str>,
        last: Option<&str>,
    ) -> SchedTask {
        SchedTask {
            task: Task {
                id: id.into(),
                title: String::new(),
                status,
                reason: String::new(),
                deps: deps.iter().map(|d| (*d).into()).collect(),
                worker: worker.map(String::from),
                last_worker: last.map(String::from),
                model: None,
                attempt: 0,
                fix_rounds: 0,
                review_rounds: 0,
                conflict_rounds: 0,
                lost: 0,
                escalated: false,
            },
            position: 0,
            tier: None,
            prefer: "any".into(),
            files: vec![],
            parallel: false,
        }
    }

    fn with_tier(mut t: SchedTask, tier: &str) -> SchedTask {
        t.tier = Some(tier.into());
        t
    }

    fn with_files(mut t: SchedTask, files: &[&str]) -> SchedTask {
        t.files = files.iter().map(|f| (*f).into()).collect();
        t
    }

    fn snap(tasks: Vec<SchedTask>, workers: Vec<SchedWorker>) -> Snapshot {
        Snapshot {
            tasks,
            workers,
            intents: HashMap::new(),
            benches: vec![],
            max_parallel: 8,
            shared: false,
            lead_on: false,
        }
    }

    fn assigned<'a>(s: &'a Schedule, task: &str) -> Option<&'a str> {
        s.assignments
            .iter()
            .find(|a| a.task == task)
            .map(|a| a.worker.as_str())
    }

    fn hold<'a>(s: &'a Schedule, task: &str) -> Option<&'a Hold> {
        s.unassigned
            .iter()
            .find(|u| u.task == task)
            .map(|u| &u.hold)
    }

    #[test]
    fn ranks_follow_the_tier_order() {
        assert_eq!(rank(None), 0);
        assert_eq!(rank(Some("weird")), 0);
        assert_eq!(rank(Some("small")), 1);
        assert_eq!(rank(Some("medium")), 2);
        assert_eq!(rank(Some("high")), 3);
        assert_eq!(need_rank(Some("medium"), None), 2);
        assert_eq!(need_rank(Some("small"), Some("high")), 3);
        assert_eq!(need_rank(Some("high"), Some("small")), 3);
        assert_eq!(need_rank(None, None), 0);
    }

    #[test]
    fn assigns_ready_milestones_to_free_coders() {
        let s = schedule(
            &snap(
                vec![task("M1", Pending, &[], None, None)],
                vec![worker("w1", Some("medium")), worker("w2", Some("medium"))],
            ),
            100,
        );
        assert_eq!(s.assignments.len(), 1);
        assert_eq!(assigned(&s, "M1"), Some("w1"), "team order breaks the tie");
        assert!(s.unassigned.is_empty());
    }

    #[test]
    fn dependencies_must_be_merged() {
        let s = schedule(
            &snap(
                vec![
                    task("M1", Coding, &[], Some("w1"), None),
                    task("M2", Pending, &["M1"], None, None),
                ],
                vec![worker("w1", Some("medium")), worker("w2", Some("medium"))],
            ),
            100,
        );
        assert!(s.assignments.is_empty());
        assert!(hold(&s, "M2").is_none(), "M2 is not ready, so no hold line");
    }

    #[test]
    fn a_busy_coder_never_blocks_the_others() {
        let s = schedule(
            &snap(
                vec![
                    task("M1", Coding, &[], Some("w1"), None),
                    task("M2", Pending, &[], None, None),
                    task("M3", Pending, &[], None, None),
                ],
                vec![worker("w1", Some("medium")), worker("w2", Some("medium"))],
            ),
            100,
        );
        assert_eq!(
            s.assignments
                .iter()
                .map(|a| (a.task.as_str(), a.worker.as_str()))
                .collect::<Vec<_>>(),
            [("M2", "w2")],
            "w1 stays busy on M1; it must not be double-booked"
        );
        assert_eq!(
            hold(&s, "M3"),
            Some(&Hold::CodersBusy),
            "M3 waits because both coders are taken"
        );
    }

    #[test]
    fn never_a_weaker_coder_while_a_suitable_one_exists() {
        let s = schedule(
            &snap(
                vec![with_tier(task("M1", Pending, &[], None, None), "medium")],
                vec![
                    worker("small-1", Some("small")),
                    worker("mid-1", Some("medium")),
                ],
            ),
            100,
        );
        assert_eq!(assigned(&s, "M1"), Some("mid-1"));
    }

    #[test]
    fn the_asked_tier_outranks_a_stronger_coder() {
        let s = schedule(
            &snap(
                vec![
                    with_tier(task("M1", Pending, &[], None, None), "medium"),
                    with_tier(task("M2", Pending, &[], None, None), "medium"),
                ],
                vec![worker("hi", Some("high")), worker("mid", Some("medium"))],
            ),
            100,
        );
        assert_eq!(
            assigned(&s, "M1"),
            Some("mid"),
            "the tier asked for before a dearer stronger one"
        );
        assert_eq!(assigned(&s, "M2"), Some("hi"));
    }

    #[test]
    fn when_nobody_reaches_the_tier_the_strongest_coders_get_it() {
        let mut s = snap(
            vec![with_tier(task("M1", Pending, &[], None, None), "high")],
            vec![worker("s1", Some("small")), worker("s2", Some("small"))],
        );
        assert_eq!(pick(&s, "M1", 100).as_deref(), Some("s1"));
        // But when even the strongest coders are benched, the milestone says why it waited.
        s.benches.push(Bench {
            agent: "s1".into(),
            reason: "rate-limit".into(),
            until: 500,
        });
        s.benches.push(Bench {
            agent: "s2".into(),
            reason: "rate-limit".into(),
            until: 500,
        });
        let out = schedule(&s, 100);
        assert_eq!(hold(&out, "M1"), Some(&Hold::TierUnstaffed(3)));
        assert_eq!(
            out.unassigned[0].hold.line("M1"),
            "M1: no coder on the team reaches tier high"
        );
    }

    #[test]
    fn unrated_workers_count_as_any_tier() {
        let s = schedule(
            &snap(
                vec![
                    with_tier(task("M1", Pending, &[], None, None), "high"),
                    with_tier(task("M2", Pending, &[], None, None), "high"),
                ],
                vec![worker("free", None), worker("hi", Some("high"))],
            ),
            100,
        );
        assert_eq!(s.assignments.len(), 2);
    }

    #[test]
    fn the_assign_intent_pins_one_coder() {
        let mut s = snap(
            vec![task("M1", Pending, &[], None, None)],
            vec![worker("w1", Some("medium")), worker("w2", Some("medium"))],
        );
        s.intents.insert(
            "M1".into(),
            Intent {
                assign: Some("w2".into()),
                ..Intent::default()
            },
        );
        let s = schedule(&s, 100);
        assert_eq!(assigned(&s, "M1"), Some("w2"));
    }

    #[test]
    fn an_assign_intent_to_a_stranger_is_ignored() {
        let mut s = snap(
            vec![task("M1", Pending, &[], None, None)],
            vec![worker("w1", Some("medium"))],
        );
        s.intents.insert(
            "M1".into(),
            Intent {
                assign: Some("ghost".into()),
                ..Intent::default()
            },
        );
        let s = schedule(&s, 100);
        assert_eq!(assigned(&s, "M1"), Some("w1"));
    }

    #[test]
    fn the_min_tier_intent_raises_the_tier() {
        let mut s = snap(
            vec![with_tier(task("M1", Pending, &[], None, None), "small")],
            vec![worker("mid", Some("medium")), worker("hi", Some("high"))],
        );
        s.intents.insert(
            "M1".into(),
            Intent {
                min_tier: Some("high".into()),
                ..Intent::default()
            },
        );
        let s = schedule(&s, 100);
        assert_eq!(assigned(&s, "M1"), Some("hi"));
    }

    #[test]
    fn the_last_worker_is_preferred_for_fix_jobs() {
        let s = schedule(
            &snap(
                vec![task("M1", NeedsFix, &[], None, Some("w2"))],
                vec![worker("w1", Some("medium")), worker("w2", Some("medium"))],
            ),
            100,
        );
        assert_eq!(assigned(&s, "M1"), Some("w2"));
        assert_eq!(s.assignments[0].job, Job::Fix);
    }

    #[test]
    fn the_last_worker_is_preferred_for_new_work_too() {
        let s = schedule(
            &snap(
                vec![task("M1", Pending, &[], None, Some("w2"))],
                vec![worker("w1", Some("medium")), worker("w2", Some("medium"))],
            ),
            100,
        );
        assert_eq!(assigned(&s, "M1"), Some("w2"));
    }

    #[test]
    fn prefer_matches_kind_and_name() {
        let mut s = snap(
            vec![task("M1", Pending, &[], None, None)],
            vec![
                worker("codex-1", Some("medium")),
                worker("claude-1", Some("medium")),
            ],
        );
        s.tasks[0].prefer = "codex".into();
        let s = schedule(&s, 100);
        assert_eq!(assigned(&s, "M1"), Some("codex-1"));
    }

    #[test]
    fn fix_and_conflict_jobs_go_before_new_milestones() {
        let s = schedule(
            &snap(
                vec![
                    task("M1", Pending, &[], None, None),
                    task("M2", Conflict, &[], None, None),
                    task("M3", NeedsFix, &[], None, None),
                ],
                vec![worker("w1", Some("medium"))],
            ),
            100,
        );
        assert_eq!(
            s.assignments
                .iter()
                .map(|a| (a.task.as_str(), a.job.as_str()))
                .collect::<Vec<_>>(),
            [("M2", "fix")],
            "the one coder goes to the conflict job first"
        );
        assert_eq!(
            hold(&s, "M1"),
            Some(&Hold::CodersBusy),
            "the only coder went to the fix jobs"
        );
    }

    #[test]
    fn a_coder_holding_a_fix_job_is_left_alone() {
        let s = schedule(
            &snap(
                vec![task("M1", NeedsFix, &[], Some("w1"), None)],
                vec![worker("w1", Some("medium"))],
            ),
            100,
        );
        assert!(s.assignments.is_empty());
    }

    #[test]
    fn ties_break_in_plan_order() {
        let s = schedule(
            &snap(
                vec![
                    task("M2", Pending, &[], None, None),
                    task("M1", Pending, &[], None, None),
                ],
                vec![worker("w1", Some("medium"))],
            ),
            100,
        );
        // Equal positions: the snapshot's task order is the tiebreak, M2 first here.
        assert_eq!(assigned(&s, "M2"), Some("w1"));
        assert_eq!(hold(&s, "M1"), Some(&Hold::CodersBusy));
    }

    #[test]
    fn benched_agents_are_never_assigned_until_the_bench_expires() {
        let mut s = snap(
            vec![task("M1", Pending, &[], None, None)],
            vec![worker("w1", Some("medium"))],
        );
        s.benches.push(Bench {
            agent: "w1".into(),
            reason: "rate-limit".into(),
            until: 200,
        });
        let held = schedule(&s, 100);
        assert_eq!(hold(&held, "M1"), Some(&Hold::CodersBusy));
        assert!(held.assignments.is_empty());
        let free = schedule(&s, 200);
        assert_eq!(assigned(&free, "M1"), Some("w1"));
    }

    #[test]
    fn a_busy_or_benched_coder_does_not_block_the_other_milestones() {
        let mut s = snap(
            vec![
                task("M1", Coding, &[], Some("w1"), None),
                task("M2", Pending, &[], None, None),
                task("M3", Pending, &[], None, None),
            ],
            vec![
                worker("w1", Some("medium")),
                worker("w2", Some("medium")),
                worker("w3", Some("medium")),
            ],
        );
        s.benches.push(Bench {
            agent: "w2".into(),
            reason: "gone".into(),
            until: 999,
        });
        let s = schedule(&s, 100);
        assert_eq!(
            assigned(&s, "M2"),
            Some("w3"),
            "the benched w2 is skipped, the busy w1 is not reused"
        );
        assert_eq!(hold(&s, "M3"), Some(&Hold::CodersBusy));
    }

    #[test]
    fn max_parallel_limits_the_pass() {
        let mut s = snap(
            vec![
                task("M1", Pending, &[], None, None),
                task("M2", Pending, &[], None, None),
                task("M3", Pending, &[], None, None),
            ],
            vec![
                worker("w1", Some("medium")),
                worker("w2", Some("medium")),
                worker("w3", Some("medium")),
            ],
        );
        s.max_parallel = 2;
        let s = schedule(&s, 100);
        assert_eq!(s.assignments.len(), 2);
        assert_eq!(
            s.unassigned
                .iter()
                .map(|u| u.hold.clone())
                .collect::<Vec<_>>(),
            [Hold::NoSlots(2)]
        );
        assert_eq!(
            s.unassigned[0].hold.line("M3"),
            "M3: no free slot (max parallel 2)"
        );
    }

    #[test]
    fn milestones_sharing_files_run_one_after_the_other() {
        let s = schedule(
            &snap(
                vec![
                    with_files(task("M1", Coding, &[], Some("w1"), None), &["src/lib.rs"]),
                    with_files(task("M2", Pending, &[], None, None), &["src/lib.rs"]),
                ],
                vec![worker("w1", Some("medium")), worker("w2", Some("medium"))],
            ),
            100,
        );
        assert_eq!(hold(&s, "M2"), Some(&Hold::FilesBusy(vec!["M1".into()])));
        assert_eq!(
            s.unassigned[0].hold.line("M2"),
            "WAIT M2 shares files with M1"
        );
    }

    #[test]
    fn parallel_allow_ignores_the_file_clash() {
        let mut t2 = with_files(task("M2", Pending, &[], None, None), &["src/lib.rs"]);
        t2.parallel = true;
        let s = schedule(
            &snap(
                vec![
                    with_files(task("M1", Coding, &[], Some("w1"), None), &["src/lib.rs"]),
                    t2,
                ],
                vec![worker("w1", Some("medium")), worker("w2", Some("medium"))],
            ),
            100,
        );
        assert_eq!(assigned(&s, "M2"), Some("w2"));
    }

    #[test]
    fn an_assignment_in_the_same_pass_claims_its_files_for_the_rest() {
        let s = schedule(
            &snap(
                vec![
                    with_files(task("M1", Pending, &[], None, None), &["src/lib.rs"]),
                    with_files(task("M2", Pending, &[], None, None), &["src/lib.rs"]),
                ],
                vec![worker("w1", Some("medium")), worker("w2", Some("medium"))],
            ),
            100,
        );
        assert_eq!(assigned(&s, "M1"), Some("w1"));
        assert_eq!(hold(&s, "M2"), Some(&Hold::FilesBusy(vec!["M1".into()])));
    }

    #[test]
    fn shared_mode_has_no_file_serialization() {
        let mut s = snap(
            vec![
                with_files(task("M1", Coding, &[], Some("w1"), None), &["src/lib.rs"]),
                with_files(task("M2", Pending, &[], None, None), &["src/lib.rs"]),
            ],
            vec![worker("w1", Some("medium"))],
        );
        s.shared = true;
        s.max_parallel = 1;
        let s = schedule(&s, 100);
        assert!(s.assignments.is_empty());
        assert_eq!(
            hold(&s, "M2"),
            Some(&Hold::NoSlots(1)),
            "shared mode waits for the slot, not for the files"
        );
    }

    #[test]
    fn a_lead_held_milestone_waits_for_the_dispatch() {
        let mut s = snap(
            vec![task("M1", Pending, &[], None, None)],
            vec![worker("w1", Some("medium"))],
        );
        s.lead_on = true;
        let held = schedule(&s, 100);
        assert_eq!(hold(&held, "M1"), Some(&Hold::LeadHold));
        assert_eq!(
            held.unassigned[0].hold.line("M1"),
            "HOLD M1 is ready and waits for the lead: orch dispatch M1"
        );
        s.intents.insert(
            "M1".into(),
            Intent {
                go: true,
                ..Intent::default()
            },
        );
        let go = schedule(&s, 100);
        assert_eq!(assigned(&go, "M1"), Some("w1"));
    }

    #[test]
    fn without_a_lead_no_go_is_needed() {
        let s = schedule(
            &snap(
                vec![task("M1", Pending, &[], None, None)],
                vec![worker("w1", Some("medium"))],
            ),
            100,
        );
        assert_eq!(assigned(&s, "M1"), Some("w1"));
    }

    #[test]
    fn fix_jobs_ignore_the_lead_hold() {
        let mut s = snap(
            vec![task("M1", NeedsFix, &[], None, None)],
            vec![worker("w1", Some("medium"))],
        );
        s.lead_on = true;
        let s = schedule(&s, 100);
        assert_eq!(assigned(&s, "M1"), Some("w1"));
    }

    #[test]
    fn shared_mode_frees_one_slot_for_a_queued_fix_job() {
        let mut s = snap(
            vec![task("M1", NeedsFix, &[], None, None)],
            vec![worker("w1", Some("medium"))],
        );
        s.shared = true;
        s.max_parallel = 1;
        let s = schedule(&s, 100);
        assert_eq!(assigned(&s, "M1"), Some("w1"));
    }

    #[test]
    fn shared_mode_gives_the_fix_job_its_slot_but_not_more() {
        let mut s = snap(
            vec![
                task("M1", NeedsFix, &[], None, None),
                task("M2", Pending, &[], None, None),
            ],
            vec![worker("w1", Some("medium"))],
        );
        s.shared = true;
        s.max_parallel = 1;
        let s = schedule(&s, 100);
        assert_eq!(assigned(&s, "M1"), Some("w1"));
        assert_eq!(
            hold(&s, "M2"),
            Some(&Hold::NoSlots(1)),
            "the freed slot went to the fix job, not to the new milestone"
        );
    }

    // ---- pick (Python cmd_pick)

    #[test]
    fn pick_returns_the_first_free_suitable_coder() {
        let s = snap(
            vec![with_tier(task("M1", Pending, &[], None, None), "medium")],
            vec![worker("w1", Some("small")), worker("w2", Some("medium"))],
        );
        assert_eq!(pick(&s, "M1", 100).as_deref(), Some("w2"));
    }

    #[test]
    fn pick_answers_none_when_nobody_is_free() {
        let mut s = snap(
            vec![task("M1", Pending, &[], None, None)],
            vec![worker("w1", Some("medium"))],
        );
        s.benches.push(Bench {
            agent: "w1".into(),
            reason: "gone".into(),
            until: 500,
        });
        assert_eq!(pick(&s, "M1", 100), None);
        assert_eq!(pick(&s, "M1", 500).as_deref(), Some("w1"));
        assert_eq!(pick(&s, "M9", 100), None);
    }

    #[test]
    fn pick_prefers_the_last_worker_over_the_asked_tier() {
        let s = snap(
            vec![with_tier(
                task("M1", NeedsFix, &[], None, Some("hi")),
                "medium",
            )],
            vec![worker("hi", Some("high")), worker("mid", Some("medium"))],
        );
        assert_eq!(pick(&s, "M1", 100).as_deref(), Some("hi"));
    }

    // ---- escalation (Python cmd_escalate)

    #[test]
    fn escalation_moves_once_to_the_next_stronger_tier() {
        let team = [Some("medium"), Some("small"), Some("high")];
        assert_eq!(
            escalate(false, Some("medium"), 2, &team),
            Escalation::ToTier("high")
        );
        let team2 = [Some("small"), Some("medium"), Some("high")];
        assert_eq!(
            escalate(false, Some("small"), 1, &team2),
            Escalation::ToTier("medium")
        );
    }

    #[test]
    fn escalation_answers_no_when_there_is_nothing_stronger() {
        let team = [Some("medium"), Some("small")];
        assert_eq!(
            escalate(false, Some("high"), 2, &team),
            Escalation::NoEscalation
        );
        assert_eq!(
            escalate(false, Some("medium"), 3, &team),
            Escalation::NoEscalation
        );
    }

    #[test]
    fn escalation_answers_no_for_a_second_request_or_no_current_coder() {
        let team = [Some("medium"), Some("high")];
        assert_eq!(
            escalate(true, Some("medium"), 2, &team),
            Escalation::NoEscalation
        );
        assert_eq!(escalate(false, None, 2, &team), Escalation::NoEscalation);
        assert_eq!(
            escalate(false, Some("weird"), 2, &team),
            Escalation::NoEscalation
        );
    }

    // ---- the stored intent and benches (§16, migration 5)

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
        (dir, s)
    }

    fn add(s: &mut Store, id: &str, deps: &[&str]) {
        s.add_task(&NewTask {
            id: id.into(),
            title: format!("task {id}"),
            deps: deps.iter().map(|d| d.to_string()).collect(),
        })
        .unwrap();
    }

    #[test]
    fn intent_roundtrips_and_clears() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        assert!(s.intent("M1").unwrap().is_none());
        let i = Intent {
            go: true,
            assign: Some("w2".into()),
            min_tier: Some("high".into()),
            ready_ts: Some(42),
        };
        s.set_intent("M1", &i).unwrap();
        assert_eq!(s.intent("M1").unwrap(), Some(i.clone()));
        let i2 = Intent {
            go: false,
            ..Intent::default()
        };
        s.set_intent("M1", &i2).unwrap();
        assert_eq!(s.intent("M1").unwrap(), Some(i2), "set replaces");
        s.clear_intent("M1").unwrap();
        assert!(s.intent("M1").unwrap().is_none());
        s.clear_intent("M1").unwrap(); // still fine without a row
        assert!(matches!(
            s.set_intent("M9", &i),
            Err(StoreError::NoSuchTask(_))
        ));
        assert!(matches!(
            s.clear_intent("M9"),
            Err(StoreError::NoSuchTask(_))
        ));
    }

    #[test]
    fn benches_roundtrip_and_expire() {
        let (_d, mut s) = store();
        s.bench_agent("w1", "rate-limit", 200).unwrap();
        s.bench_agent("w2", "gone", 100).unwrap();
        s.bench_agent("w1", "unreachable", 300).unwrap(); // replaces
        assert_eq!(
            s.active_benches(150).unwrap(),
            vec![Bench {
                agent: "w1".into(),
                reason: "unreachable".into(),
                until: 300
            }]
        );
        assert_eq!(s.benches().unwrap().len(), 2);
        s.end_bench("w1").unwrap();
        s.end_bench("w1").unwrap(); // idempotent
        assert!(s.benches().unwrap().iter().all(|b| b.agent == "w2"));
    }

    #[test]
    fn the_snapshot_comes_from_the_stored_state() {
        let (_dir, mut s) = store();
        assert!(s.scheduler_snapshot().unwrap().is_none());
        // A minimal stored run: settings, one worker, two planned tasks.
        s.conn
            .execute_batch(
                "INSERT INTO run (id, branch, title, isolation, max_parallel, lead_mode, synced_at)
                 VALUES (1,'feature/x','X','worktree',2,'off',1);
                 INSERT INTO run_agents (role, name, kind, tier, detect, position)
                    VALUES ('worker','w1','claude','medium',1,0);
                 INSERT INTO tasks (id, title) VALUES ('M1','one'),('M2','two');
                 INSERT INTO task_deps VALUES ('M2','M1');
                 INSERT INTO task_plan (task_id, position, enabled, tier, prefer, likely_files, parallel)
                    VALUES ('M1',0,1,'medium','any','[\"src/a.rs\"]',0),
                           ('M2',1,1,NULL,'any','[]',0);",
            )
            .unwrap();
        s.set_intent(
            "M1",
            &Intent {
                go: true,
                ready_ts: Some(7),
                ..Intent::default()
            },
        )
        .unwrap();
        s.bench_agent("w1", "rate-limit", 10_000).unwrap();
        let snap = s.scheduler_snapshot().unwrap().unwrap();
        assert_eq!(snap.max_parallel, 2);
        assert!(!snap.shared && !snap.lead_on);
        assert_eq!(
            snap.workers,
            vec![SchedWorker {
                name: "w1".into(),
                kind: "claude".into(),
                tier: Some("medium".into())
            }]
        );
        assert_eq!(snap.tasks.len(), 2);
        assert_eq!(snap.tasks[0].tier.as_deref(), Some("medium"));
        assert_eq!(snap.tasks[0].files, ["src/a.rs"]);
        assert_eq!(snap.intents.get("M1").map(|i| i.go), Some(true));
        assert_eq!(snap.benches.len(), 1);
        // The schedule from the snapshot: M1 held by the bench, M2 by its dependency.
        let out = schedule(&snap, 5);
        assert!(out.assignments.is_empty());
        let out = schedule(&snap, 20_000);
        assert_eq!(assigned(&out, "M1"), Some("w1"));
        // Once M1 merged, M2 is ready for the next pass.
        drop(snap);
        s.transition(
            "M1",
            &TransitionRequest::manual(Merged, Some("accepted"), true),
        )
        .unwrap();
        let snap = s.scheduler_snapshot().unwrap().unwrap();
        let out = schedule(&snap, 20_000);
        assert_eq!(assigned(&out, "M2"), Some("w1"));
    }

    #[test]
    fn every_active_status_makes_its_worker_busy() {
        // The busy derivation must agree with run_store::is_active for all 15 states.
        let mut active: Vec<&str> = State::ALL
            .iter()
            .filter(|s| is_active(**s))
            .map(|s| s.as_str())
            .collect();
        active.sort();
        assert_eq!(
            active,
            [
                "awaiting-review",
                "coding",
                "committed",
                "conflict",
                "held",
                "needs-fix",
                "reviewed",
                "reviewing",
                "verifying"
            ]
        );
    }
}
