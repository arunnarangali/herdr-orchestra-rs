//! The runtime events the event manager acts on, and the polling that produces them
//! (§9: Python's `orch poll` vocabulary — `DONE`, `REVIEWED`, `UNCONFIRMED`, `ORPHAN`,
//! `BLOCKED`, `IDLE`, `TIMEOUT`, `GONE`).
//!
//! The loop polls herdr and done files as the normal path (§24 Phase 6): a bounded
//! scan over the milestones in flight, with long waits and a short return when no
//! agent is working. Every event carries an idempotency key ([`RunEvent::key`]) so a
//! replayed event can be recognized and ignored.

use crate::store::{Delivery, DeliveryKind, Store};
use std::path::PathBuf;

/// One runtime event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    /// The milestone's coder wrote its done file.
    Done { task: String, agent: String },
    /// The reviewer wrote its done file.
    Reviewed { task: String, agent: String },
    /// The prompt was recorded but never confirmed (interrupted run).
    Unconfirmed { task: String, agent: String },
    /// The milestone is in flight but no prompt ever went out.
    Orphan { task: String, status: String },
    /// The agent waits for a permission or an answer in its pane.
    Blocked { task: String, agent: String },
    /// The agent went idle without writing its done file.
    Idle { task: String, agent: String, after_s: i64 },
    /// The agent is over its time budget.
    Timeout { task: String, agent: String, after_min: i64 },
    /// herdr no longer knows the agent.
    Gone { task: String, agent: String },
}

/// The agent of the event, when it has one.
impl RunEvent {
    pub fn task(&self) -> &str {
        match self {
            RunEvent::Done { task, .. }
            | RunEvent::Reviewed { task, .. }
            | RunEvent::Unconfirmed { task, .. }
            | RunEvent::Orphan { task, .. }
            | RunEvent::Blocked { task, .. }
            | RunEvent::Idle { task, .. }
            | RunEvent::Timeout { task, .. }
            | RunEvent::Gone { task, .. } => task,
        }
    }

    pub fn agent(&self) -> Option<&str> {
        match self {
            RunEvent::Done { agent, .. }
            | RunEvent::Reviewed { agent, .. }
            | RunEvent::Unconfirmed { agent, .. }
            | RunEvent::Blocked { agent, .. }
            | RunEvent::Idle { agent, .. }
            | RunEvent::Timeout { agent, .. }
            | RunEvent::Gone { agent, .. } => Some(agent),
            RunEvent::Orphan { .. } => None,
        }
    }

    /// The idempotency key: one event kind per milestone and agent. A replayed event
    /// (a poll source re-reporting what was already applied) has the same key.
    pub fn key(&self) -> String {
        match self {
            RunEvent::Done { task, agent } => format!("DONE {task} {agent}"),
            RunEvent::Reviewed { task, agent } => format!("REVIEWED {task} {agent}"),
            RunEvent::Unconfirmed { task, agent } => format!("UNCONFIRMED {task} {agent}"),
            RunEvent::Orphan { task, .. } => format!("ORPHAN {task}"),
            RunEvent::Blocked { task, agent } => format!("BLOCKED {task} {agent}"),
            RunEvent::Idle { task, agent, after_s } => format!("IDLE {task} {agent} {after_s}"),
            RunEvent::Timeout { task, agent, after_min } => {
                format!("TIMEOUT {task} {agent} {after_min}")
            }
            RunEvent::Gone { task, agent } => format!("GONE {task} {agent}"),
        }
    }

    /// The poll line, as Python's `orch poll` prints it (also what a poll source
    /// emits into the log).
    pub fn line(&self) -> String {
        match self {
            RunEvent::Done { task, agent } => format!("DONE {task} worker={agent}"),
            RunEvent::Reviewed { task, agent } => format!("REVIEWED {task} reviewer={agent}"),
            RunEvent::Unconfirmed { task, agent } => {
                format!("UNCONFIRMED {task} agent={agent} (prompt recorded, not confirmed)")
            }
            RunEvent::Orphan { task, status } => {
                format!("ORPHAN {task} status={status} (no agent was prompted)")
            }
            RunEvent::Blocked { task, agent } => format!("BLOCKED {task} agent={agent}"),
            RunEvent::Idle { task, agent, after_s } => {
                format!("IDLE {task} agent={agent} (no sentinel after {after_s}s)")
            }
            RunEvent::Timeout { task, agent, after_min } => {
                format!("TIMEOUT {task} agent={agent} after {after_min} min")
            }
            RunEvent::Gone { task, agent } => format!("GONE {task} agent={agent}"),
        }
    }

