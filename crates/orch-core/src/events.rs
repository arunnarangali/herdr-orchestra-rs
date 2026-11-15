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
    /// The agent waits for a permission or an answer in its pane. `prompt` is the pane's
    /// visible prompt text (the command or question), when herdr could read it (§15A).
    Blocked { task: String, agent: String, prompt: Option<String> },
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
            RunEvent::Blocked { task, agent, .. } => format!("BLOCKED {task} {agent}"),
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
            RunEvent::Blocked { task, agent, prompt } => match prompt {
                Some(p) => format!("BLOCKED {task} agent={agent} (wants: {p})"),
                None => format!("BLOCKED {task} agent={agent}"),
            },
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
                prompt: rest
                    .find(" (wants: ")
                    .and_then(|i| {
                        rest[i + " (wants: ".len()..]
                            .strip_suffix(')')
                            .map(String::from)
                    }),
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
                    prompt: prompt_from_output(self.adapter.output(&d.agent, PROMPT_LINES)),
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

/// How many recent output lines the blocked-pane prompt is looked for in (§15A: a
/// permission prompt sits at the bottom of the pane).
pub const PROMPT_LINES: u32 = 8;

/// The pane's visible prompt text for a blocked agent (§15A item 1): the last non-empty
/// line of its recent output, control characters and terminal escapes removed, capped to
/// one line of 160 characters. `None` when the output cannot be read or is blank.
pub fn prompt_from_output(out: Option<String>) -> Option<String> {
    let line = out?
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())?
        .trim()
        .to_string();
    let text: String = strip_control(&line).trim().chars().take(160).collect();
    (!text.is_empty()).then_some(text)
}

/// Remove terminal escape sequences (`ESC [ … letter`) and other control characters.
fn strip_control(line: &str) -> String {
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

/// One recorded task state transition event read from the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEventRecord {
    pub id: i64,
    pub ts: i64,
    pub task: String,
    pub from: String,
    pub to: String,
    pub why: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub attempt: u32,
    pub forced: bool,
}

/// One lead queue record read from the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeadQueueRecord {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub task: Option<String>,
    pub text: String,
    pub read_at: Option<i64>,
    /// The blocked agent the entry names (schema 10, `blocked` entries only).
    pub agent: Option<String>,
    /// The pane's visible prompt text the blocked agent wants answered.
    pub prompt: Option<String>,
}

/// Initialise max event id and max lead queue id from `state.db`.
pub fn init_db_poll_state(db_path: &std::path::Path) -> (i64, i64) {
    if !db_path.exists() {
        return (0, 0);
    }
    let Ok(conn) = rusqlite::Connection::open(db_path) else {
        return (0, 0);
    };
    let last_event: i64 = conn
        .query_row("SELECT COALESCE(MAX(id), 0) FROM events", [], |r| r.get(0))
        .unwrap_or(0);
    let last_queue: i64 = conn
        .query_row("SELECT COALESCE(MAX(id), 0) FROM lead_queue", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);
    (last_event, last_queue)
}

/// Poll newly added events from `state.db` (read-only mode).
pub fn poll_db_events(db_path: &std::path::Path, last_id: &mut i64) -> Vec<TaskEventRecord> {
    if !db_path.exists() {
        return Vec::new();
    }
    let Ok(conn) = rusqlite::Connection::open(db_path) else {
        return Vec::new();
    };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(200));

    let Ok(mut stmt) = conn.prepare(
        "SELECT id, ts, task_id, from_state, to_state, why, agent, model, attempt, forced
         FROM events WHERE id > ?1 ORDER BY id",
    ) else {
        return Vec::new();
    };

    let rows = stmt.query_map([*last_id], |r| {
        Ok(TaskEventRecord {
            id: r.get(0)?,
            ts: r.get(1)?,
            task: r.get(2)?,
            from: r.get(3)?,
            to: r.get(4)?,
            why: r.get(5)?,
            agent: r.get(6)?,
            model: r.get(7)?,
            attempt: r.get(8)?,
            forced: r.get(9)?,
        })
    });

    let mut out = Vec::new();
    if let Ok(rows) = rows {
        for row in rows.flatten() {
            if row.id > *last_id {
                *last_id = row.id;
            }
            out.push(row);
        }
    }
    out
}

