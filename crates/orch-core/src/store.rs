//! SQLite storage under `.orchestrate/state.db`.
//!
//! * Versioned migrations (`MIGRATIONS`, tracked in `PRAGMA user_version`) are applied on open.
//!   To add tables in a later phase: add `migrations/000N_name.sql` and append it to `MIGRATIONS`.
//! * A task's state changes only through [`Store::transition`]: validated against the §11 table,
//!   and the new state, history row and event row are written in one `BEGIN IMMEDIATE`
//!   transaction. The task is re-read inside that transaction, so concurrent writers are
//!   serialised and each transition is validated against the latest state (no lost updates).

use crate::error::Error;
use crate::task::{Mode, State, Task, TaskError, Transition, check_transition};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// File name of the database inside `.orchestrate/`.
pub const DB_FILE: &str = "state.db";

/// Ordered `(version, sql)` migrations. Versions are consecutive from 1.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_init.sql")),
    (2, include_str!("../migrations/0002_deliveries.sql")),
];

/// The schema version this build writes.
pub const SCHEMA_VERSION: i64 = MIGRATIONS[MIGRATIONS.len() - 1].0;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{} was written by a newer orch (schema {found}, this build supports {supported}) — upgrade herdr-orchestra", .path.display())]
    NewerSchema {
        path: PathBuf,
        found: i64,
        supported: i64,
    },
    #[error("unknown task {0}")]
    NoSuchTask(String),
    #[error("unknown delivery {0}")]
    NoSuchDelivery(i64),
    #[error("delivery {0} is already confirmed")]
    DeliveryConfirmed(i64),
    #[error("task {0} already exists")]
    DuplicateTask(String),
    #[error(transparent)]
    Task(#[from] TaskError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
}

impl From<StoreError> for Error {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Task(t) => t.into(),
            other => Error::wrap(other, crate::error::EXIT_USAGE),
        }
    }
}

type Res<T> = std::result::Result<T, StoreError>;

/// Who a prompt was delivered for: a coder working a task, or the reviewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryKind {
    Coder,
    Reviewer,
}

impl DeliveryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryKind::Coder => "coder",
            DeliveryKind::Reviewer => "reviewer",
        }
    }

    fn parse(s: &str) -> rusqlite::Result<Self> {
        match s {
            "coder" => Ok(DeliveryKind::Coder),
            "reviewer" => Ok(DeliveryKind::Reviewer),
            other => Err(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(TaskError::UnknownState(other.to_string())),
            )),
        }
    }
}

/// One pending (or confirmed) prompt delivery, recorded durably before the
/// prompt is sent and confirmed when the runtime accepts it (§15, §16).
#[derive(Debug, Clone, PartialEq)]
pub struct Delivery {
    pub id: i64,
    /// The one agent this prompt may be sent to; a re-send always uses it.
    pub agent: String,
    /// The task the prompt belongs to.
    pub task: String,
    /// The attempt the prompt starts.
    pub attempt: u32,
    pub kind: DeliveryKind,
    pub prompt: String,
    /// When the delivery was recorded (committed before the send).
    pub recorded_at: i64,
    pub confirmed: bool,
    pub confirmed_at: Option<i64>,
    /// Kind and message of the last send error, when the send failed.
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
}

/// A request to change a task's state.
#[derive(Debug, Clone)]
pub struct TransitionRequest {
    pub to: State,
    pub why: Option<String>,
    pub mode: Mode,
}

impl TransitionRequest {
    pub fn auto(to: State, why: Option<&str>) -> Self {
        Self {
            to,
            why: why.map(String::from),
            mode: Mode::Auto,
        }
    }
    pub fn manual(to: State, why: Option<&str>, force: bool) -> Self {
        Self {
            to,
            why: why.map(String::from),
            mode: if force { Mode::Forced } else { Mode::Manual },
        }
    }
}

/// What a transition did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Same state, no reason: nothing recorded.
    NoOp,
    /// Recorded (state changed, or same state with a reason).
    Recorded(Transition),
}

/// Per-task counters bumped atomically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Counter {
    Attempt,
    Fix,
    Review,
    Conflict,
    Lost,
}

impl Counter {
    fn column(self) -> &'static str {
        match self {
            Counter::Attempt => "attempt",
            Counter::Fix => "fix_rounds",
            Counter::Review => "review_rounds",
            Counter::Conflict => "conflict_rounds",
            Counter::Lost => "lost",
        }
    }
}

/// A new task (always starts `pending`).
#[derive(Debug, Clone, Default)]
pub struct NewTask {
    pub id: String,
    pub title: String,
    pub deps: Vec<String>,
}

