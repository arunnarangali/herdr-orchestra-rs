//! The lead's event queue (§9, preserve 4.7.0): every event the lead must hear about is stored
//! durably, in the same transaction as the change that caused it. Reading the queue with
//! `orch events` is the acknowledgement ([`Store::mark_lead_read`]).
//!
//! * [`enqueue`] writes one entry inside a transaction the caller already holds. The
//!   transition path ([`Store::transition`]) calls it itself for the states the lead must
//!   decide on and for `merged`, so callers never enqueue those by hand; `ready`,
//!   `escalated`, `auto-dispatched`, `all_done`, `stopped` and `AnswerProvided` are enqueued
//!   by the code that causes them ([`Store::enqueue_lead_event`], [`emit_answer_provided`]).
//! * A queue only exists for a run with a lead (`Lead mode: events` and a lead agent).
//! * A blocking entry is open while its milestone is still in that state; a `ready` entry is
//!   stale once nothing it names is left to dispatch; every other entry is open until read.

use crate::store::{Res, Store, StoreError, now};
use crate::task::State;
use rusqlite::{Connection, OptionalExtension, params};

/// The queue keeps this many of the newest entries.
pub const QUEUE_KEEP: i64 = 200;

/// What an entry tells the lead about. The stored name is [`LeadKind::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeadKind {
    Paused,
    Failed,
    Held,
    NeedsAnswer,
    Merged,
    Ready,
    Escalated,
    AutoDispatched,
    AllDone,
    Stopped,
    /// An answer was given (`orch answer`), by the lead or by the developer in their own terminal.
    AnswerProvided,
}

impl LeadKind {
    pub const ALL: [LeadKind; 11] = [
        LeadKind::Paused,
        LeadKind::Failed,
        LeadKind::Held,
        LeadKind::NeedsAnswer,
        LeadKind::Merged,
        LeadKind::Ready,
        LeadKind::Escalated,
        LeadKind::AutoDispatched,
        LeadKind::AllDone,
        LeadKind::Stopped,
        LeadKind::AnswerProvided,
    ];

    /// The name stored in the queue (Python's kinds; `AnswerProvided` is new).
    pub fn as_str(self) -> &'static str {
        match self {
            LeadKind::Paused => "paused",
            LeadKind::Failed => "failed",
            LeadKind::Held => "held",
            LeadKind::NeedsAnswer => "needs-answer",
            LeadKind::Merged => "merged",
            LeadKind::Ready => "ready",
            LeadKind::Escalated => "escalated",
            LeadKind::AutoDispatched => "auto-dispatched",
            LeadKind::AllDone => "all_done",
            LeadKind::Stopped => "stopped",
            LeadKind::AnswerProvided => "AnswerProvided",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The kind a transition into `to` queues (`to` must be a lead event state).
    pub fn from_state(to: State) -> Self {
        match to {
            State::Paused => LeadKind::Paused,
            State::Failed => LeadKind::Failed,
            State::Held => LeadKind::Held,
            State::NeedsAnswer => LeadKind::NeedsAnswer,
            _ => LeadKind::Merged,
        }
    }

    /// Blocking kinds stay open until their milestone leaves that state.
    pub fn is_blocking(self) -> bool {
        matches!(
            self,
            LeadKind::Paused | LeadKind::Failed | LeadKind::Held | LeadKind::NeedsAnswer
        )
    }

    /// The kind as `orch events` prints it (Python: `kind.upper()`).
    pub fn heading(self) -> String {
        match self {
            LeadKind::AnswerProvided => "ANSWER_PROVIDED".to_string(),
            k => k.as_str().to_uppercase(),
        }
    }
}

/// One queue entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeadEntry {
    pub id: i64,
    pub ts: i64,
    pub kind: LeadKind,
    pub task: Option<String>,
    pub text: String,
    /// When the lead read it (`orch events`); `None` while unread.
    pub read_at: Option<i64>,
}

/// Does this run have a lead whose events are queued: `Lead mode: events` and a lead agent.
pub fn lead_on(conn: &Connection) -> Res<bool> {
    Ok(conn.query_row(
        "SELECT COALESCE((SELECT lead_mode='events' FROM run WHERE id=1), 0)
            AND EXISTS(SELECT 1 FROM run_agents WHERE role='lead')",
        [],
        |r| r.get(0),
    )?)
}

