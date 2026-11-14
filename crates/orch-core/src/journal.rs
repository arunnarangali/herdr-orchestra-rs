//! The merge journal (§15, §16): one row that says "a merge of Mn is in progress, the branch was
//! at X, the work being merged is Y". It is written before the first change to the feature branch
//! and removed in the same transaction that marks the milestone merged, so a process killed at any
//! step leaves either nothing to repair or a row that [`crate::merge::recover`] can use to finish
//! the merge (its commit landed) or undo it (it did not).

use crate::error::{Error, Result};
use crate::store::{Store, StoreError, apply_transition, now};
use crate::task::{Mode, State, check_transition};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

/// How far a merge got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The row is written; the branch has not been touched.
    Started,
    /// The squash merge is staged in the main folder.
    Squashed,
    /// The check passed on the squashed result.
    Checked,
    /// The commit exists on the branch.
    Committed,
}

impl Step {
    pub fn as_str(self) -> &'static str {
        match self {
            Step::Started => "started",
            Step::Squashed => "squashed",
            Step::Checked => "checked",
            Step::Committed => "committed",
        }
    }

    fn parse(s: &str) -> Step {
        match s {
            "squashed" => Step::Squashed,
            "checked" => Step::Checked,
            "committed" => Step::Committed,
            _ => Step::Started,
        }
    }
}

/// The journal row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journal {
    pub task: String,
    pub step: Step,
    pub head_before: String,
    pub src: String,
    pub commit: Option<String>,
    pub started_at: i64,
}

fn busy(task: &str) -> Error {
    Error::wrap(
        std::io::Error::other(format!("a merge of {task} is already in progress")),
        3,
    )
}

fn store_err(e: rusqlite::Error) -> Error {
    Error::from(StoreError::from(e))
}

impl Store {
    /// The merge in progress, if any.
    pub fn journal(&self) -> Result<Option<Journal>> {
        self.conn
            .query_row(
                "SELECT task_id, step, head_before, src, commit_sha, started_at FROM merge_journal WHERE id=1",
                [],
                |r| {
                    Ok(Journal {
                        task: r.get(0)?,
                        step: Step::parse(&r.get::<_, String>(1)?),
                        head_before: r.get(2)?,
                        src: r.get(3)?,
                        commit: r.get(4)?,
                        started_at: r.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(store_err)
    }

    /// Write the journal row for a merge about to touch the branch. Refuses (exit 3) when another
    /// merge is journaled.
    pub fn journal_begin(&mut self, task: &str, head_before: &str, src: &str) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_err)?;
        let existing: Option<String> = tx
            .query_row("SELECT task_id FROM merge_journal WHERE id=1", [], |r| r.get(0))
            .optional()
            .map_err(store_err)?;
        if let Some(other) = existing {
            return Err(busy(&other));
        }
        tx.execute(
            "INSERT INTO merge_journal (id, task_id, step, head_before, src, started_at) VALUES (1,?1,'started',?2,?3,?4)",
            params![task, head_before, src, now()],
        )
        .map_err(store_err)?;
        tx.commit().map_err(store_err)
    }

    /// Record how far the merge got (and the commit, once it exists).
    pub fn journal_step(&mut self, step: Step, commit: Option<&str>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE merge_journal SET step=?1, commit_sha=COALESCE(?2, commit_sha) WHERE id=1",
                params![step.as_str(), commit],
            )
            .map_err(store_err)?;
        Ok(())
    }

    /// Drop the journal row without touching the milestone (the merge was undone or never
    /// changed anything).
    pub fn journal_clear(&mut self) -> Result<()> {
        self.conn
            .execute("DELETE FROM merge_journal WHERE id=1", [])
            .map_err(store_err)?;
        Ok(())
    }

    /// The landing is complete: in ONE transaction remove the journal row and the milestone's
    /// landing problem, free its coder and mark it `merged`. A crash before this commits leaves
    /// the journal row (so recovery finishes the job); after it, nothing is left to repair.
    pub fn journal_finish(&mut self, task: &str) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_err)?;
        tx.execute("DELETE FROM merge_journal WHERE id=1", [])
            .map_err(store_err)?;
        tx.execute("DELETE FROM merge_issues WHERE task_id=?1", params![task])
            .map_err(store_err)?;
        let status = crate::store::load_task(&tx, task)?
            .ok_or_else(|| StoreError::NoSuchTask(task.to_string()))?
            .status;
        if status != State::Merged && check_transition(task, status, State::Merged, Mode::Auto).is_ok()
        {
            tx.execute(
                "UPDATE tasks SET last_worker=COALESCE(worker, last_worker), worker=NULL WHERE id=?1",
                params![task],
            )
            .map_err(store_err)?;
            apply_transition(&tx, task, &crate::store::TransitionRequest::auto(State::Merged, None))?;
        }
        tx.commit().map_err(store_err)
    }
}

