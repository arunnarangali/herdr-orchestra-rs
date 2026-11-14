//! Atomic task claiming (§10): one milestone, one coder. [`Store::claim`] moves a
//! milestone to `coding` and ties a coder to it in a single `BEGIN IMMEDIATE`
//! transaction that re-reads the task inside the transaction, so two simultaneous
//! claimers (concurrent drivers, or a crashed run followed by a restart) can never both
//! win: the second sees the first's `coding` and fails cleanly. It also refuses a coder
//! that is already working another milestone. [`Store::release_claim`] frees the coder
//! without a state change (the `handoff` of review pipelining: the work stays queued for
//! review while the coder takes the next milestone).

use crate::error::EXIT_USAGE;
use crate::store::{Store, StoreError, TransitionRequest, apply_transition, load_task};
use crate::task::State;
use rusqlite::{OptionalExtension, params};

#[derive(Debug, thiserror::Error)]
pub enum ClaimError {
    #[error("unknown task {0}")]
    NoSuchTask(String),
    /// Python's `prepare`: only pending, paused, needs-fix or conflict milestones are
    /// handed to a coder.
    #[error("{task} is {status}, expected pending/paused/needs-fix/conflict")]
    NotClaimable { task: String, status: State },
    #[error("{worker} is busy with {task}")]
    WorkerBusy { worker: String, task: String },
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for ClaimError {
    fn from(e: rusqlite::Error) -> Self {
        ClaimError::Store(StoreError::Sqlite(e))
    }
}

impl From<ClaimError> for crate::Error {
    fn from(e: ClaimError) -> Self {
        crate::Error::wrap(e, EXIT_USAGE)
    }
}

/// The states a coder can be handed a milestone in (Python's `prepare`).
const CLAIMABLE: [State; 4] = [
    State::Pending,
    State::Paused,
    State::Conflict,
    State::NeedsFix,
];

/// The states where a coder is working a milestone; the same list as
/// [`crate::run_store::is_active`], spelled out for SQL (tested against it below).
const ACTIVE_SQL: &str = "'coding','verifying','awaiting-review','reviewing','reviewed','committed','conflict','needs-fix','held'";

impl Store {
    /// Claim a milestone for a worker, atomically: the task moves to `coding` with
    /// `worker` and `last_worker` set to the coder (`model` recorded when given, the
    /// existing reason kept), and the dispatch intent's `--to` is consumed (it holds for
    /// one dispatch). Exactly one of two simultaneous claimers succeeds; the other gets
    /// [`ClaimError::NotClaimable`]. A worker already on another milestone is refused
    /// ([`ClaimError::WorkerBusy`]).
    pub fn claim(
        &mut self,
        task: &str,
        worker: &str,
        model: Option<&str>,
    ) -> Result<(), ClaimError> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let t = load_task(&tx, task)?.ok_or_else(|| ClaimError::NoSuchTask(task.to_string()))?;
        if !CLAIMABLE.contains(&t.status) {
            return Err(ClaimError::NotClaimable {
                task: task.to_string(),
                status: t.status,
            });
        }
        let busy: Option<String> = tx
            .query_row(
                &format!(
                    "SELECT id FROM tasks WHERE worker=?1 AND id<>?2
                     AND status IN ({ACTIVE_SQL}) LIMIT 1"
                ),
                params![worker, task],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(other) = busy {
            return Err(ClaimError::WorkerBusy {
                worker: worker.to_string(),
                task: other,
            });
        }
        tx.execute(
            "UPDATE tasks SET worker=?2, last_worker=?2, model=COALESCE(?3, model) WHERE id=?1",
            params![task, worker, model],
        )?;
        // The lead's `--to` holds for one dispatch; later retries may move the coder.
        tx.execute(
            "UPDATE dispatch_intent SET assign=NULL WHERE task_id=?1",
            params![task],
        )?;
        apply_transition(&tx, task, &TransitionRequest::auto(State::Coding, None))?;
        tx.commit()?;
        Ok(())
    }