/// Poll newly added lead queue entries from `state.db` (read-only mode).
pub fn poll_lead_queue_events(
    db_path: &std::path::Path,
    last_id: &mut i64,
) -> Vec<LeadQueueRecord> {
    if !db_path.exists() {
        return Vec::new();
    }
    let Ok(conn) = rusqlite::Connection::open(db_path) else {
        return Vec::new();
    };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(200));

    let Ok(mut stmt) = conn.prepare(
        "SELECT id, ts, kind, task_id, text, read_at, agent, prompt
         FROM lead_queue WHERE id > ?1 ORDER BY id",
    ) else {
        return Vec::new();
    };

    let rows = stmt.query_map([*last_id], |r| {
        Ok(LeadQueueRecord {
            id: r.get(0)?,
            ts: r.get(1)?,
            kind: r.get(2)?,
            task: r.get(3)?,
            text: r.get(4)?,
            read_at: r.get(5)?,
            agent: r.get(6)?,
            prompt: r.get(7)?,
        })
    });

    let mut out = Vec::new();
    if let Ok(rows) = rows {
        for row in rows.flatten() {
            if row.id > *last_id {
                *last_id = row.id;
            }
            out.push(row);
        }
    }
    out
}

/// One run-control record read from the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunControlRecord {
    pub state: String,
    pub source: String,
    pub since: i64,
}

/// One run action record read from the database audit trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunActionRecord {
    pub id: i64,
    pub ts: i64,
    pub source: String,
    pub action: String,
    pub task_id: Option<String>,
    pub detail: String,
}

/// Read current `run_control` row from `state.db` (read-only mode).
pub fn get_run_control(db_path: &std::path::Path) -> Option<RunControlRecord> {
    if !db_path.exists() {
        return None;
    }
    let Ok(conn) = rusqlite::Connection::open(db_path) else {
        return None;
    };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(200));

    conn.query_row(
        "SELECT state, source, since FROM run_control WHERE id=1",
        [],
        |r| {
            Ok(RunControlRecord {
                state: r.get(0)?,
                source: r.get(1)?,
                since: r.get(2)?,
            })
        },
    )
    .ok()
}

/// Poll newly added run actions from `state.db` (read-only mode).
pub fn poll_run_actions(db_path: &std::path::Path, last_id: &mut i64) -> Vec<RunActionRecord> {
    if !db_path.exists() {
        return Vec::new();
    }
    let Ok(conn) = rusqlite::Connection::open(db_path) else {
        return Vec::new();
    };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(200));

    let Ok(mut stmt) = conn.prepare(
        "SELECT id, ts, source, action, task_id, detail
         FROM run_actions WHERE id > ?1 ORDER BY id",
    ) else {
        return Vec::new();
    };

    let rows = stmt.query_map([*last_id], |r| {
        Ok(RunActionRecord {
            id: r.get(0)?,
            ts: r.get(1)?,
            source: r.get(2)?,
            action: r.get(3)?,
            task_id: r.get(4)?,
            detail: r.get(5)?,
        })
    });

    let mut out = Vec::new();
    if let Ok(rows) = rows {
        for row in rows.flatten() {
            if row.id > *last_id {
                *last_id = row.id;
            }
            out.push(row);
        }
    }
    out
}

