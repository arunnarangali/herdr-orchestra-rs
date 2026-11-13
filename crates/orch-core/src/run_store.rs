//! Run storage for the plan engine (§16): the validated run that `orch sync` writes and
//! `orch status` / `orch set` read. The run settings, the team and each milestone's plan
//! fields live in the `run`, `run_agents` and `task_plan` tables (migration 3); a milestone's
//! progress (state, attempt, counters, history) stays in `tasks` / `history`, so re-syncing a
//! plan never loses it.

use crate::store::{Res, Store, StoreError, TransitionRequest, apply_transition, load_task, now};
use crate::task::State;
use rusqlite::{OptionalExtension, params};
use std::collections::HashSet;

/// The run settings from the plan header (what Python keeps at the top of `state.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSettings {
    pub branch: String,
    pub base: Option<String>,
    pub title: String,
    pub commits: String,
    pub isolation: String,
    pub max_parallel: i64,
    pub setup_inputs: Vec<String>,
    pub scope: String,
    pub risk_hold: bool,
    pub budget_s: Option<i64>,
    pub check_command: String,
    pub setup_command: String,
    pub lead_mode: String,
    /// The reviewer's name, when the plan has one.
    pub reviewer: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRole {
    Worker,
    Reviewer,
    Lead,
}

impl AgentRole {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentRole::Worker => "worker",
            AgentRole::Reviewer => "reviewer",
            AgentRole::Lead => "lead",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "reviewer" => AgentRole::Reviewer,
            "lead" => AgentRole::Lead,
            _ => AgentRole::Worker,
        }
    }
}

/// One agent of the plan's team.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunAgent {
    pub role: AgentRole,
    pub name: String,
    pub kind: String,
    pub model: Option<String>,
    pub tier: Option<String>,
    /// Whether herdr watches this kind (false for a pane-addressed agent).
    pub detect: bool,
    pub pane: Option<String>,
}

/// One milestone as `orch sync` hands it to the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTask {
    pub id: String,
    pub title: String,
    pub deps: Vec<String>,
    pub enabled: bool,
    /// Python's `has_open_questions`.
    pub open_questions: bool,
    pub tier: Option<String>,
    pub needs: Vec<String>,
    pub model_why: String,
    pub prefer: String,
    pub review: bool,
    pub files: Vec<String>,
    pub parallel: bool,
}

/// Everything one `orch sync` writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncInput {
    pub settings: RunSettings,
    pub agents: Vec<RunAgent>,
    pub tasks: Vec<PlanTask>,
}

/// A stored milestone's plan fields, in plan order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedTask {
    pub id: String,
    pub position: i64,
    pub enabled: bool,
    pub tier: Option<String>,
    pub needs: Vec<String>,
    pub model_why: String,
    pub prefer: String,
    pub review: bool,
    pub files: Vec<String>,
    pub parallel: bool,
}

/// The stored run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub settings: RunSettings,
    pub agents: Vec<RunAgent>,
    pub plan: Vec<PlannedTask>,
}

/// States where a coder is working the milestone (Python `ACTIVE`).
pub fn is_active(s: State) -> bool {
    use State::*;
    matches!(
        s,
        Coding
            | Verifying
            | AwaitingReview
            | Reviewing
            | Reviewed
            | Committed
            | Conflict
            | NeedsFix
            | Held
    )
}