/// One row of the events table (Python's `events.jsonl` fields plus model and lead flag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub id: i64,
    pub ts: i64,
    pub task: String,
    pub from: State,
    pub to: State,
    pub why: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub attempt: u32,
    pub forced: bool,
    pub lead_blocking: bool,
}

pub struct Store {
    conn: Connection,
    path: PathBuf,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn parse_state(s: String) -> rusqlite::Result<State> {
    s.parse().map_err(|e: TaskError| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

impl Store {
    /// Open (creating if needed) `<orchestrate_dir>/state.db`, creating the folder too.
    pub fn open_in(orchestrate_dir: &Path) -> Res<Store> {
        std::fs::create_dir_all(orchestrate_dir).map_err(|e| StoreError::Io {
            context: format!("creating {}", orchestrate_dir.display()),
            source: e,
        })?;
        Store::open(&orchestrate_dir.join(DB_FILE))
    }

    /// Open the database at `path` and apply pending migrations.
    pub fn open(path: &Path) -> Res<Store> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(10))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        // WAL lets readers run while one writer writes; ignore failure on exotic filesystems.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        migrate(&mut conn, path)?;
        Ok(Store {
            conn,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> Res<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    /// Insert a new `pending` task with its dependencies.
    pub fn add_task(&mut self, new: &NewTask) -> Res<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
            [&new.id],
            |r| r.get(0),
        )?;
        if exists {
            return Err(StoreError::DuplicateTask(new.id.clone()));
        }
        tx.execute(
            "INSERT INTO tasks (id, title) VALUES (?1, ?2)",
            params![new.id, new.title],
        )?;
        for d in &new.deps {
            tx.execute(
                "INSERT OR IGNORE INTO task_deps (task_id, dep_id) VALUES (?1, ?2)",
                params![new.id, d],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_task(&self, id: &str) -> Res<Task> {
        load_task(&self.conn, id)?.ok_or_else(|| StoreError::NoSuchTask(id.to_string()))
    }

    pub fn list_tasks(&self) -> Res<Vec<Task>> {
        let mut stmt = self.conn.prepare("SELECT id FROM tasks ORDER BY rowid")?;
        let ids: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        ids.iter().map(|i| self.get_task(i)).collect()
    }

    /// Derived check: pending with every dependency merged.
    pub fn is_ready(&self, id: &str) -> Res<bool> {
        let t = self.get_task(id)?;
        let mut statuses = std::collections::HashMap::new();
        for d in &t.deps {
            if let Some(dt) = load_task(&self.conn, d)? {
                statuses.insert(d.clone(), dt.status);
            }
        }
        Ok(t.is_ready(|d| statuses.get(d).copied()))
    }

    /// Change a task's state. Validated, and saved with its history and event atomically.
    pub fn transition(&mut self, id: &str, req: &TransitionRequest) -> Res<Outcome> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = apply_transition(&tx, id, req)?;
        tx.commit()?;
        Ok(out)
    }

    /// Set (or clear) the task's current worker and model. Not a state change. A set worker
    /// also becomes `last_worker`.
    pub fn assign_worker(
        &mut self,
        id: &str,
        worker: Option<&str>,
        model: Option<&str>,
    ) -> Res<()> {
        let n = self.conn.execute(
            "UPDATE tasks SET worker=?2, last_worker=COALESCE(?2, last_worker), model=COALESCE(?3, model) WHERE id=?1",
            params![id, worker, model],
        )?;
        if n == 0 {
            Err(StoreError::NoSuchTask(id.to_string()))
        } else {
            Ok(())
        }
    }

    /// Atomically add one to a counter; returns the new value.
    pub fn bump(&mut self, id: &str, counter: Counter) -> Res<u32> {
        let col = counter.column();
        let v = self
            .conn
            .query_row(
                &format!("UPDATE tasks SET {col}={col}+1 WHERE id=?1 RETURNING {col}"),
                [id],
                |r| r.get(0),
            )
            .optional()?;
        v.ok_or_else(|| StoreError::NoSuchTask(id.to_string()))
    }

    /// Record that the one escalation to a stronger model was used.
    pub fn mark_escalated(&mut self, id: &str) -> Res<()> {
        let n = self
            .conn
            .execute("UPDATE tasks SET escalated=1 WHERE id=?1", [id])?;
        if n == 0 {
            Err(StoreError::NoSuchTask(id.to_string()))
        } else {
            Ok(())
        }
    }

    /// A task's full transition history, oldest first.
    pub fn history(&self, id: &str) -> Res<Vec<Transition>> {
        let mut stmt = self.conn.prepare(
            "SELECT task_id, ts, from_state, to_state, why, worker, model, attempt, forced
             FROM history WHERE task_id=?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([id], |r| {
            Ok(Transition {
                task: r.get(0)?,
                ts: r.get(1)?,
                from: parse_state(r.get(2)?)?,
                to: parse_state(r.get(3)?)?,
                why: r.get(4)?,
                worker: r.get(5)?,
                model: r.get(6)?,
                attempt: r.get(7)?,
                forced: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// All events with id greater than `after`, oldest first.
    pub fn events_after(&self, after: i64) -> Res<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, task_id, from_state, to_state, why, agent, model, attempt, forced, lead_blocking
             FROM events WHERE id>?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([after], |r| {
            Ok(Event {
                id: r.get(0)?,
                ts: r.get(1)?,
                task: r.get(2)?,
                from: parse_state(r.get(3)?)?,
                to: parse_state(r.get(4)?)?,
                why: r.get(5)?,
                agent: r.get(6)?,
                model: r.get(7)?,
                attempt: r.get(8)?,
                forced: r.get(9)?,
                lead_blocking: r.get(10)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Record a prompt delivery, durably, before anything is sent to the
    /// runtime (§15, §16). Returns its id. The task must exist.
    pub fn record_delivery(
        &mut self,
        agent: &str,
        task: &str,
        attempt: u32,
        kind: DeliveryKind,
        prompt: &str,
    ) -> Res<i64> {
        self.conn.execute(
            "INSERT INTO deliveries (agent, task_id, attempt, kind, prompt, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![agent, task, attempt, kind.as_str(), prompt, now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// One delivery by id.
    pub fn delivery(&self, id: i64) -> Res<Delivery> {
        self.conn
            .query_row(
                &format!("{DELIVERY_SELECT} WHERE id=?1"),
                [id],
                row_delivery,
            )
            .optional()?
            .ok_or(StoreError::NoSuchDelivery(id))
    }

    /// Every unconfirmed delivery, oldest first (recovery walks these).
    pub fn unconfirmed_deliveries(&self) -> Res<Vec<Delivery>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{DELIVERY_SELECT} WHERE confirmed=0 ORDER BY id"))?;
        let rows = stmt.query_map([], row_delivery)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Mark a delivery confirmed (the runtime accepted the send). Idempotent
    /// rejection: confirming a confirmed or missing delivery is an error.
    pub fn confirm_delivery(&mut self, id: i64) -> Res<()> {
        let n = self.conn.execute(
            "UPDATE deliveries
             SET confirmed=1, confirmed_at=?2, error_kind=NULL, error_message=NULL
             WHERE id=?1 AND confirmed=0",
            params![id, now()],
        )?;
        if n == 0 {
            return Err(self.delivery_state_error(id));
        }
        Ok(())
    }

    /// Keep why a send failed: the delivery stays unconfirmed, with the error
    /// kind and message preserved for the report.
    pub fn record_delivery_error(&mut self, id: i64, error_kind: &str, message: &str) -> Res<()> {
        let n = self.conn.execute(
            "UPDATE deliveries SET error_kind=?2, error_message=?3 WHERE id=?1 AND confirmed=0",
            params![id, error_kind, message],
        )?;
        if n == 0 {
            return Err(self.delivery_state_error(id));
        }
        Ok(())
    }

    fn delivery_state_error(&self, id: i64) -> StoreError {
        let confirmed: Option<bool> = self
            .conn
            .query_row("SELECT confirmed FROM deliveries WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .unwrap_or(None);
        match confirmed {
            Some(true) => StoreError::DeliveryConfirmed(id),
            Some(false) => StoreError::NoSuchDelivery(id), // unreachable
            None => StoreError::NoSuchDelivery(id),
        }
    }

    /// Add one to the agent's consecutive delivery failures (§15); returns the
    /// new value.
    pub fn bump_delivery_fails(&mut self, agent: &str) -> Res<u32> {
        let v = self.conn.query_row(
            "INSERT INTO delivery_fails (agent, fails) VALUES (?1, 1)
             ON CONFLICT(agent) DO UPDATE SET fails=fails+1
             RETURNING fails",
            [agent],
            |r| r.get(0),
        )?;
        Ok(v)
    }

    /// A confirmed send resets the agent's consecutive-failure count.
    pub fn reset_delivery_fails(&mut self, agent: &str) -> Res<()> {
        self.conn.execute(
            "INSERT INTO delivery_fails (agent, fails) VALUES (?1, 0)
             ON CONFLICT(agent) DO UPDATE SET fails=0",
            [agent],
        )?;
        Ok(())
    }

    /// The agent's consecutive delivery failures (0 when none).
    pub fn delivery_fails(&self, agent: &str) -> Res<u32> {
        Ok(self
            .conn
            .query_row(
                "SELECT fails FROM delivery_fails WHERE agent=?1",
                [agent],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }
}

const DELIVERY_SELECT: &str = "SELECT id, agent, task_id, attempt, kind, prompt, recorded_at,
    confirmed, confirmed_at, error_kind, error_message FROM deliveries";

fn row_delivery(r: &rusqlite::Row<'_>) -> rusqlite::Result<Delivery> {
    Ok(Delivery {
        id: r.get(0)?,
        agent: r.get(1)?,
        task: r.get(2)?,
        attempt: r.get(3)?,
        kind: DeliveryKind::parse(&r.get::<_, String>(4)?)?,
        prompt: r.get(5)?,
        recorded_at: r.get(6)?,
        confirmed: r.get(7)?,
        confirmed_at: r.get(8)?,
        error_kind: r.get(9)?,
        error_message: r.get(10)?,
    })
}

fn migrate(conn: &mut Connection, path: &Path) -> Res<()> {
    // IMMEDIATE so two processes opening a fresh database don't both migrate.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(StoreError::NewerSchema {
            path: path.to_path_buf(),
            found: current,
            supported: SCHEMA_VERSION,
        });
    }
    for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
    }
    tx.commit()?;
    Ok(())
}

fn load_task(conn: &Connection, id: &str) -> Res<Option<Task>> {
    let row = conn
        .query_row(
            "SELECT id, title, status, reason, worker, last_worker, model, attempt,
                    fix_rounds, review_rounds, conflict_rounds, lost, escalated
             FROM tasks WHERE id=?1",
            [id],
            |r| {
                Ok(Task {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    status: parse_state(r.get(2)?)?,
                    reason: r.get(3)?,
                    deps: Vec::new(),
                    worker: r.get(4)?,
                    last_worker: r.get(5)?,
                    model: r.get(6)?,
                    attempt: r.get(7)?,
                    fix_rounds: r.get(8)?,
                    review_rounds: r.get(9)?,
                    conflict_rounds: r.get(10)?,
                    lost: r.get(11)?,
                    escalated: r.get(12)?,
                })
            },
        )
        .optional()?;
    let Some(mut t) = row else { return Ok(None) };
    let mut stmt = conn.prepare("SELECT dep_id FROM task_deps WHERE task_id=?1 ORDER BY rowid")?;
    t.deps = stmt
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(t))
}

fn apply_transition(tx: &Transaction<'_>, id: &str, req: &TransitionRequest) -> Res<Outcome> {
    let t = load_task(tx, id)?.ok_or_else(|| StoreError::NoSuchTask(id.to_string()))?;
    let (from, to) = (t.status, req.to);
    check_transition(id, from, to, req.mode)?;
    let why = req.why.clone().unwrap_or_default();
    if from == to && why.is_empty() {
        return Ok(Outcome::NoOp);
    }
    let forced = req.mode == Mode::Forced;
    let agent = t.worker.clone().or_else(|| t.last_worker.clone());
    let ts = now();

    // Like Python: the reason is only replaced when one is given.
    tx.execute(
        "UPDATE tasks SET status=?2, reason=CASE WHEN ?3 IS NULL THEN reason ELSE ?3 END WHERE id=?1",
        params![id, to.as_str(), req.why],
    )?;
    tx.execute(
        "INSERT INTO history (task_id, ts, from_state, to_state, why, worker, model, attempt, forced)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![id, ts, from.as_str(), to.as_str(), why, agent, t.model, t.attempt, forced],
    )?;
    let lead = from != to && to.is_lead_event();
    tx.execute(
        "INSERT INTO events (ts, task_id, from_state, to_state, why, agent, model, attempt, forced, lead_blocking)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![ts, id, from.as_str(), to.as_str(), why, agent, t.model, t.attempt, forced, lead],
    )?;
    Ok(Outcome::Recorded(Transition {
        task: id.to_string(),
        ts,
        from,
        to,
        why,
        worker: agent,
        model: t.model,
        attempt: t.attempt,
        forced,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use State::*;

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

    fn mv(s: &mut Store, id: &str, to: State) -> Res<Outcome> {
        s.transition(id, &TransitionRequest::auto(to, None))
    }

    #[test]
    fn open_creates_db_and_is_idempotent() {
        let (dir, s) = store();
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
        let p = s.path().to_path_buf();
        assert_eq!(p, dir.path().join(".orchestrate/state.db"));
        drop(s);
        let s2 = Store::open(&p).unwrap();
        assert_eq!(s2.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn newer_schema_is_refused() {
        let (_d, s) = store();
        let p = s.path().to_path_buf();
        s.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        drop(s);
        let e = Store::open(&p).err().unwrap();
        assert!(matches!(e, StoreError::NewerSchema { .. }));
        assert!(e.to_string().contains("newer orch"), "{e}");
        let core: Error = e.into();
        assert_eq!(core.exit_code(), 2);
    }

    #[test]
    fn add_and_get_task_with_defaults() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        add(&mut s, "M2", &["M1"]);
        let t = s.get_task("M2").unwrap();
        assert_eq!(t.status, Pending);
        assert_eq!(t.deps, ["M1"]);
        assert_eq!(
            (
                t.fix_rounds,
                t.review_rounds,
                t.conflict_rounds,
                t.lost,
                t.attempt
            ),
            (0, 0, 0, 0, 0)
        );
        assert!(!t.escalated);
        assert!(matches!(
            s.add_task(&NewTask {
                id: "M1".into(),
                ..Default::default()
            }),
            Err(StoreError::DuplicateTask(_))
        ));
        assert!(matches!(s.get_task("nope"), Err(StoreError::NoSuchTask(_))));
        assert_eq!(s.list_tasks().unwrap().len(), 2);
    }

    #[test]
    fn valid_transition_records_history_and_event() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        s.assign_worker("M1", Some("claude-1"), Some("sonnet"))
            .unwrap();
        let out = s
            .transition("M1", &TransitionRequest::auto(Coding, Some("dispatched")))
            .unwrap();
        let Outcome::Recorded(tr) = out else { panic!() };
        assert_eq!((tr.from, tr.to), (Pending, Coding));
        assert_eq!(tr.worker.as_deref(), Some("claude-1"));
        assert_eq!(tr.model.as_deref(), Some("sonnet"));
        assert!(!tr.forced && tr.ts > 0);
        let t = s.get_task("M1").unwrap();
        assert_eq!((t.status, t.reason.as_str()), (Coding, "dispatched"));
        assert!(t.is_assigned());
        assert_eq!(s.history("M1").unwrap(), vec![tr]);
        let ev = s.events_after(0).unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(
            (ev[0].from, ev[0].to, ev[0].why.as_str()),
            (Pending, Coding, "dispatched")
        );
        assert!(!ev[0].lead_blocking);
        assert_eq!(ev[0].agent.as_deref(), Some("claude-1"));
    }

    #[test]
    fn refused_transition_changes_nothing() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        let e = mv(&mut s, "M1", Merged).unwrap_err();
        assert!(
            e.to_string()
                .contains("pending -> merged is not an allowed transition"),
            "{e}"
        );
        assert_eq!(s.get_task("M1").unwrap().status, Pending);
        assert!(s.history("M1").unwrap().is_empty());
        assert!(s.events_after(0).unwrap().is_empty());
    }

    #[test]
    fn merged_is_final_even_for_manual() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        for to in [Coding, Verifying, Merged] {
            mv(&mut s, "M1", to).unwrap();
        }
        let e = mv(&mut s, "M1", Coding).unwrap_err();
        assert!(e.to_string().contains("merged is final"), "{e}");
        let e = s
            .transition("M1", &TransitionRequest::manual(Pending, None, false))
            .unwrap_err();
        assert!(e.to_string().contains("merged is final"), "{e}");
        assert_eq!(s.get_task("M1").unwrap().status, Merged);
    }

    #[test]
    fn same_state_noop_unless_reason() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        assert_eq!(mv(&mut s, "M1", Pending).unwrap(), Outcome::NoOp);
        assert!(s.history("M1").unwrap().is_empty());
        let out = s
            .transition(
                "M1",
                &TransitionRequest::auto(Pending, Some("still waiting")),
            )
            .unwrap();
        assert!(matches!(out, Outcome::Recorded(_)));
        assert_eq!(s.get_task("M1").unwrap().reason, "still waiting");
        assert_eq!(s.history("M1").unwrap().len(), 1);
        // a same-state record is not a lead event
        let (_d2, mut s2) = store();
        add(&mut s2, "M1", &[]);
        mv(&mut s2, "M1", Paused).unwrap();
        s2.transition("M1", &TransitionRequest::auto(Paused, Some("again")))
            .unwrap();
        let ev = s2.events_after(0).unwrap();
        assert_eq!(
            ev.iter().map(|e| e.lead_blocking).collect::<Vec<_>>(),
            [true, false]
        );
    }

    #[test]
    fn manual_and_forced() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        let e = s
            .transition("M1", &TransitionRequest::manual(Coding, None, false))
            .unwrap_err();
        assert!(e.to_string().contains("only moves a task to"), "{e}");
        assert_eq!(s.get_task("M1").unwrap().status, Pending);
        s.transition(
            "M1",
            &TransitionRequest::manual(Paused, Some("hold"), false),
        )
        .unwrap();
        // forced: skips the table and is recorded as forced
        let Outcome::Recorded(tr) = s
            .transition(
                "M1",
                &TransitionRequest::manual(Merged, Some("by hand"), true),
            )
            .unwrap()
        else {
            panic!()
        };
        assert!(tr.forced);
        let h = s.history("M1").unwrap();
        assert_eq!(
            h.iter().map(|t| t.forced).collect::<Vec<_>>(),
            [false, true]
        );
        assert!(s.events_after(0).unwrap()[1].forced);
        assert_eq!(s.get_task("M1").unwrap().status, Merged);
    }

    #[test]
    fn counters() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        assert_eq!(s.bump("M1", Counter::Fix).unwrap(), 1);
        assert_eq!(s.bump("M1", Counter::Fix).unwrap(), 2);
        s.bump("M1", Counter::Review).unwrap();
        s.bump("M1", Counter::Conflict).unwrap();
        s.bump("M1", Counter::Lost).unwrap();
        s.bump("M1", Counter::Attempt).unwrap();
        s.mark_escalated("M1").unwrap();
        let t = s.get_task("M1").unwrap();
        assert_eq!(
            (
                t.fix_rounds,
                t.review_rounds,
                t.conflict_rounds,
                t.lost,
                t.attempt,
                t.escalated
            ),
            (2, 1, 1, 1, 1, true)
        );
        assert!(matches!(
            s.bump("zz", Counter::Fix),
            Err(StoreError::NoSuchTask(_))
        ));
    }

    #[test]
    fn ready_derived_from_dependencies() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        add(&mut s, "M2", &["M1"]);
        assert!(s.is_ready("M1").unwrap());
        assert!(!s.is_ready("M2").unwrap());
        for to in [Coding, Verifying, Merged] {
            mv(&mut s, "M1", to).unwrap();
        }
        assert!(s.is_ready("M2").unwrap());
        assert!(!s.is_ready("M1").unwrap());
    }

    #[test]
    fn history_keeps_everything_in_order() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        for to in [Coding, Verifying, Coding, NeedsFix, Coding] {
            mv(&mut s, "M1", to).unwrap();
        }
        let h = s.history("M1").unwrap();
        assert_eq!(h.len(), 5);
        for w in h.windows(2) {
            assert_eq!(w[0].to, w[1].from);
        }
    }

    #[test]
    fn failed_event_write_rolls_back_the_state() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        s.conn
            .execute_batch("CREATE TRIGGER no_events BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT, 'boom'); END;")
            .unwrap();
        let e = mv(&mut s, "M1", Coding).unwrap_err();
        assert!(e.to_string().contains("boom"), "{e}");
        assert_eq!(s.get_task("M1").unwrap().status, Pending);
        assert!(s.history("M1").unwrap().is_empty());
        assert_eq!(s.get_task("M1").unwrap().reason, "");
    }

    #[test]
    fn two_connections_never_lose_an_update() {
        let (_d, mut a) = store();
        add(&mut a, "M1", &[]);
        mv(&mut a, "M1", Coding).unwrap();
        let path = a.path().to_path_buf();
        let mut b = Store::open(&path).unwrap();
        // A and B race from `coding`: verifying vs needs-fix. Whichever goes second is validated
        // against the first's result; both are recorded in a consistent chain, or the second refused.
        let ta = std::thread::spawn(move || mv(&mut a, "M1", Verifying).is_ok());
        let tb = std::thread::spawn(move || mv(&mut b, "M1", NeedsFix).is_ok());
        let (ok_a, ok_b) = (ta.join().unwrap(), tb.join().unwrap());
        assert!(ok_a || ok_b);
        let s = Store::open(&path).unwrap();
        let h = s.history("M1").unwrap();
        assert_eq!(h.len(), 1 + usize::from(ok_a) + usize::from(ok_b));
        for w in h.windows(2) {
            assert_eq!(w[0].to, w[1].from, "chain broken: {h:?}");
        }
        assert_eq!(s.get_task("M1").unwrap().status, h.last().unwrap().to);
    }

    #[test]
    fn concurrent_counter_bumps_are_not_lost() {
        let (_d, mut a) = store();
        add(&mut a, "M1", &[]);
        let path = a.path().to_path_buf();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let p = path.clone();
                std::thread::spawn(move || {
                    let mut s = Store::open(&p).unwrap();
                    for _ in 0..10 {
                        s.bump("M1", Counter::Fix).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(a.get_task("M1").unwrap().fix_rounds, 40);
    }

    // §16/AC1: a schema-1 database upgrades in place when this build opens it.
    #[test]
    fn schema1_database_upgrades_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0].1).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }
        let mut s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(SCHEMA_VERSION, 2);
        add(&mut s, "M1", &[]);
        let id = s
            .record_delivery("claude-1", "M1", 1, DeliveryKind::Coder, "go")
            .unwrap();
        assert_eq!(s.delivery(id).unwrap().task, "M1");
    }

    // §16: an existing run's rows (task, transition history, event) survive the upgrade unchanged,
    // and the new delivery tables are usable against them.
    #[test]
    fn schema1_database_with_run_data_keeps_its_rows_on_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let dump = |conn: &Connection| -> Vec<String> {
            let mut out = Vec::new();
            for sql in [
                "SELECT id||'|'||title||'|'||status||'|'||reason||'|'||ifnull(worker,'-')||'|'||ifnull(last_worker,'-')||'|'||ifnull(model,'-')||'|'||attempt||'|'||fix_rounds||'|'||review_rounds||'|'||conflict_rounds||'|'||lost||'|'||escalated FROM tasks ORDER BY id",
                "SELECT task_id||'|'||dep_id FROM task_deps ORDER BY task_id, dep_id",
                "SELECT id||'|'||task_id||'|'||ts||'|'||from_state||'|'||to_state||'|'||why||'|'||ifnull(worker,'-')||'|'||ifnull(model,'-')||'|'||attempt||'|'||forced FROM history ORDER BY id",
                "SELECT id||'|'||ts||'|'||task_id||'|'||from_state||'|'||to_state||'|'||why||'|'||ifnull(agent,'-')||'|'||ifnull(model,'-')||'|'||attempt||'|'||forced||'|'||lead_blocking FROM events ORDER BY id",
            ] {
                let mut stmt = conn.prepare(sql).unwrap();
                out.extend(
                    stmt.query_map([], |r| r.get::<_, String>(0))
                        .unwrap()
                        .map(Result::unwrap),
                );
                out.push("--".into());
            }
            out
        };
        let before = {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0].1).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute_batch(
                "INSERT INTO tasks (id, title, status, reason, worker, last_worker, model, attempt, fix_rounds, review_rounds, conflict_rounds, lost, escalated)
                   VALUES ('M1', 'first', 'merged', '', NULL, 'claude-1', 'opus', 2, 1, 1, 0, 1, 1),
                          ('M2', 'second', 'coding', 'working', 'codex-2', 'codex-2', 'sonnet', 1, 0, 0, 0, 0, 0);
                 INSERT INTO task_deps (task_id, dep_id) VALUES ('M2', 'M1');
                 INSERT INTO history (task_id, ts, from_state, to_state, why, worker, model, attempt, forced)
                   VALUES ('M1', 1000, 'pending', 'coding', 'dispatched', 'claude-1', 'opus', 1, 0),
                          ('M1', 1100, 'coding', 'merged', 'accepted', 'claude-1', 'opus', 2, 1),
                          ('M2', 1200, 'pending', 'coding', 'dispatched', 'codex-2', 'sonnet', 1, 0);
                 INSERT INTO events (ts, task_id, from_state, to_state, why, agent, model, attempt, forced, lead_blocking)
                   VALUES (1100, 'M1', 'coding', 'merged', 'accepted', 'claude-1', 'opus', 2, 1, 1);",
            )
            .unwrap();
            let d = dump(&conn);
            assert!(d.len() > 8, "fixture should hold rows: {d:?}");
            d
        };

        let mut s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(dump(&s.conn), before, "upgrade changed existing rows");

        // The same rows read back through the store API.
        let m1 = s.get_task("M1").unwrap();
        assert_eq!((m1.status, m1.attempt, m1.escalated), (Merged, 2, true));
        assert_eq!(s.get_task("M2").unwrap().deps, ["M1"]);
        let h = s.history("M1").unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].from, h[0].to, h[1].to), (Pending, Coding, Merged));
        let ev = s.events_after(0).unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(
            (ev[0].task.as_str(), ev[0].to, ev[0].lead_blocking),
            ("M1", Merged, true)
        );

        // The new delivery tables work against the pre-existing task.
        let id = s
            .record_delivery("codex-2", "M2", 1, DeliveryKind::Coder, "go")
            .unwrap();
        assert_eq!(s.delivery(id).unwrap().task, "M2");
        assert_eq!(s.unconfirmed_deliveries().unwrap().len(), 1);
        s.confirm_delivery(id).unwrap();
        assert!(s.unconfirmed_deliveries().unwrap().is_empty());
        assert_eq!(s.delivery_fails("codex-2").unwrap(), 0);
    }

    #[test]
    fn delivery_roundtrip_keeps_every_field() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        let id = s
            .record_delivery("claude-1", "M1", 3, DeliveryKind::Reviewer, "review this")
            .unwrap();
        let d = s.delivery(id).unwrap();
        assert_eq!(
            (
                d.agent.as_str(),
                d.task.as_str(),
                d.attempt,
                d.kind,
                d.prompt.as_str()
            ),
            ("claude-1", "M1", 3, DeliveryKind::Reviewer, "review this")
        );
        assert!(!d.confirmed && d.confirmed_at.is_none());
        assert!(d.recorded_at > 0);
        assert!(d.error_kind.is_none() && d.error_message.is_none());
        assert_eq!(s.unconfirmed_deliveries().unwrap().len(), 1);
    }

    #[test]
    fn confirming_and_error_records_are_visible() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        add(&mut s, "M2", &[]);
        let ok = s
            .record_delivery("claude-1", "M1", 1, DeliveryKind::Coder, "go")
            .unwrap();
        let bad = s
            .record_delivery("codex-2", "M2", 1, DeliveryKind::Coder, "go")
            .unwrap();
        s.confirm_delivery(ok).unwrap();
        let d = s.delivery(ok).unwrap();
        assert!(d.confirmed && d.confirmed_at.is_some());
        assert!(d.error_kind.is_none());
        assert_eq!(
            s.unconfirmed_deliveries()
                .unwrap()
                .iter()
                .map(|d| d.id)
                .collect::<Vec<_>>(),
            [bad]
        );
        s.record_delivery_error(bad, "timed_out", "herdr call timed out after 30s")
            .unwrap();
        let d = s.delivery(bad).unwrap();
        assert!(!d.confirmed);
        assert_eq!(d.error_kind.as_deref(), Some("timed_out"));
        assert_eq!(
            d.error_message.as_deref(),
            Some("herdr call timed out after 30s")
        );
        assert_eq!(s.unconfirmed_deliveries().unwrap().len(), 1);
    }