    /// Parse a poll line back ([`RunEvent::line`]'s format); `None` for lines that
    /// are not events (like `WAITING`).
    pub fn parse(line: &str) -> Option<RunEvent> {
        let (head, rest) = line.split_once(' ')?;
        let task = rest.split(' ').next()?.to_string();
        let rest = &rest[task.len()..];
        let field = |name: &str| -> Option<String> {
            rest.split(" agent=")
                .nth(1)
                .or_else(|| rest.split(" worker=").nth(1))
                .or_else(|| rest.split(" reviewer=").nth(1))
                .and_then(|v| v.split(&[' ', ')'][..]).next())
                .map(|v| v.trim_end_matches(')').to_string())
                .or_else(|| {
                    // `UNCONFIRMED M1 agent=w1 (...)` keeps `agent=` first; other
                    // kinds put the agent field after the task id directly.
                    let want = format!(" {name}=");
                    rest.find(&want).and_then(|i| {
                        let v = &rest[i + want.len()..];
                        Some(
                            v.split([' ', ')'])
                                .next()?
                                .to_string(),
                        )
                    })
                })
        };
        let age = |what: &str| -> Option<i64> {
            let i = rest.find(what)?;
            let v = rest[i + what.len()..].trim_start();
            v.split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()
        };
        match head {
            "DONE" => Some(RunEvent::Done {
                task,
                agent: field("worker")?,
            }),
            "REVIEWED" => Some(RunEvent::Reviewed {
                task,
                agent: field("reviewer")?,
            }),
            "UNCONFIRMED" => Some(RunEvent::Unconfirmed {
                task,
                agent: field("agent")?,
            }),
            "ORPHAN" => {
                let status = rest
                    .strip_prefix(" status=")?
                    .split(' ')
                    .next()?
                    .to_string();
                Some(RunEvent::Orphan { task, status })
            }
            "BLOCKED" => Some(RunEvent::Blocked {
                task,
                agent: field("agent")?,
            }),
            "IDLE" => Some(RunEvent::Idle {
                task,
                agent: field("agent")?,
                after_s: age("after ")?,
            }),
            "TIMEOUT" => Some(RunEvent::Timeout {
                task,
                agent: field("agent")?,
                after_min: age("after ")?,
            }),
            "GONE" => Some(RunEvent::Gone {
                task,
                agent: field("agent")?,
            }),
            _ => None,
        }
    }
}

/// What the loop needs to know about one agent's state: herdr's statuses, reduced to
/// what the event vocabulary acts on. "Cannot tell" is `None` (the agent is not
/// watchable, or herdr could not be asked) — only the done file and the budget count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Working,
    Blocked,
    Idle,
    Done,
    /// herdr no longer knows the agent.
    Gone,
}

/// The narrow view of the runtime the loop and the reconciliation need (§4): one
/// agent's state and its recent output. herdr's full adapter (and its fake) sit
/// behind this trait, so orch-core carries no herdr code.
pub trait RuntimeView {
    /// The agent's state, `None` when it cannot be told (not watchable, unknown, or
    /// herdr unreachable).
    fn status(&self, agent: &str) -> Option<AgentState>;
    /// The agent's recent terminal output (rate-limit detection), `None` when it
    /// cannot be read.
    fn output(&self, agent: &str, lines: u32) -> Option<String>;
}

/// What one bounded poll returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    /// Events to act on now.
    Events(Vec<RunEvent>),
    /// The wait ran out with nothing to do; `nothing_running` says no milestone is
    /// in flight (the loop keeps a short cycle for benches and lead dispatches).
    Waiting { nothing_running: bool },
}

/// Where the loop's runtime events come from. `store` is passed per call: the poll
/// reads the milestones in flight and their deliveries from the same store the loop
/// drives.
pub trait PollSource {
    fn poll(&mut self, store: &mut Store, secs: u64) -> PollOutcome;
}

/// The poll timing the loop's poll source works from.
#[derive(Debug, Clone, Copy)]
pub struct PollTunables {
    /// `ORCH_POLL_STEP_S` (the sleep between scans).
    pub step_s: f64,
    /// `ORCH_IDLE_GRACE_S` (idle counts only after this).
    pub idle_grace_s: i64,
    /// `ORCH_CONFIRM_S` (a fresh unconfirmed delivery may still be in flight).
    pub confirm_s: i64,
    /// `ORCH_CODER_BUDGET_MIN` in seconds.
    pub coder_budget_s: i64,
    /// `ORCH_REVIEW_BUDGET_MIN` in seconds.
    pub review_budget_s: i64,
}

/// The normal-path poll: agent statuses plus done files under
/// `<root>/.orch-task/<Mn>/done-<attempt>`.
pub struct HerdrPoll<'a> {
    /// The run's main repository root (where `.orch-task/` lives).
    pub root: PathBuf,
    pub adapter: &'a dyn RuntimeView,
    /// Whether the runtime can report this agent's status (the plan's `detect`).
    pub watchable: &'a dyn Fn(&str) -> bool,
    pub tun: PollTunables,
}