    /// Free a milestone's coder without a state change (review pipelining's `handoff`):
    /// the coder becomes free for the next milestone while the work stays with the
    /// milestone (`last_worker` keeps the builder, so fix jobs prefer them). Idempotent.
    pub fn release_claim(&mut self, task: &str) -> Result<(), ClaimError> {
        let n = self.conn.execute(
            "UPDATE tasks SET last_worker=COALESCE(worker, last_worker), worker=NULL WHERE id=?1",
            params![task],
        )?;
        if n == 0 {
            return Err(ClaimError::NoSuchTask(task.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_store::is_active;
    use crate::store::NewTask;

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

    fn status_of(s: &Store, id: &str) -> State {
        s.get_task(id).unwrap().status
    }

    #[test]
    fn claiming_moves_the_task_to_coding_and_ties_the_coder() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        s.claim("M1", "claude-1", Some("sonnet")).unwrap();
        let t = s.get_task("M1").unwrap();
        assert_eq!(
            (t.status, t.worker.as_deref(), t.last_worker.as_deref()),
            (State::Coding, Some("claude-1"), Some("claude-1"))
        );
        assert_eq!(t.model.as_deref(), Some("sonnet"));
        let h = s.history("M1").unwrap();
        assert_eq!(
            (h.len(), h[0].from, h[0].to, h[0].worker.as_deref()),
            (1, State::Pending, State::Coding, Some("claude-1"))
        );
    }

    #[test]
    fn claiming_keeps_the_reason_and_can_change_the_model() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        s.transition(
            "M1",
            &TransitionRequest::manual(State::Paused, Some("held"), true),
        )
        .unwrap();
        s.claim("M1", "codex-2", None).unwrap();
        let t = s.get_task("M1").unwrap();
        assert_eq!(
            (t.status, t.reason.as_str(), t.model.as_deref()),
            (State::Coding, "held", None)
        );
        s.release_claim("M1").unwrap();
        s.transition("M1", &TransitionRequest::manual(State::Paused, None, true))
            .unwrap();
        s.claim("M1", "codex-2", Some("gpt-9")).unwrap();
        assert_eq!(s.get_task("M1").unwrap().model.as_deref(), Some("gpt-9"));
    }

    #[test]
    fn claiming_works_from_paused_conflict_and_needs_fix() {
        for from in [State::Paused, State::Conflict, State::NeedsFix] {
            let (_d, mut s) = store();
            add(&mut s, "M1", &[]);
            s.transition("M1", &TransitionRequest::auto(State::Coding, None))
                .unwrap();
            s.transition("M1", &TransitionRequest::auto(from, Some("back")))
                .unwrap();
            s.claim("M1", "w1", None).unwrap();
            assert_eq!(status_of(&s, "M1"), State::Coding, "{from}");
        }
    }

    #[test]
    fn claiming_refuses_a_task_in_any_other_state() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        s.transition("M1", &TransitionRequest::auto(State::Coding, None))
            .unwrap();
        let e = s.claim("M1", "w2", None).unwrap_err();
        assert_eq!(
            e.to_string(),
            "M1 is coding, expected pending/paused/needs-fix/conflict"
        );
        assert_eq!(
            s.get_task("M1").unwrap().worker.as_deref(),
            None,
            "nothing changed"
        );
        assert_eq!(status_of(&s, "M1"), State::Coding);
        assert_eq!(s.history("M1").unwrap().len(), 1);
    }

    #[test]
    fn claiming_refuses_a_worker_on_another_task() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        add(&mut s, "M2", &[]);
        s.claim("M1", "w1", None).unwrap();
        let e = s.claim("M2", "w1", None).unwrap_err();
        assert_eq!(e.to_string(), "w1 is busy with M1");
        assert_eq!(s.get_task("M2").unwrap().status, State::Pending);
        assert_eq!(s.history("M2").unwrap().len(), 0);
    }

    #[test]
    fn the_busy_check_counts_every_active_state() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        for to in [
            State::Coding,
            State::Verifying,
            State::Reviewed,
            State::Committed,
        ] {
            // forced: the point is the states, not the steps that make them true
            s.transition("M1", &TransitionRequest::manual(to, Some("go"), true))
                .unwrap();
        }
        s.release_claim("M1").unwrap();
        add(&mut s, "M2", &[]);
        // A worker on a committed task is busy: it may still be merging.
        s.claim("M1", "w1", None).unwrap_err(); // committed is not claimable, but sets nothing
        s.conn
            .execute("UPDATE tasks SET worker='w1' WHERE id='M1'", [])
            .unwrap();
        let e = s.claim("M2", "w1", None).unwrap_err();
        assert!(e.to_string().contains("is busy with M1"), "{e}");
    }

    #[test]
    fn claiming_an_unknown_task_fails_cleanly() {
        let (_d, mut s) = store();
        assert_eq!(
            s.claim("M9", "w1", None).unwrap_err().to_string(),
            "unknown task M9"
        );
        assert_eq!(
            s.release_claim("M9").unwrap_err().to_string(),
            "unknown task M9"
        );
    }