fn json_list(v: &[String]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn from_json(s: &str) -> Vec<String> {
    serde_json::from_str(s).unwrap_or_default()
}

/// The state Python gives a milestone on a re-sync, and why (the reason Python records).
fn synced_state(old: State, enabled: bool, open_questions: bool) -> (State, &'static str) {
    let mut s = old;
    if !enabled && !is_active(s) && s != State::Merged {
        s = State::Skipped;
    } else if enabled && s == State::Skipped {
        s = State::Pending;
    }
    if s == State::Pending && open_questions {
        s = State::NeedsAnswer;
    } else if s == State::NeedsAnswer && !open_questions {
        s = State::Pending;
    }
    let why = match s {
        State::Skipped => "unticked in plan.md",
        State::NeedsAnswer => "open questions in plan.md",
        _ => "",
    };
    (s, why)
}

fn py_list(items: &[String]) -> String {
    let q: Vec<String> = items.iter().map(|i| format!("'{i}'")).collect();
    format!("[{}]", q.join(", "))
}

impl Store {
    /// The run settings, `None` before the first `orch sync`.
    pub fn run_settings(&self) -> Res<Option<RunSettings>> {
        Ok(self
            .conn
            .query_row(
                "SELECT branch, base, title, commits, isolation, max_parallel, setup_inputs, scope,
                        risk_hold, budget_s, check_command, setup_command, lead_mode, reviewer
                 FROM run WHERE id=1",
                [],
                |r| {
                    Ok(RunSettings {
                        branch: r.get(0)?,
                        base: r.get(1)?,
                        title: r.get(2)?,
                        commits: r.get(3)?,
                        isolation: r.get(4)?,
                        max_parallel: r.get(5)?,
                        setup_inputs: from_json(&r.get::<_, String>(6)?),
                        scope: r.get(7)?,
                        risk_hold: r.get(8)?,
                        budget_s: r.get(9)?,
                        check_command: r.get(10)?,
                        setup_command: r.get(11)?,
                        lead_mode: r.get(12)?,
                        reviewer: r.get(13)?,
                    })
                },
            )
            .optional()?)
    }

    /// The stored run (settings, team, milestone plan fields), `None` before the first sync.
    pub fn load_run(&self) -> Res<Option<Run>> {
        let Some(settings) = self.run_settings()? else {
            return Ok(None);
        };
        let mut stmt = self.conn.prepare(
            "SELECT role, name, kind, model, tier, detect, pane FROM run_agents
             ORDER BY CASE role WHEN 'worker' THEN 0 WHEN 'reviewer' THEN 1 ELSE 2 END, position",
        )?;
        let agents = stmt
            .query_map([], |r| {
                Ok(RunAgent {
                    role: AgentRole::parse(&r.get::<_, String>(0)?),
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    model: r.get(3)?,
                    tier: r.get(4)?,
                    detect: r.get(5)?,
                    pane: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut stmt = self.conn.prepare(
            "SELECT task_id, position, enabled, tier, needs, model_why, prefer, review,
                    likely_files, parallel FROM task_plan ORDER BY position",
        )?;
        let plan = stmt
            .query_map([], |r| {
                Ok(PlannedTask {
                    id: r.get(0)?,
                    position: r.get(1)?,
                    enabled: r.get(2)?,
                    tier: r.get(3)?,
                    needs: from_json(&r.get::<_, String>(4)?),
                    model_why: r.get(5)?,
                    prefer: r.get(6)?,
                    review: r.get(7)?,
                    files: from_json(&r.get::<_, String>(8)?),
                    parallel: r.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Some(Run {
            settings,
            agents,
            plan,
        }))
    }

    /// Store a validated plan, in one transaction (Python's `cmd_sync` tail):
    ///
    /// * every milestone already stored keeps its state, attempt, counters and history;
    /// * new milestones are added `pending`;
    /// * an unticked milestone that is not in progress becomes `skipped`, a re-ticked one
    ///   `pending`, and open questions move `pending` <-> `needs-answer`;
    /// * a milestone depending on a skipped one is parked as `needs-answer`;
    /// * a coder whose milestone is no longer in progress is freed.
    ///
    /// Returns Python's warnings for the last two cases.
    pub fn apply_sync(&mut self, input: &SyncInput) -> Res<Vec<String>> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut warns = Vec::new();
        let s = &input.settings;
        tx.execute(
            "INSERT OR REPLACE INTO run (id, branch, base, title, commits, isolation, max_parallel,
                 setup_inputs, scope, risk_hold, budget_s, check_command, setup_command, lead_mode,
                 reviewer, synced_at)
             VALUES (1,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                s.branch,
                s.base,
                s.title,
                s.commits,
                s.isolation,
                s.max_parallel,
                json_list(&s.setup_inputs),
                s.scope,
                s.risk_hold,
                s.budget_s,
                s.check_command,
                s.setup_command,
                s.lead_mode,
                s.reviewer,
                now()
            ],
        )?;
        tx.execute("DELETE FROM run_agents", [])?;
        for (pos, a) in input.agents.iter().enumerate() {
            tx.execute(
                "INSERT OR REPLACE INTO run_agents (role, name, kind, model, tier, detect, pane, position)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![a.role.as_str(), a.name, a.kind, a.model, a.tier, a.detect, a.pane, pos as i64],
            )?;
        }

        tx.execute("DELETE FROM task_plan", [])?;
        let mut plan_ids = HashSet::new();
        for (pos, t) in input.tasks.iter().enumerate() {
            plan_ids.insert(t.id.clone());
            let old = load_task(&tx, &t.id)?;
            if old.is_some() {
                tx.execute(
                    "UPDATE tasks SET title=?2 WHERE id=?1",
                    params![t.id, t.title],
                )?;
            } else {
                tx.execute(
                    "INSERT INTO tasks (id, title) VALUES (?1, ?2)",
                    params![t.id, t.title],
                )?;
            }
            tx.execute("DELETE FROM task_deps WHERE task_id=?1", [&t.id])?;
            for d in &t.deps {
                tx.execute(
                    "INSERT OR IGNORE INTO task_deps (task_id, dep_id) VALUES (?1, ?2)",
                    params![t.id, d],
                )?;
            }
            tx.execute(
                "INSERT INTO task_plan (task_id, position, enabled, tier, needs, model_why, prefer,
                     review, likely_files, parallel)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    t.id,
                    pos as i64,
                    t.enabled,
                    t.tier,
                    json_list(&t.needs),
                    t.model_why,
                    t.prefer,
                    t.review,
                    json_list(&t.files),
                    t.parallel
                ],
            )?;
            let old_state = old.map_or(State::Pending, |o| o.status);
            let (new, why) = synced_state(old_state, t.enabled, t.open_questions);
            if new != old_state {
                apply_transition(&tx, &t.id, &TransitionRequest::manual(new, Some(why), true))?;
            }
        }

        // A milestone that depends on a skipped one waits for the developer.
        for t in &input.tasks {
            let mut skipped = Vec::new();
            for d in &t.deps {
                if load_task(&tx, d)?.is_some_and(|dt| dt.status == State::Skipped) {
                    skipped.push(d.clone());
                }
            }
            let status = load_task(&tx, &t.id)?.map(|x| x.status);
            if !skipped.is_empty() && matches!(status, Some(State::Pending | State::NeedsAnswer)) {
                let why = format!(
                    "depends on skipped {} — ask the developer",
                    skipped.join(", ")
                );
                apply_transition(
                    &tx,
                    &t.id,
                    &TransitionRequest::manual(State::NeedsAnswer, Some(&why), true),
                )?;
                warns.push(format!("{} depends on skipped {}", t.id, py_list(&skipped)));
            }
        }

        // A coder cannot stay tied to a milestone that left the plan or is no longer in progress.
        for a in input.agents.iter().filter(|a| a.role == AgentRole::Worker) {
            let mut stmt =
                tx.prepare("SELECT id, status FROM tasks WHERE worker=?1 ORDER BY rowid")?;
            let held: Vec<(String, String)> = stmt
                .query_map([&a.name], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            drop(stmt);
            for (id, status) in held {
                let active = status.parse::<State>().is_ok_and(is_active);
                if !plan_ids.contains(&id) || !active {
                    warns.push(format!(
                        "{} was holding {id}, which is no longer in progress — freed",
                        a.name
                    ));
                    tx.execute("UPDATE tasks SET worker=NULL WHERE id=?1", [&id])?;
                }
            }
        }
        tx.commit()?;
        Ok(warns)
    }

    /// `orch set`: change a milestone's state by hand (the §11 table applies unless `force`).
    /// Moving to `pending`, `paused` or `failed` also frees the milestone's coder, as Python's
    /// `park` does. Records the history row and event.
    pub fn manual_set(
        &mut self,
        id: &str,
        to: State,
        reason: &str,
        force: bool,
    ) -> Res<crate::store::Outcome> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let task = load_task(&tx, id)?.ok_or_else(|| StoreError::NoSuchTask(id.to_string()))?;
        let out = apply_transition(&tx, id, &TransitionRequest::manual(to, Some(reason), force))?;
        if matches!(to, State::Pending | State::Paused | State::Failed) && task.worker.is_some() {
            tx.execute(
                "UPDATE tasks SET last_worker=worker, worker=NULL WHERE id=?1",
                [id],
            )?;
        }
        tx.commit()?;
        Ok(out)
    }
}