impl PollSource for HerdrPoll<'_> {
    fn poll(&mut self, store: &mut Store, secs: u64) -> PollOutcome {
        let mut deadline = secs as f64;
        loop {
            let events = self.scan(store);
            if !events.is_empty() {
                return PollOutcome::Events(events);
            }
            // Nobody in flight: do not sit out the whole wait (benches end sooner).
            if self.nothing_in_flight(store) {
                return PollOutcome::Waiting { nothing_running: true };
            }
            let step = self.tun.step_s;
            if deadline <= step {
                // one bounded wait, then back to the loop
                return PollOutcome::Waiting { nothing_running: false };
            }
            deadline -= step;
            std::thread::sleep(std::time::Duration::from_secs_f64(step));
        }
    }
}

impl<'a> HerdrPoll<'a> {
    /// One scan of every milestone in flight; no waiting.
    pub fn scan(&self, store: &mut Store) -> Vec<RunEvent> {
        let now = crate::store::now();
        let mut out = Vec::new();
        for t in store.list_tasks().unwrap_or_default() {
            let kind = match t.status {
                crate::task::State::Coding => DeliveryKind::Coder,
                crate::task::State::Reviewing => DeliveryKind::Reviewer,
                _ => continue,
            };
            let Some(d) = latest_delivery(store, &t.id, t.attempt, kind) else {
                out.push(RunEvent::Orphan {
                    task: t.id.clone(),
                    status: t.status.as_str().to_string(),
                });
                continue;
            };
            if self.done_file(&t.id, t.attempt).exists() {
                if kind == DeliveryKind::Coder {
                    out.push(RunEvent::Done {
                        task: t.id.clone(),
                        agent: d.agent.clone(),
                    });
                } else {
                    out.push(RunEvent::Reviewed {
                        task: t.id.clone(),
                        agent: d.agent.clone(),
                    });
                }
                continue;
            }
            let age = now - d.recorded_at;
            if !d.confirmed {
                // recorded but never confirmed: a crash between record and send
                if age < self.tun.confirm_s {
                    continue;
                }
                if (self.watchable)(&d.agent)
                    && let Some(AgentState::Working | AgentState::Blocked) =
                        self.adapter.status(&d.agent)
                {
                    continue; // reached the agent after all
                }
                out.push(RunEvent::Unconfirmed {
                    task: t.id.clone(),
                    agent: d.agent.clone(),
                });
                continue;
            }
            let st = if (self.watchable)(&d.agent) {
                self.adapter.status(&d.agent)
            } else {
                None
            };
            match st {
                Some(AgentState::Gone) => out.push(RunEvent::Gone {
                    task: t.id.clone(),
                    agent: d.agent.clone(),
                }),
                Some(AgentState::Blocked) => out.push(RunEvent::Blocked {
                    task: t.id.clone(),
                    agent: d.agent.clone(),
                }),
                Some(s)
                    if age > self.tun.idle_grace_s
                        && matches!(s, AgentState::Idle | AgentState::Done) =>
                {
                    out.push(RunEvent::Idle {
                        task: t.id.clone(),
                        agent: d.agent.clone(),
                        after_s: age,
                    })
                }
                _ if age > self.budget(kind) => out.push(RunEvent::Timeout {
                    task: t.id.clone(),
                    agent: d.agent.clone(),
                    after_min: age / 60,
                }),
                _ => {}
            }
        }
        out
    }

    fn budget(&self, kind: DeliveryKind) -> i64 {
        if kind == DeliveryKind::Coder {
            self.tun.coder_budget_s
        } else {
            self.tun.review_budget_s
        }
    }

    /// `<root>/.orch-task/<Mn>/done-<attempt>` — the file the prompt names as the
    /// very last action.
    pub fn done_file(&self, task: &str, attempt: u32) -> PathBuf {
        self.task_dir(task).join(format!("done-{attempt}"))
    }

    /// `<root>/.orch-task/<Mn>`.
    pub fn task_dir(&self, task: &str) -> PathBuf {
        self.root.join(".orch-task").join(task)
    }

    fn nothing_in_flight(&self, store: &mut Store) -> bool {
        !store.list_tasks().unwrap_or_default().iter().any(|t| {
            matches!(
                t.status,
                crate::task::State::Coding | crate::task::State::Reviewing
            )
        })
    }
}

/// The newest delivery for `(task, attempt, kind)`, `None` when no prompt went out.
pub fn latest_delivery(
    store: &Store,
    task: &str,
    attempt: u32,
    kind: DeliveryKind,
) -> Option<Delivery> {
    store
        .latest_delivery(task, attempt, kind)
        .ok()
        .flatten()
}