    #[test]
    fn confirming_twice_or_missing_delivery_is_an_error() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        let id = s
            .record_delivery("claude-1", "M1", 1, DeliveryKind::Coder, "go")
            .unwrap();
        s.confirm_delivery(id).unwrap();
        assert!(matches!(
            s.confirm_delivery(id),
            Err(StoreError::DeliveryConfirmed(_))
        ));
        assert!(matches!(
            s.delivery(999),
            Err(StoreError::NoSuchDelivery(999))
        ));
        assert!(matches!(
            s.record_delivery_error(999, "other", "x"),
            Err(StoreError::NoSuchDelivery(999))
        ));
    }

    #[test]
    fn a_delivery_needs_an_existing_task() {
        let (_d, mut s) = store();
        assert!(
            s.record_delivery("claude-1", "nope", 1, DeliveryKind::Coder, "go")
                .is_err()
        );
    }

    #[test]
    fn failure_streaks_bump_and_reset_per_agent() {
        let (_d, mut s) = store();
        assert_eq!(s.delivery_fails("claude-1").unwrap(), 0);
        assert_eq!(s.bump_delivery_fails("claude-1").unwrap(), 1);
        assert_eq!(s.bump_delivery_fails("claude-1").unwrap(), 2);
        assert_eq!(s.bump_delivery_fails("codex-2").unwrap(), 1);
        assert_eq!(s.delivery_fails("claude-1").unwrap(), 2);
        s.reset_delivery_fails("claude-1").unwrap();
        assert_eq!(s.delivery_fails("claude-1").unwrap(), 0);
        assert_eq!(s.delivery_fails("codex-2").unwrap(), 1);
    }
}