    /// Exactly one of two simultaneous claimers wins (§10 / AC6): two connections on one
    /// database file, two threads, a barrier so they race.
    #[test]
    fn exactly_one_of_two_simultaneous_claimers_wins() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        let path = s.path().to_path_buf();
        drop(s);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = ["w1", "w2"]
            .iter()
            .map(|w| {
                let p = path.clone();
                let b = barrier.clone();
                let w = *w;
                std::thread::spawn(move || {
                    let mut s = Store::open(&p).unwrap();
                    b.wait();
                    s.claim("M1", w, None)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            1,
            "{results:?}"
        );
        let s = Store::open(&path).unwrap();
        let t = s.get_task("M1").unwrap();
        assert_eq!(t.status, State::Coding);
        assert!(t.worker.is_some());
        assert_eq!(
            s.history("M1").unwrap().len(),
            1,
            "exactly one transition was recorded"
        );
        let loser = results
            .iter()
            .find(|r| r.is_err())
            .unwrap()
            .as_ref()
            .unwrap_err();
        assert!(
            loser
                .to_string()
                .contains("expected pending/paused/needs-fix/conflict"),
            "{loser}"
        );
    }

    /// Two simultaneous claims on two different tasks by the same worker: still one
    /// worker, one task.
    #[test]
    fn simultaneous_claims_of_two_tasks_by_one_worker_leave_one_unclaimed() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        add(&mut s, "M2", &[]);
        let path = s.path().to_path_buf();
        drop(s);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = ["M1", "M2"]
            .iter()
            .map(|t| {
                let p = path.clone();
                let b = barrier.clone();
                let t = *t;
                std::thread::spawn(move || {
                    let mut s = Store::open(&p).unwrap();
                    b.wait();
                    s.claim(t, "w1", None)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            1,
            "{results:?}"
        );
        let s = Store::open(&path).unwrap();
        let busy: Vec<&str> = ["M1", "M2"]
            .iter()
            .filter(|t| s.get_task(t).unwrap().worker.is_some())
            .copied()
            .collect();
        assert_eq!(busy.len(), 1);
        assert!(matches!(
            results.iter().find(|r| r.is_err()).unwrap(),
            Err(ClaimError::WorkerBusy { .. })
        ));
    }

    #[test]
    fn releasing_frees_the_coder_but_keeps_the_builder() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        s.claim("M1", "w1", Some("sonnet")).unwrap();
        s.release_claim("M1").unwrap();
        let t = s.get_task("M1").unwrap();
        assert_eq!(
            (t.status, t.worker.as_deref(), t.last_worker.as_deref()),
            (State::Coding, None, Some("w1"))
        );
        // The freed coder can take the next milestone...
        add(&mut s, "M2", &[]);
        s.claim("M2", "w1", None).unwrap();
        assert_eq!(s.get_task("M2").unwrap().worker.as_deref(), Some("w1"));
        // ...and a release without a claim is fine.
        s.release_claim("M2").unwrap();
        assert_eq!(s.get_task("M2").unwrap().last_worker.as_deref(), Some("w1"));
    }

    /// Review pipelining (§10 / AC8): after the checks pass the work is saved, the coder
    /// is freed and the review is queued; the freed coder is immediately eligible for the
    /// next ready milestone, and review blockers come back preferring the builder.
    #[test]
    fn a_freed_coder_is_eligible_again_while_the_review_is_pending() {
        let (_d, mut s) = store();
        add(&mut s, "M1", &[]);
        s.claim("M1", "w1", None).unwrap();
        s.release_claim("M1").unwrap(); // handoff: work saved, coder free
        s.transition(
            "M1",
            &TransitionRequest::manual(State::AwaitingReview, Some("review"), true),
        )
        .unwrap();
        // The freed coder takes the next ready milestone while the review is pending.
        add(&mut s, "M2", &[]);
        s.claim("M2", "w1", None).unwrap();
        assert_eq!(s.get_task("M2").unwrap().worker.as_deref(), Some("w1"));
        // Review blockers come back as a fix job that prefers the builder.
        s.transition(
            "M1",
            &TransitionRequest::manual(State::Reviewed, None, true),
        )
        .unwrap();
        s.transition(
            "M1",
            &TransitionRequest::manual(State::NeedsFix, Some("blockers"), true),
        )
        .unwrap();
        assert_eq!(s.get_task("M1").unwrap().last_worker.as_deref(), Some("w1"));
    }

    #[test]
    fn the_claimable_states_match_the_active_ones_for_busy_checks() {
        let claimable: Vec<&str> = CLAIMABLE.iter().map(|s| s.as_str()).collect();
        assert_eq!(claimable, ["pending", "paused", "conflict", "needs-fix"]);
        let active: Vec<&str> = State::ALL
            .iter()
            .filter(|s| is_active(**s))
            .map(|s| s.as_str())
            .collect();
        let sql_states: Vec<&str> = ACTIVE_SQL
            .split(',')
            .map(|s| s.trim_matches('\''))
            .collect();
        let mut sql_sorted = sql_states.clone();
        sql_sorted.sort();
        let mut active_sorted = active.clone();
        active_sorted.sort();
        assert_eq!(
            sql_sorted, active_sorted,
            "ACTIVE_SQL must match run_store::is_active"
        );
    }
}
