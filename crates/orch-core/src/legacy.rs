//! Task branches left by orch versions before 4.5 (`<branch>-Mn` — plans then read
//! `Branch: orch/<slug>`, so the branches were `orch/<slug>-Mn`): their work is carried into
//! the matching hidden refs so nothing is lost, and the old branches are deleted.
//!
//! A branch is deleted only when its tip is already safe in the hidden ref: either the ref was
//! just written from it, or the work was carried over earlier. A branch whose tip is not in
//! the ref (different work, or a ref whose object is gone) is kept and the output says so.

use crate::error::Result;
use crate::git;
use crate::refs::{self, RefState, task_ref};
use std::path::Path;

/// The branch name an orch before 4.5 used for `tid` on this run's branch.
pub fn branch_name(branch: &str, tid: &str) -> String {
    format!("{branch}-{tid}")
}

/// One line printed for a legacy branch that was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pickup {
    /// Its work was written into the hidden ref and the branch is gone.
    Picked(String),
    /// Its work was already in the hidden ref; the branch is gone.
    Already(String),
    /// The branch was left alone (the message says why); nothing was lost.
    Kept(String),
}

impl Pickup {
    /// The line as the CLI prints it.
    pub fn line(&self) -> String {
        match self {
            Pickup::Picked(m) | Pickup::Already(m) => format!("PICKED UP legacy branch {m}"),
            Pickup::Kept(m) => format!("KEPT legacy branch {m}"),
        }
    }
}

/// Carry every legacy task branch of `branch`'s run (one per milestone id in `order`) into its
/// hidden ref and delete it when safe. Idempotent: a branch that is gone changes nothing.
pub fn pickup(root: &Path, branch: &str, order: &[String]) -> Result<Vec<Pickup>> {
    let mut out = Vec::new();
    for tid in order {
        let short = branch_name(branch, tid);
        if !git::branch_exists(root, &short)? {
            continue;
        }
        let tip = git::resolve_commit(root, &format!("refs/heads/{short}"))?.ok_or_else(|| {
            crate::Error::wrap(
                std::io::Error::other(format!("{short} exists but names no commit")),
                2,
            )
        })?;
        let tref = task_ref(branch, tid);
        match refs::read_state(root, &tref)? {
            RefState::Missing => {
                refs::write(root, &tref, &tip)?;
                delete(root, &short)?;
                out.push(Pickup::Picked(format!(
                    "{short}: its work is in {tref} now, and the branch is gone"
                )));
            }
            RefState::Valid(sha) if sha == tip || is_ancestor(root, &tip, &sha)? => {
                delete(root, &short)?;
                out.push(Pickup::Already(format!(
                    "{short}: its work was already in {tref}, and the branch is gone"
                )));
            }
            RefState::Valid(sha) => out.push(Pickup::Kept(format!(
                "{short}: {tref} holds other saved work ({sha}) — nothing lost, the branch stays"
            ))),
            RefState::Broken(sha) => out.push(Pickup::Kept(format!(
                "{short}: {tref} points at {sha}, which is gone — nothing lost, the branch stays"
            ))),
        }
    }
    Ok(out)
}

fn is_ancestor(root: &Path, tip: &str, sha: &str) -> Result<bool> {
    Ok(git::run(
        root,
        &["merge-base", "--is-ancestor", tip, sha],
    )?
    .success())
}