/// Queue one entry inside the caller's open transaction. Does nothing (returns `None`) when
/// the run has no lead, or when an unread entry with the same kind, milestone and text is
/// already queued. Keeps only the newest [`QUEUE_KEEP`] entries.
pub fn enqueue(
    conn: &Connection,
    kind: LeadKind,
    task: Option<&str>,
    text: &str,
) -> Res<Option<i64>> {
    if !lead_on(conn)? {
        return Ok(None);
    }
    let dup: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM lead_queue
                       WHERE read_at IS NULL AND kind=?1 AND task_id IS ?2 AND text=?3)",
        params![kind.as_str(), task, text],
        |r| r.get(0),
    )?;
    if dup {
        return Ok(None);
    }
    conn.execute(
        "INSERT INTO lead_queue (ts, kind, task_id, text) VALUES (?1,?2,?3,?4)",
        params![now(), kind.as_str(), task, text],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute("DELETE FROM lead_queue WHERE id <= ?1", [id - QUEUE_KEEP])?;
    Ok(Some(id))
}

/// Queue an `AnswerProvided` entry for `task` inside the caller's transaction. Every answer
/// emits one, whoever gave it: the lead, or the developer typing `orch answer` themselves
/// (§15A item 2), so the lead always hears of it.
pub fn emit_answer_provided(conn: &Connection, task: &str, text: &str) -> Res<Option<i64>> {
    enqueue(conn, LeadKind::AnswerProvided, Some(task), text)
}

/// The ids `M<digits>` in `text` (Python: `re.findall(r"M\d+", text)`).
fn milestone_ids(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'M' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 {
                out.push(text[i..j].to_string());
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn row_entry(r: &rusqlite::Row<'_>) -> rusqlite::Result<LeadEntry> {
    let kind: String = r.get(2)?;
    let kind = LeadKind::parse(&kind).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            format!("unknown lead event kind {kind:?}").into(),
        )
    })?;
    Ok(LeadEntry {
        id: r.get(0)?,
        ts: r.get(1)?,
        kind,
        task: r.get(3)?,
        text: r.get(4)?,
        read_at: r.get(5)?,
    })
}

impl Store {
    /// Does the run have a lead whose events are queued?
    pub fn lead_on(&self) -> Res<bool> {
        lead_on(&self.conn)
    }

