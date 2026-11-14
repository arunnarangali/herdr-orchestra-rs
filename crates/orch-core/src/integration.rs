//! A green milestone must not break the run branch (§13, §15): the prompt that sends a milestone
//! back when it fails the check once merged, and the run-level "the branch is broken" mark.
//!
//! The mark is set when the final check fails after every milestone merged. It is stored in the
//! database (`integration_broken`, one row) so a restart sees it: autopilot re-checks the branch
//! first and only carries on once it is green again.

use crate::error::{Error, Result};
use crate::store::{Store, StoreError, now};
use rusqlite::{OptionalExtension, params};

/// The reason stored when the final check fails.
pub const FINAL_CHECK_REASON: &str = "the final check";

/// Python's `_integration_failed` job text: which milestone, which branch, the failing tail. It
/// says what is wrong and never how to fix it.
pub fn job_body(task: &str, branch: &str, check: &str, round: u32, tail: &str, result_file: &str) -> String {
    let dir = format!(".orch-task/{task}");
    let run = if check.trim().is_empty() { String::new() } else { format!(", then run `{check}`") };
    format!(
        "# {task} fails the check once merged onto `{branch}` (round {round})\n\n\
         Your milestone passes `{check}` on its own, but fails once it is merged with what landed on \
         `{branch}` since you started. In this folder run `git merge --no-commit {branch}`, find the \
         cause and fix it (you decide how, and keep both sides working){run}. Don't commit, and \
         don't weaken or skip tests.\n\n```\n{tail}\n```\n\n\
         When you are done, write {dir}/{result_file} (the `result_format` shape from {dir}/task.json) \
         and your short report, then create the done file as the last thing you do.\n"
    )
}

/// The prompt file of integration-fix round `round`.
pub fn job_file(round: u32) -> String {
    format!("integration-fix-{round}.md")
}

/// The line `orch status` prints while the branch is broken (Python's, verbatim).
pub fn status_line(branch: &str, reason: &str) -> String {
    format!(
        "INTEGRATION BROKEN: the check failed on {branch} after merging {reason} — fix it; autopilot \
         re-checks before it continues"
    )
}

/// The broken mark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broken {
    pub reason: String,
    pub since: i64,
}

fn store_err(e: rusqlite::Error) -> Error {
    Error::from(StoreError::from(e))
}

impl Store {
    /// The broken mark, if the run branch is known to fail its check.
    pub fn integration_broken(&self) -> Result<Option<Broken>> {
        self.conn
            .query_row("SELECT reason, since FROM integration_broken WHERE id=1", [], |r| {
                Ok(Broken { reason: r.get(0)?, since: r.get(1)? })
            })
            .optional()
            .map_err(store_err)
    }

    /// Mark the run branch broken (a second mark keeps the first time but takes the new reason).
    pub fn mark_integration_broken(&mut self, reason: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO integration_broken (id, reason, since) VALUES (1, ?1, ?2) \
                 ON CONFLICT(id) DO UPDATE SET reason=excluded.reason",
                params![reason, now()],
            )
            .map_err(store_err)?;
        Ok(())
    }

    /// The branch is green again: drop the mark.
    pub fn clear_integration_broken(&mut self) -> Result<()> {
        self.conn.execute("DELETE FROM integration_broken WHERE id=1", []).map_err(store_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_job_names_the_milestone_branch_check_and_failing_tail() {
        let b = job_body("M2", "feature/demo", "cargo test", 2, "FAILED a::b", "result-3.json");
        assert!(b.starts_with("# M2 fails the check once merged onto `feature/demo` (round 2)"));
        assert!(b.contains("git merge --no-commit feature/demo"));
        assert!(b.contains("`cargo test`"));
        assert!(b.contains("```\nFAILED a::b\n```"));
        assert!(b.contains(".orch-task/M2/result-3.json"));
        assert!(!b.contains("Co-Authored"));
    }

    #[test]
    fn the_mark_is_set_read_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open_in(dir.path()).unwrap();
        assert_eq!(s.integration_broken().unwrap(), None);
        s.mark_integration_broken(FINAL_CHECK_REASON).unwrap();
        let first = s.integration_broken().unwrap().unwrap();
        assert_eq!(first.reason, "the final check");
        s.mark_integration_broken("M3").unwrap();
        let again = s.integration_broken().unwrap().unwrap();
        assert_eq!((again.reason.as_str(), again.since), ("M3", first.since));
        s.clear_integration_broken().unwrap();
        assert_eq!(s.integration_broken().unwrap(), None);
    }
}