fn delete(root: &Path, short: &str) -> Result<()> {
    git::check(root, &["branch", "-D", "-q", short])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::*;

    fn commit_file(dir: &Path, name: &str, body: &str) -> String {
        std::fs::write(dir.join(name), body).unwrap();
        g(dir, &["add", "-A"]);
        g(dir, &["commit", "-q", "-m", name]);
        g(dir, &["rev-parse", "HEAD"])
    }

    #[test]
    fn names_match_the_old_naming() {
        assert_eq!(branch_name("orch/demo", "M1"), "orch/demo-M1");
        assert_eq!(branch_name("feature/x", "M12"), "feature/x-M12");
    }

    #[test]
    fn a_legacy_branch_is_carried_into_the_hidden_ref_and_deleted() {
        let (_t, root) = repo();
        g(&root, &["switch", "-q", "-c", "orch/demo-M1"]);
        let sha = commit_file(&root, "work.txt", "w\n");
        g(&root, &["switch", "-q", "main"]);
        let out = pickup(&root, "orch/demo", &["M1".to_string()]).unwrap();
        assert_eq!(
            out,
            vec![Pickup::Picked(
                "orch/demo-M1: its work is in refs/orch/orch-demo/M1 now, and the branch is gone"
                    .to_string()
            )]
        );
        assert_eq!(refs::read(&root, "refs/orch/orch-demo/M1").unwrap(), Some(sha));
        assert!(!git::branch_exists(&root, "orch/demo-M1").unwrap());
        // a second pickup finds nothing and changes nothing
        assert!(pickup(&root, "orch/demo", &["M1".to_string()]).unwrap().is_empty());
    }

    #[test]
    fn work_already_in_the_ref_lets_the_branch_go() {
        let (_t, root) = repo();
        g(&root, &["switch", "-q", "-c", "orch/demo-M1"]);
        let sha = commit_file(&root, "work.txt", "w\n");
        g(&root, &["switch", "-q", "main"]);
        refs::write(&root, "refs/orch/orch-demo/M1", &sha).unwrap();
        let out = pickup(&root, "orch/demo", &["M1".to_string()]).unwrap();
        assert!(matches!(out.as_slice(), [Pickup::Already(_)]), "{out:?}");
        assert!(!git::branch_exists(&root, "orch/demo-M1").unwrap());
        // carried over earlier: the branch tip is part of the ref's history
        g(&root, &["switch", "-q", "-c", "orch/demo-M2"]);
        commit_file(&root, "a.txt", "a\n");
        let new = commit_file(&root, "b.txt", "b\n");
        g(&root, &["switch", "-q", "main"]);
        refs::write(&root, "refs/orch/orch-demo/M2", &new).unwrap();
        let out = pickup(&root, "orch/demo", &["M2".to_string()]).unwrap();
        assert!(matches!(out.as_slice(), [Pickup::Already(_)]), "{out:?}");
        assert!(!git::branch_exists(&root, "orch/demo-M2").unwrap());
        assert_eq!(refs::read(&root, "refs/orch/orch-demo/M2").unwrap(), Some(new));
    }

    #[test]
    fn different_or_lost_ref_work_keeps_the_branch() {
        let (_t, root) = repo();
        // the branch holds work the ref does not contain
        g(&root, &["switch", "-q", "-c", "orch/demo-M1"]);
        commit_file(&root, "branch-only.txt", "b\n");
        g(&root, &["switch", "-q", "main"]);
        // the ref holds divergent work the branch does not contain
        g(&root, &["switch", "-q", "-c", "divergent"]);
        let sha = commit_file(&root, "ref-only.txt", "r\n");
        g(&root, &["switch", "-q", "main"]);
        refs::write(&root, "refs/orch/orch-demo/M1", &sha).unwrap();
        let out = pickup(&root, "orch/demo", &["M1".to_string()]).unwrap();
        assert!(
            matches!(out.as_slice(), [Pickup::Kept(m)] if m.contains("holds other saved work") && m.contains(&sha)),
            "{out:?}"
        );
        assert!(git::branch_exists(&root, "orch/demo-M1").unwrap());
        // a ref whose object is gone keeps the branch too
        let path = root.join(".git").join("refs/orch/orch-demo/M1");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "1111111111111111111111111111111111111111\n").unwrap();
        let out = pickup(&root, "orch/demo", &["M1".to_string()]).unwrap();
        assert!(
            matches!(out.as_slice(), [Pickup::Kept(m)] if m.contains("which is gone")),
            "{out:?}"
        );
        assert!(git::branch_exists(&root, "orch/demo-M1").unwrap());
    }

    #[test]
    fn milestones_outside_the_order_are_untouched() {
        let (_t, root) = repo();
        g(&root, &["switch", "-q", "-c", "orch/demo-M9"]);
        commit_file(&root, "x.txt", "x\n");
        g(&root, &["switch", "-q", "main"]);
        assert!(pickup(&root, "orch/demo", &["M1".to_string()]).unwrap().is_empty());
        assert!(git::branch_exists(&root, "orch/demo-M9").unwrap());
    }
}
