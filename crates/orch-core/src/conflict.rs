//! A milestone that could not land (§13, §14): what is stored for its coder's next job, and the
//! patches a re-review needs.
//!
//! * A **merge conflict** keeps the conflicting files. For a milestone the reviewer had already
//!   approved, it also keeps `resolution_base` — the saved work as it was before the resolution —
//!   so the reviewer can be shown just what the resolution changed (`resolution.patch`, next to
//!   the whole milestone in `diff.patch`).
//! * A **check that fails on the feature branch** keeps the tail of the check output.
//!
//! Landing the milestone clears the row. Everything here is data and text; the state changes
//! (conflict round, needs-fix) belong to the caller.

use crate::error::{Error, Result};
use crate::git;
use crate::refs::{self, task_ref};
use crate::store::{Store, StoreError, now};
use rusqlite::{OptionalExtension, params};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    Conflict,
    Check,
}

impl IssueKind {
    fn as_str(self) -> &'static str {
        match self {
            IssueKind::Conflict => "conflict",
            IssueKind::Check => "check",
        }
    }
}

/// Why a milestone did not land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub task: String,
    pub kind: IssueKind,
    pub files: Vec<String>,
    pub detail: String,
    pub resolution_base: Option<String>,
    pub rereview: bool,
}

fn store_err(e: rusqlite::Error) -> Error {
    Error::from(StoreError::from(e))
}

impl Store {
    /// Keep (or replace) the milestone's landing problem.
    pub fn record_issue(&mut self, issue: &Issue) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO merge_issues
                   (task_id, kind, files, detail, resolution_base, rereview, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    issue.task,
                    issue.kind.as_str(),
                    serde_json::to_string(&issue.files).unwrap_or_else(|_| "[]".into()),
                    issue.detail,
                    issue.resolution_base,
                    issue.rereview,
                    now()
                ],
            )
            .map_err(store_err)?;
        Ok(())
    }

    /// The milestone's landing problem, if one is kept.
    pub fn issue(&self, task: &str) -> Result<Option<Issue>> {
        self.conn
            .query_row(
                "SELECT kind, files, detail, resolution_base, rereview FROM merge_issues WHERE task_id=?1",
                params![task],
                |r| {
                    let kind: String = r.get(0)?;
                    let files: String = r.get(1)?;
                    Ok(Issue {
                        task: task.to_string(),
                        kind: if kind == "check" { IssueKind::Check } else { IssueKind::Conflict },
                        files: serde_json::from_str(&files).unwrap_or_default(),
                        detail: r.get(2)?,
                        resolution_base: r.get(3)?,
                        rereview: r.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(store_err)
    }

    /// Forget the milestone's landing problem (it landed, or was approved again).
    pub fn clear_issue(&mut self, task: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM merge_issues WHERE task_id=?1", params![task])
            .map_err(store_err)?;
        Ok(())
    }

    /// The reviewer approved the resolution: stop asking for a re-review (Python's `review-ok`
    /// dropping `pre_conflict`).
    pub fn clear_rereview(&mut self, task: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE merge_issues SET rereview=0, resolution_base=NULL WHERE task_id=?1",
                params![task],
            )
            .map_err(store_err)?;
        Ok(())
    }
}

/// The text of the conflict job the coder gets (`conflict-<n>.md`), as Python writes it.
pub fn job_body(task: &str, title: &str, branch: &str, check: &str, files: &[String]) -> String {
    let files = if files.is_empty() {
        "see git status".to_string()
    } else {
        files.join(", ")
    };
    format!(
        "# Merge conflict for {task}: {title}\n\n\
         Other milestones landed on `{branch}` while you worked, and they conflict with yours in: {files}.\n\
         In this folder run `git merge --no-commit {branch}`, resolve every conflict so BOTH sides keep \
         working (you own the resolution), then run `{check}`. Do not commit.\n"
    )
}

/// git's stdout exactly (a patch must keep its trailing newline), or the failure.
fn raw(dir: &Path, args: &[&str]) -> Result<String> {
    let out = git::run(dir, args)?;
    if out.success() {
        Ok(out.stdout)
    } else {
        Err(git::failed(dir, args, &out))
    }
}

/// What [`write_review_patches`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewPatches {
    /// `diff.patch` was written: the whole milestone against its merge-base with the branch.
    pub diff: bool,
    /// `resolution.patch` was written: what the conflict resolution changed. The review prompt
    /// then says this round reviews a merge-conflict resolution.
    pub resolution: bool,
}

/// Write the patches a review of a milestone needs into `<root>/.orchestrate/review/<Mn>/`:
/// `diff.patch` always (when the milestone has saved work), and `resolution.patch` when an
/// approved milestone was re-opened by a conflict.
pub fn write_review_patches(
    store: &Store,
    root: &Path,
    branch: &str,
    task: &str,
) -> Result<ReviewPatches> {
    let none = ReviewPatches { diff: false, resolution: false };
    let Some(src) = refs::read(root, &task_ref(branch, task))? else {
        return Ok(none);
    };
    let dir = root.join(crate::paths::ORCH_DIR).join("review").join(task);
    std::fs::create_dir_all(&dir)
        .map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
    let mb = git::check(root, &["merge-base", branch, &src])?;
    let diff = raw(root, &["diff", &mb, &src])?;
    let write = |name: &str, body: &str| {
        std::fs::write(dir.join(name), body)
            .map_err(|e| Error::io(format!("writing {}", dir.join(name).display()), e))
    };
    write("diff.patch", &diff)?;
    let mut resolution = false;
    if let Some(issue) = store.issue(task)?
        && issue.rereview
        && let Some(base) = issue.resolution_base
    {
        write("resolution.patch", &raw(root, &["diff", &base, &src])?)?;
        resolution = true;
    }
    Ok(ReviewPatches { diff: true, resolution })
}