/// Initialise max event id, max lead queue id, max action id, and current run control from `state.db`.
pub fn init_db_poll_state_all(
    db_path: &std::path::Path,
) -> (i64, i64, i64, Option<RunControlRecord>) {
    if !db_path.exists() {
        return (0, 0, 0, None);
    }
    let Ok(conn) = rusqlite::Connection::open(db_path) else {
        return (0, 0, 0, None);
    };
    let last_event: i64 = conn
        .query_row("SELECT COALESCE(MAX(id), 0) FROM events", [], |r| r.get(0))
        .unwrap_or(0);
    let last_queue: i64 = conn
        .query_row("SELECT COALESCE(MAX(id), 0) FROM lead_queue", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);
    let last_action: i64 = conn
        .query_row("SELECT COALESCE(MAX(id), 0) FROM run_actions", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);
    let run_control = conn
        .query_row(
            "SELECT state, source, since FROM run_control WHERE id=1",
            [],
            |r| {
                Ok(RunControlRecord {
                    state: r.get(0)?,
                    source: r.get(1)?,
                    since: r.get(2)?,
                })
            },
        )
        .ok();
    (last_event, last_queue, last_action, run_control)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Counter, NewTask, Store};

    // §15A item 1: the visible prompt text is the pane's last non-empty line.
    #[test]
    fn the_prompt_is_the_last_visible_line() {
        let out = "working...\n\nDo you want to allow `cargo test`? [y/n]\n".to_string();
        assert_eq!(
            prompt_from_output(Some(out)),
            Some("Do you want to allow `cargo test`? [y/n]".to_string())
        );
        // escapes are removed
        assert_eq!(prompt_from_output(Some("a\u{1b}[31mred\u{1b}[0m".into())), Some("ared".into()));
        // the last non-empty line wins, and lines are capped to 160 characters
        let out = format!("first line\n{}\n", "x".repeat(200));
        let p = prompt_from_output(Some(out)).unwrap();
        assert_eq!(p.chars().count(), 160);
        assert_eq!(p, "x".repeat(160));
        // nothing readable: None
        assert_eq!(prompt_from_output(None), None);
        assert_eq!(prompt_from_output(Some("\n \n".into())), None);
    }

    // §15A item 1: the BLOCKED event carries agent and prompt, and its line round-trips.
    #[test]
    fn the_blocked_event_carries_the_agent_and_the_prompt() {
        let ev = RunEvent::Blocked {
            task: "M1".into(),
            agent: "codex-1".into(),
            prompt: Some("cargo test --workspace".into()),
        };
        assert_eq!(ev.agent(), Some("codex-1"));
        assert_eq!(ev.key(), "BLOCKED M1 codex-1");
        let line = ev.line();
        assert_eq!(line, "BLOCKED M1 agent=codex-1 (wants: cargo test --workspace)");
        assert_eq!(RunEvent::parse(&line), Some(ev.clone()));
        // without a prompt the line is the old one
        let ev = RunEvent::Blocked { task: "M1".into(), agent: "w1".into(), prompt: None };
        assert_eq!(ev.line(), "BLOCKED M1 agent=w1");
        assert_eq!(RunEvent::parse(&ev.line()), Some(ev));
    }

    struct Rt {
        st: Option<AgentState>,
        out: Option<String>,
    }
    impl RuntimeView for Rt {
        fn status(&self, _agent: &str) -> Option<AgentState> {
            self.st
        }
        fn output(&self, _agent: &str, _lines: u32) -> Option<String> {
            self.out.clone()
        }
    }

    fn watch_all(_: &str) -> bool {
        true
    }

    fn one_in_flight(dir: &std::path::Path, agent: &str) -> Store {
        let mut s = Store::open_in(&dir.join(".orchestrate")).unwrap();
        s.add_task(&NewTask { id: "M1".into(), title: "t".into(), deps: vec![] }).unwrap();
        s.bump("M1", Counter::Attempt).unwrap();
        s.claim("M1", agent, None).unwrap();
        s.record_delivery(agent, "M1", 1, DeliveryKind::Coder, "go").unwrap();
        let d = s.latest_delivery("M1", 1, DeliveryKind::Coder).unwrap().unwrap();
        s.confirm_delivery(d.id).unwrap();
        s
    }

    // §15A item 1: the poll reads the pane's prompt text into the event.
    #[test]
    fn the_scan_puts_the_panes_prompt_text_into_the_blocked_event() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = one_in_flight(dir.path(), "w1");
        let rt = Rt {
            st: Some(AgentState::Blocked),
            out: Some("\nAllow? [y/n] cargo build\n".into()),
        };
        let poll = HerdrPoll {
            root: dir.path().to_path_buf(),
            adapter: &rt,
            watchable: &watch_all,
            tun: PollTunables {
                step_s: 0.01,
                idle_grace_s: 120,
                confirm_s: 30,
                coder_budget_s: 2700,
                review_budget_s: 900,
            },
        };
        let evs = poll.scan(&mut store);
        assert!(
            matches!(
                evs.first(),
                Some(RunEvent::Blocked { agent, prompt, .. })
                    if agent == "w1" && prompt.as_deref() == Some("Allow? [y/n] cargo build")
            ),
            "{evs:?}"
        );
        // output herdr cannot read: the event still carries the agent, without a prompt
        let rt = Rt { st: Some(AgentState::Blocked), out: None };
        let poll = HerdrPoll { adapter: &rt, ..poll };
        let evs = poll.scan(&mut store);
        assert!(
            matches!(
                evs.first(),
                Some(RunEvent::Blocked { agent, prompt: None, .. }) if agent == "w1"
            ),
            "{evs:?}"
        );
    }
}