    /// Queue one entry (its own transaction). See [`enqueue`] to do it inside a transition.
    pub fn enqueue_lead_event(
        &mut self,
        kind: LeadKind,
        task: Option<&str>,
        text: &str,
    ) -> Res<Option<i64>> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let id = enqueue(&tx, kind, task, text)?;
        tx.commit()?;
        Ok(id)
    }

    /// Emit the `AnswerProvided` entry for an answer (its own transaction).
    pub fn emit_answer_provided(&mut self, task: &str, text: &str) -> Res<Option<i64>> {
        self.enqueue_lead_event(LeadKind::AnswerProvided, Some(task), text)
    }

    /// Every stored entry, oldest first.
    pub fn lead_entries(&self) -> Res<Vec<LeadEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, ts, kind, task_id, text, read_at FROM lead_queue ORDER BY id")?;
        Ok(stmt
            .query_map([], row_entry)?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Is the entry still open? A blocking entry while its milestone is in that state, a
    /// `ready` entry while something it names waits to be dispatched, others until read.
    pub fn lead_entry_open(&self, e: &LeadEntry) -> Res<bool> {
        if e.kind.is_blocking() {
            let status: Option<String> = match &e.task {
                Some(t) => self
                    .conn
                    .query_row("SELECT status FROM tasks WHERE id=?1", [t], |r| r.get(0))
                    .optional()?,
                None => None,
            };
            return Ok(status.as_deref() == Some(e.kind.as_str()));
        }
        if e.kind == LeadKind::Ready {
            if e.read_at.is_some() {
                return Ok(false);
            }
            let waiting = self.undispatched()?;
            return Ok(milestone_ids(&e.text).iter().any(|m| waiting.contains(m)));
        }
        Ok(e.read_at.is_none())
    }

    /// Unread entries that are still open, oldest first (what `orch events` shows as new and
    /// what a wake-up is for).
    pub fn unread_lead_events(&self) -> Res<Vec<LeadEntry>> {
        let mut out = Vec::new();
        for e in self.lead_entries()? {
            if e.read_at.is_none() && self.lead_entry_open(&e)? {
                out.push(e);
            }
        }
        Ok(out)
    }

    /// Entries the lead has read whose blocking state still holds (`STILL OPEN`).
    pub fn still_open_lead_events(&self) -> Res<Vec<LeadEntry>> {
        let mut out = Vec::new();
        for e in self.lead_entries()? {
            if e.read_at.is_some() && e.kind.is_blocking() && self.lead_entry_open(&e)? {
                out.push(e);
            }
        }
        Ok(out)
    }

    /// The acknowledgement: mark entries read now. Returns how many were unread.
    pub fn mark_lead_read(&mut self, ids: &[i64]) -> Res<usize> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut n = 0;
        for id in ids {
            n += tx.execute(
                "UPDATE lead_queue SET read_at=?2 WHERE id=?1 AND read_at IS NULL",
                params![id, now()],
            )?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Ready milestones still waiting for the lead's `orch dispatch`, in plan order: pending,
    /// every dependency merged, not yet started. Empty when the run has no lead.
    pub fn undispatched(&self) -> Res<Vec<String>> {
        if !self.lead_on()? {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT t.id FROM tasks t LEFT JOIN task_plan p ON p.task_id = t.id
             WHERE t.status='pending'
               AND NOT EXISTS(SELECT 1 FROM task_go g WHERE g.task_id = t.id)
               AND NOT EXISTS(SELECT 1 FROM task_deps d LEFT JOIN tasks dt ON dt.id = d.dep_id
                              WHERE d.task_id = t.id AND COALESCE(dt.status, '') <> 'merged')
             ORDER BY COALESCE(p.position, 1000000000), t.rowid",
        )?;
        Ok(stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Record that a milestone was started (by `orch dispatch`, or by the event manager after
    /// its wait), so it is no longer "waiting for the lead".
    pub fn mark_dispatched(&mut self, task: &str) -> Res<()> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
            [task],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(StoreError::NoSuchTask(task.to_string()));
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO task_go (task_id, ts) VALUES (?1, ?2)",
            params![task, now()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
    use crate::store::{NewTask, TransitionRequest};
    use State::*;

    pub(crate) fn settings(lead_mode: &str) -> RunSettings {
        RunSettings {
            branch: "feature/x".into(),
            base: None,
            title: String::new(),
            commits: "per-milestone".into(),
            isolation: "worktree".into(),
            max_parallel: 2,
            setup_inputs: vec![],
            scope: "warn".into(),
            risk_hold: true,
            budget_s: None,
            check_command: String::new(),
            setup_command: "none".into(),
            lead_mode: lead_mode.into(),
            reviewer: None,
        }
    }

    pub(crate) fn agent(role: AgentRole, name: &str) -> RunAgent {
        RunAgent {
            role,
            name: name.into(),
            kind: "claude".into(),
            model: None,
            tier: None,
            detect: true,
            pane: None,
        }
    }

    /// A store holding a synced run with milestones M1 and M2 (M2 depends on M1), with or
    /// without a lead.
    pub(crate) fn store_with_run(lead: bool) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
        let mut agents = vec![agent(AgentRole::Worker, "claude-1")];
        if lead {
            agents.push(agent(AgentRole::Lead, "lead"));
        }
        let task = |id: &str, deps: &[&str]| PlanTask {
            id: id.into(),
            title: format!("task {id}"),
            deps: deps.iter().map(|d| d.to_string()).collect(),
            enabled: true,
            open_questions: false,
            tier: Some("medium".into()),
            needs: vec![],
            model_why: String::new(),
            prefer: "any".into(),
            review: false,
            files: vec![],
            parallel: false,
        };
        s.apply_sync(&SyncInput {
            settings: settings(if lead { "events" } else { "off" }),
            agents,
            tasks: vec![task("M1", &[]), task("M2", &["M1"])],
        })
        .unwrap();
        (dir, s)
    }

    fn go(s: &mut Store, id: &str, to: State) {
        s.transition(id, &TransitionRequest::auto(to, Some("why")))
            .unwrap();
    }

    #[test]
    fn kind_names_round_trip_and_answer_provided_is_exact() {
        for k in LeadKind::ALL {
            assert_eq!(LeadKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(LeadKind::AnswerProvided.as_str(), "AnswerProvided");
        assert_eq!(LeadKind::NeedsAnswer.as_str(), "needs-answer");
        assert_eq!(LeadKind::AllDone.as_str(), "all_done");
    }

    // AC1
    #[test]
    fn a_lead_transition_queues_its_entry_with_the_change() {
        let (_d, mut s) = store_with_run(true);
        go(&mut s, "M1", Coding);
        assert!(
            s.lead_entries().unwrap().is_empty(),
            "coding is not a lead event"
        );
        go(&mut s, "M1", Paused);
        go(&mut s, "M1", Pending);
        go(&mut s, "M1", Failed);
        let q = s.lead_entries().unwrap();
        let kinds: Vec<_> = q
            .iter()
            .map(|e| (e.kind, e.task.as_deref(), e.text.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                (LeadKind::Paused, Some("M1"), "why"),
                (LeadKind::Failed, Some("M1"), "why")
            ]
        );
    }

    #[test]
    fn merged_held_and_needs_answer_are_queued_too() {
        let (_d, mut s) = store_with_run(true);
        s.transition(
            "M2",
            &TransitionRequest::auto(NeedsAnswer, Some("open questions")),
        )
        .unwrap();
        s.transition(
            "M1",
            &TransitionRequest::manual(Merged, Some("landed"), true),
        )
        .unwrap();
        s.transition("M2", &TransitionRequest::manual(Held, None, true))
            .unwrap();
        let k: Vec<_> = s.lead_entries().unwrap().iter().map(|e| e.kind).collect();
        assert_eq!(k, [LeadKind::NeedsAnswer, LeadKind::Merged, LeadKind::Held]);
    }

    #[test]
    fn the_entry_text_falls_back_to_the_stored_reason() {
        let (_d, mut s) = store_with_run(true);
        s.transition("M1", &TransitionRequest::auto(Paused, Some("stuck")))
            .unwrap();
        s.transition("M1", &TransitionRequest::auto(Pending, None))
            .unwrap();
        s.transition("M1", &TransitionRequest::auto(Failed, None))
            .unwrap();
        let q = s.lead_entries().unwrap();
        assert_eq!(q.last().unwrap().text, "stuck");
    }

    #[test]
    fn a_failed_transaction_stores_neither_the_state_nor_the_entry() {
        let (_d, mut s) = store_with_run(true);
        {
            let tx = s.conn.transaction().unwrap();
            crate::store::apply_transition(&tx, "M1", &TransitionRequest::auto(Paused, Some("x")))
                .unwrap();
            let n: i64 = tx
                .query_row("SELECT COUNT(*) FROM lead_queue", [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 1, "queued inside the transaction");
            tx.rollback().unwrap();
        }
        assert_eq!(s.get_task("M1").unwrap().status, Pending);
        assert!(s.lead_entries().unwrap().is_empty());
        assert!(s.history("M1").unwrap().is_empty());
    }

    #[test]
    fn a_rejected_transition_queues_nothing() {
        let (_d, mut s) = store_with_run(true);
        assert!(
            s.transition("M1", &TransitionRequest::auto(Merged, Some("no")))
                .is_err()
        );
        assert!(s.lead_entries().unwrap().is_empty());
    }

    #[test]
    fn a_run_without_a_lead_queues_nothing() {
        let (_d, mut s) = store_with_run(false);
        go(&mut s, "M1", Paused);
        assert!(!s.lead_on().unwrap());
        assert!(s.lead_entries().unwrap().is_empty());
        assert_eq!(
            s.enqueue_lead_event(LeadKind::Stopped, None, "x").unwrap(),
            None
        );
    }

    #[test]
    fn a_store_with_no_run_queues_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
        s.add_task(&NewTask {
            id: "M1".into(),
            title: "t".into(),
            deps: vec![],
        })
        .unwrap();
        go(&mut s, "M1", Paused);
        assert!(s.lead_entries().unwrap().is_empty());
    }

    // AC2
    #[test]
    fn an_unread_duplicate_is_stored_once_a_read_one_again() {
        let (_d, mut s) = store_with_run(true);
        let a = s
            .enqueue_lead_event(LeadKind::Escalated, Some("M1"), "stronger")
            .unwrap();
        assert!(a.is_some());
        assert_eq!(
            s.enqueue_lead_event(LeadKind::Escalated, Some("M1"), "stronger")
                .unwrap(),
            None
        );
        assert!(
            s.enqueue_lead_event(LeadKind::Escalated, Some("M1"), "other")
                .unwrap()
                .is_some()
        );
        assert!(
            s.enqueue_lead_event(LeadKind::Escalated, Some("M2"), "stronger")
                .unwrap()
                .is_some()
        );
        // a milestone-less entry dedupes on NULL too
        assert!(
            s.enqueue_lead_event(LeadKind::Stopped, None, "x")
                .unwrap()
                .is_some()
        );
        assert_eq!(
            s.enqueue_lead_event(LeadKind::Stopped, None, "x").unwrap(),
            None
        );
        s.mark_lead_read(&[a.unwrap()]).unwrap();
        assert!(
            s.enqueue_lead_event(LeadKind::Escalated, Some("M1"), "stronger")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn the_queue_keeps_the_last_200() {
        let (_d, mut s) = store_with_run(true);
        for i in 0..230 {
            s.enqueue_lead_event(LeadKind::Stopped, None, &format!("n{i}"))
                .unwrap();
        }
        let q = s.lead_entries().unwrap();
        assert_eq!(q.len(), 200);
        assert_eq!(q.first().unwrap().text, "n30");
        assert_eq!(q.last().unwrap().text, "n229");
    }

    // AC3
    #[test]
    fn a_blocking_entry_stays_open_until_its_milestone_leaves_the_state() {
        let (_d, mut s) = store_with_run(true);
        go(&mut s, "M1", Paused);
        let un = s.unread_lead_events().unwrap();
        assert_eq!(un.len(), 1);
        s.mark_lead_read(&[un[0].id]).unwrap();
        assert!(s.unread_lead_events().unwrap().is_empty());
        let open = s.still_open_lead_events().unwrap();
        assert_eq!(open.len(), 1, "read but still open");
        go(&mut s, "M1", Pending);
        assert!(s.still_open_lead_events().unwrap().is_empty());
    }

    #[test]
    fn an_unread_blocking_entry_whose_milestone_moved_on_is_not_unread() {
        let (_d, mut s) = store_with_run(true);
        go(&mut s, "M1", Paused);
        go(&mut s, "M1", Pending);
        assert!(s.unread_lead_events().unwrap().is_empty());
    }

    #[test]
    fn a_ready_entry_goes_stale_when_nothing_it_names_is_left_to_dispatch() {
        let (_d, mut s) = store_with_run(true);
        assert_eq!(s.undispatched().unwrap(), ["M1"]);
        s.enqueue_lead_event(LeadKind::Ready, None, "M1").unwrap();
        assert_eq!(s.unread_lead_events().unwrap().len(), 1);
        s.mark_dispatched("M1").unwrap();
        assert!(s.undispatched().unwrap().is_empty());
        assert!(s.unread_lead_events().unwrap().is_empty(), "stale");
    }

    #[test]
    fn other_entries_are_open_until_read() {
        let (_d, mut s) = store_with_run(true);
        let id = s
            .enqueue_lead_event(LeadKind::Merged, Some("M1"), "landed")
            .unwrap()
            .unwrap();
        assert_eq!(s.unread_lead_events().unwrap().len(), 1);
        assert_eq!(s.mark_lead_read(&[id]).unwrap(), 1);
        assert_eq!(s.mark_lead_read(&[id]).unwrap(), 0, "already read");
        assert!(s.unread_lead_events().unwrap().is_empty());
        assert!(
            s.still_open_lead_events().unwrap().is_empty(),
            "not blocking"
        );
    }

    #[test]
    fn undispatched_needs_merged_dependencies_and_a_lead() {
        let (_d, mut s) = store_with_run(true);
        assert_eq!(s.undispatched().unwrap(), ["M1"]);
        s.transition("M1", &TransitionRequest::manual(Merged, Some("x"), true))
            .unwrap();
        assert_eq!(s.undispatched().unwrap(), ["M2"]);
        assert!(matches!(
            s.mark_dispatched("nope"),
            Err(StoreError::NoSuchTask(_))
        ));
        let (_d2, s2) = store_with_run(false);
        assert!(s2.undispatched().unwrap().is_empty());
    }

    // AC5
    #[test]
    fn every_answer_emits_answer_provided() {
        let (_d, mut s) = store_with_run(true);
        let id = s.emit_answer_provided("M1", "use sqlite").unwrap().unwrap();
        let e = s.unread_lead_events().unwrap().remove(0);
        assert_eq!(
            (e.id, e.kind, e.task.as_deref(), e.text.as_str()),
            (id, LeadKind::AnswerProvided, Some("M1"), "use sqlite")
        );
        let raw: String = s
            .conn
            .query_row("SELECT kind FROM lead_queue WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(raw, "AnswerProvided");
        // inside a caller's transaction too
        let tx = s.conn.transaction().unwrap();
        assert!(emit_answer_provided(&tx, "M2", "yes").unwrap().is_some());
    }

    #[test]
    fn milestone_ids_are_found_like_the_python_regex() {
        assert_eq!(milestone_ids("M1, M12 and M3x"), ["M1", "M12", "M3"]);
        assert!(milestone_ids("none MM").is_empty());
    }
}
