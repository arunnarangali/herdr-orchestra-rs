//! Hidden refs (§13): a milestone's work in progress lives in `refs/orch/<branch-slug>/Mn`,
//! never in a branch. `git branch` does not list it and pushing the feature branch does not
//! carry it. This module names, reads, writes, deletes and lists those refs and saves a folder's
//! uncommitted work into one.
//!
//! Saving never discards anything: the work is committed (worktree mode) or stashed (shared
//! mode), the ref is moved to it and re-read, and only then is the tree left clean. When any
//! step fails the work stays where it was and the error says so.

use crate::error::{Error, Result};
use crate::git::{self, GitOutput};
use std::path::Path;

/// The ref namespace for orch's hidden refs.
pub const REF_NS: &str = "refs/orch";

/// The branch name as one ref path component (`feature/x` -> `feature-x`).
pub fn branch_slug(branch: &str) -> String {
    branch.replace('/', "-")
}

/// The hidden ref for a milestone: `refs/orch/<branch-slug>/Mn`.
pub fn task_ref(branch: &str, task: &str) -> String {
    format!("{REF_NS}/{}/{task}", branch_slug(branch))
}

/// What a hidden ref currently is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefState {
    /// There is no such ref.
    Missing,
    /// The ref points at a commit.
    Valid(String),
    /// The ref exists but its object is gone (or is not a commit): the saved work is lost.
    Broken(String),
}

/// Look a ref up and tell "no ref" from "ref whose object is missing".
pub fn read_state(root: &Path, name: &str) -> Result<RefState> {
    let raw = git::run(root, &["rev-parse", "--verify", "--quiet", name])?;
    if !raw.success() || raw.out().is_empty() {
        return Ok(RefState::Missing);
    }
    Ok(match git::resolve_commit(root, name)? {
        Some(sha) => RefState::Valid(sha),
        None => RefState::Broken(raw.out().to_string()),
    })
}

/// The commit a ref points at; `None` when the ref is missing or its object is gone.
pub fn read(root: &Path, name: &str) -> Result<Option<String>> {
    Ok(match read_state(root, name)? {
        RefState::Valid(sha) => Some(sha),
        RefState::Missing | RefState::Broken(_) => None,
    })
}

/// Point `name` at commit `sha`. The commit must exist, and the ref is read back.
pub fn write(root: &Path, name: &str, sha: &str) -> Result<()> {
    let sha = git::resolve_commit(root, sha)?
        .ok_or_else(|| Error::wrap(std::io::Error::other(format!("{sha} is not a commit")), 2))?;
    git::check(root, &["update-ref", name, &sha])?;
    match read(root, name)? {
        Some(now) if now == sha => Ok(()),
        _ => Err(Error::wrap(
            std::io::Error::other(format!("{name} does not point at {sha} after writing it")),
            2,
        )),
    }
}

/// Delete a ref. Deleting one that is already gone (or broken) is fine.
pub fn delete(root: &Path, name: &str) -> Result<()> {
    if read_state(root, name)? == RefState::Missing {
        return Ok(());
    }
    git::check(root, &["update-ref", "-d", name])?;
    Ok(())
}

/// One hidden ref found by [`list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenRef {
    pub task: String,
    pub name: String,
    pub sha: String,
}

/// Every hidden ref of a run branch, sorted by name (broken ones included).
pub fn list(root: &Path, branch: &str) -> Result<Vec<HiddenRef>> {
    let prefix = format!("{REF_NS}/{}/", branch_slug(branch));
    let out = git::check(
        root,
        &["for-each-ref", "--format=%(refname) %(objectname)", &prefix],
    )?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let (name, sha) = l.split_once(' ')?;
            Some(HiddenRef {
                task: name.strip_prefix(&prefix)?.to_string(),
                name: name.to_string(),
                sha: sha.to_string(),
            })
        })
        .collect())
}

/// Where the work to save sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveMode {
    /// A worker's detached worktree: committed on its HEAD, the ref follows HEAD.
    Worktree,
    /// The main folder (shared mode): stashed, the ref keeps the stash commit.
    Shared,
}

/// What [`save_tree`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Saved {
    /// Nothing to save: the tree is clean and has no commits beyond the feature branch.
    Nothing,
    /// The work is in the ref, at this commit; the tree is clean.
    Saved(String),
}

/// `-c user.name/-c user.email` placeholders when git has no identity configured, so an
/// internal save never fails for want of one.
fn identity(dir: &Path) -> Result<Vec<&'static str>> {
    let have = git::run(dir, &["config", "user.email"])?.success()
        && git::run(dir, &["config", "user.name"])?.success();
    Ok(if have {
        vec![]
    } else {
        vec!["-c", "user.name=orch", "-c", "user.email=orch@localhost"]
    })
}

fn save_failed(what: &str, dir: &Path, out: &GitOutput) -> Error {
    Error::wrap(
        std::io::Error::other(format!(
            "could not save the work in {}: {what} failed; the work is still there, untouched:\n{}",
            dir.display(),
            out.text()
        )),
        2,
    )
}

/// Save the uncommitted work of folder `dir` into hidden ref `ref_name` and leave `dir` clean.
/// `root` is the main repository (where refs are written); `branch` is the feature branch; `msg`
/// names the internal commit. Never discards anything: on an error the work is still in `dir`.
pub fn save_tree(
    dir: &Path,
    root: &Path,
    branch: &str,
    ref_name: &str,
    msg: &str,
    mode: SaveMode,
) -> Result<Saved> {
    match mode {
        SaveMode::Worktree => save_worktree(dir, root, branch, ref_name, msg),
        SaveMode::Shared => save_shared(dir, root, ref_name, msg),
    }
}

fn save_worktree(
    dir: &Path,
    root: &Path,
    branch: &str,
    ref_name: &str,
    msg: &str,
) -> Result<Saved> {
    if git::is_dirty(dir)? {
        let add = git::run(dir, &["add", "-A"])?;
        if !add.success() {
            return Err(save_failed("git add -A", dir, &add));
        }
        let mut args = identity(dir)?;
        args.extend(["commit", "-q", "--no-verify", "-m", msg]);
        let commit = git::run(dir, &args)?;
        if !commit.success() {
            return Err(save_failed("git commit", dir, &commit));
        }
    }
    if git::ahead_of(dir, branch)? == 0 {
        return Ok(Saved::Nothing);
    }
    let head = git::check(dir, &["rev-parse", "HEAD"])?;
    if let Some(old) = read(root, ref_name)?
        && !git::run(dir, &["merge-base", "--is-ancestor", &old, &head])?.success()
    {
        return Err(Error::wrap(
            std::io::Error::other(format!(
                "{ref_name} holds other saved work ({old}) that is not part of the history of {}; not overwriting it. The work in the folder is committed at {head}, not lost",
                dir.display()
            )),
            2,
        ));
    }
    write(root, ref_name, &head)
        .map_err(|e| Error::wrap(std::io::Error::other(format!(
            "could not move {ref_name} to the saved work {head} (the work is committed in {}, not lost): {e}",
            dir.display()
        )), 2))?;
    Ok(Saved::Saved(head))
}

fn stash_top(dir: &Path) -> Result<Option<String>> {
    git::resolve_commit(dir, "refs/stash")
}

fn save_shared(dir: &Path, root: &Path, ref_name: &str, msg: &str) -> Result<Saved> {
    if !git::is_dirty(dir)? {
        return Ok(Saved::Nothing);
    }
    let before = stash_top(dir)?;
    let mut args = identity(dir)?;
    args.extend(["stash", "push", "--include-untracked", "-q", "-m", msg]);
    let push = git::run(dir, &args)?;
    if !push.success() {
        return Err(save_failed("git stash push", dir, &push));
    }
    let sha = match stash_top(dir)? {
        Some(s) if Some(&s) != before.as_ref() => s,
        _ => {
            return Err(save_failed(
                "git stash push (it stashed nothing)",
                dir,
                &push,
            ));
        }
    };
    if let Err(e) = write(root, ref_name, &sha) {
        return Err(Error::wrap(
            std::io::Error::other(format!(
                "could not move {ref_name} to the parked work: {e}\nthe work is NOT lost: it is the top stash entry, {sha} (`git stash apply {sha}`)"
            )),
            2,
        ));
    }
    // The ref now holds the work: drop the stash entry that was only a stepping stone.
    let drop = git::run(dir, &["stash", "drop", "-q"])?;
    if !drop.success() {
        // Harmless: the work is safe in the ref; a stray stash entry is only clutter.
        tracing::warn!(sha, "could not drop the stash entry after parking: {}", drop.text());
    }
    Ok(Saved::Saved(sha))
}

/// Shared mode: put a milestone's parked work back into the (clean) main folder and drop its
/// ref. A failed apply leaves the ref in place, with a message that says where the work is.
pub fn restore_shared(root: &Path, ref_name: &str, task: &str) -> Result<()> {
    let sha = read(root, ref_name)?.ok_or_else(|| {
        Error::wrap(
            std::io::Error::other(format!("{task}'s parked work {ref_name} is gone")),
            2,
        )
    })?;
    let out = git::run(root, &["stash", "apply", "-q", &sha])?;
    if !out.success() {
        // The tree was clean before the apply: only the apply's own leftovers are undone.
        let _ = git::run(root, &["reset", "--hard", "-q", "HEAD"])?;
        let _ = git::run(root, &["clean", "-fdq"])?;
        return Err(Error::wrap(
            std::io::Error::other(format!(
                "could not restore {task}'s parked work: it conflicts with what landed since. It is kept in {ref_name} (`git stash apply {sha}` to look at it)\n{}",
                out.text()
            )),
            2,
        ));
    }
    delete(root, ref_name)
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
    fn names() {
        assert_eq!(branch_slug("feature/x/y"), "feature-x-y");
        assert_eq!(
            task_ref("feature/event-manager", "M4"),
            "refs/orch/feature-event-manager/M4"
        );
    }

    #[test]
    fn write_read_delete_and_list() {
        let (_t, root) = repo();
        let sha = g(&root, &["rev-parse", "HEAD"]);
        let r = task_ref("feat/x", "M1");
        assert_eq!(read_state(&root, &r).unwrap(), RefState::Missing);
        write(&root, &r, &sha).unwrap();
        assert_eq!(read_state(&root, &r).unwrap(), RefState::Valid(sha.clone()));
        assert_eq!(read(&root, &r).unwrap(), Some(sha.clone()));
        write(&root, &task_ref("feat/x", "M2"), "HEAD").unwrap();
        write(&root, &task_ref("feat/y", "M1"), "HEAD").unwrap();
        let listed = list(&root, "feat/x").unwrap();
        assert_eq!(
            listed.iter().map(|h| h.task.as_str()).collect::<Vec<_>>(),
            ["M1", "M2"]
        );
        delete(&root, &r).unwrap();
        delete(&root, &r).unwrap(); // already gone: fine
        assert_eq!(read(&root, &r).unwrap(), None);
    }

    #[test]
    fn a_hidden_ref_is_not_a_branch_and_a_push_does_not_carry_it() {
        let (t, root) = repo();
        let sha = g(&root, &["rev-parse", "HEAD"]);
        write(&root, &task_ref("feat/x", "M1"), &sha).unwrap();
        assert_eq!(g(&root, &["branch", "--list", "--format=%(refname)"]), "refs/heads/main");
        let bare = t.path().join("origin.git");
        g(t.path(), &["init", "-q", "--bare", bare.to_str().unwrap()]);
        g(&root, &["remote", "add", "origin", bare.to_str().unwrap()]);
        g(&root, &["push", "-q", "origin", "main"]);
        assert_eq!(g(&bare, &["for-each-ref", "--format=%(refname)"]), "refs/heads/main");
    }

    #[test]
    fn a_ref_to_a_missing_object_is_detected_not_mistaken_for_missing() {
        let (_t, root) = repo();
        let r = task_ref("feat/x", "M1");
        let path = root.join(".git").join(&r);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "1111111111111111111111111111111111111111\n").unwrap();
        assert_eq!(
            read_state(&root, &r).unwrap(),
            RefState::Broken("1111111111111111111111111111111111111111".into())
        );
        assert_eq!(read(&root, &r).unwrap(), None);
        // it can be listed and removed, and writing a missing object is refused
        assert_eq!(list(&root, "feat/x").unwrap().len(), 1);
        assert!(write(&root, &r, "1111111111111111111111111111111111111111").is_err());
        delete(&root, &r).unwrap();
        assert_eq!(read_state(&root, &r).unwrap(), RefState::Missing);
    }

    #[test]
    fn save_worktree_keeps_dirty_files_in_the_ref_and_cleans_the_tree() {
        let (t, root) = repo();
        g(&root, &["switch", "-q", "-c", "feat/x"]);
        let wt = t.path().join("wt");
        g(&root, &["worktree", "add", "-q", "--detach", wt.to_str().unwrap(), "feat/x"]);
        std::fs::write(wt.join("src/a.txt"), "changed\n").unwrap();
        std::fs::write(wt.join("new.txt"), "new\n").unwrap();
        let r = task_ref("feat/x", "M1");
        let Saved::Saved(sha) = save_tree(&wt, &root, "feat/x", &r, "orch wip M1", SaveMode::Worktree).unwrap()
        else {
            panic!("expected a save");
        };
        assert!(!git::is_dirty(&wt).unwrap());
        assert_eq!(read(&root, &r).unwrap(), Some(sha.clone()));
        assert_eq!(g(&root, &["show", &format!("{sha}:new.txt")]), "new");
        assert_eq!(g(&root, &["show", &format!("{sha}:src/a.txt")]), "changed");
        // the feature branch got nothing and no branch was made
        assert_eq!(g(&root, &["rev-list", "--count", "feat/x"]), "1");
        assert_eq!(
            g(&root, &["branch", "--format=%(refname:short)"]).lines().count(),
            2
        );
        // a second save with nothing new keeps the ref
        assert!(matches!(
            save_tree(&wt, &root, "feat/x", &r, "again", SaveMode::Worktree).unwrap(),
            Saved::Saved(s) if s == sha
        ));
    }

    #[test]
    fn save_worktree_with_nothing_to_save_says_so() {
        let (t, root) = repo();
        g(&root, &["switch", "-q", "-c", "feat/x"]);
        let wt = t.path().join("wt");
        g(&root, &["worktree", "add", "-q", "--detach", wt.to_str().unwrap(), "feat/x"]);
        let r = task_ref("feat/x", "M1");
        assert_eq!(
            save_tree(&wt, &root, "feat/x", &r, "m", SaveMode::Worktree).unwrap(),
            Saved::Nothing
        );
        assert_eq!(read_state(&root, &r).unwrap(), RefState::Missing);
    }

    #[test]
    fn a_failing_save_leaves_the_work_in_place_and_says_so() {
        let (t, root) = repo();
        g(&root, &["switch", "-q", "-c", "feat/x"]);
        let wt = t.path().join("wt");
        g(&root, &["worktree", "add", "-q", "--detach", wt.to_str().unwrap(), "feat/x"]);
        std::fs::write(wt.join("new.txt"), "precious\n").unwrap();
        // a pre-commit hook is skipped (--no-verify), but a commit-msg-free failure is forced
        // by making the index unwritable: point git at a lock that already exists
        let gitdir = g(&wt, &["rev-parse", "--absolute-git-dir"]);
        std::fs::write(Path::new(&gitdir).join("index.lock"), "").unwrap();
        let e = save_tree(&wt, &root, "feat/x", &task_ref("feat/x", "M1"), "m", SaveMode::Worktree).unwrap_err();
        assert!(e.to_string().contains("untouched"), "{e}");
        assert_eq!(std::fs::read_to_string(wt.join("new.txt")).unwrap(), "precious\n");
        assert_eq!(read(&root, &task_ref("feat/x", "M1")).unwrap(), None);
    }

    #[test]
    fn save_shared_stashes_into_the_ref_and_restore_brings_it_back() {
        let (_t, root) = repo();
        g(&root, &["switch", "-q", "-c", "feat/x"]);
        std::fs::write(root.join("src/a.txt"), "edited\n").unwrap();
        std::fs::write(root.join("untracked.txt"), "u\n").unwrap();
        let r = task_ref("feat/x", "M1");
        let Saved::Saved(sha) = save_tree(&root, &root, "feat/x", &r, "orch M1", SaveMode::Shared).unwrap()
        else {
            panic!("expected a save");
        };
        assert!(!git::is_dirty(&root).unwrap());
        assert!(!root.join("untracked.txt").exists());
        assert_eq!(read(&root, &r).unwrap(), Some(sha));
        assert_eq!(g(&root, &["stash", "list"]), "", "no stash entry is left behind");
        restore_shared(&root, &r, "M1").unwrap();
        assert_eq!(std::fs::read_to_string(root.join("src/a.txt")).unwrap(), "edited\n");
        assert_eq!(std::fs::read_to_string(root.join("untracked.txt")).unwrap(), "u\n");
        assert_eq!(read_state(&root, &r).unwrap(), RefState::Missing);
    }

    #[test]
    fn save_shared_on_a_clean_tree_is_nothing() {
        let (_t, root) = repo();
        let r = task_ref("feat/x", "M1");
        assert_eq!(
            save_tree(&root, &root, "feat/x", &r, "m", SaveMode::Shared).unwrap(),
            Saved::Nothing
        );
    }

    #[test]
    fn a_conflicting_restore_keeps_the_ref() {
        let (_t, root) = repo();
        g(&root, &["switch", "-q", "-c", "feat/x"]);
        std::fs::write(root.join("src/a.txt"), "mine\n").unwrap();
        let r = task_ref("feat/x", "M1");
        save_tree(&root, &root, "feat/x", &r, "m", SaveMode::Shared).unwrap();
        commit_file(&root, "src/a.txt", "landed\n");
        let e = restore_shared(&root, &r, "M1").unwrap_err();
        assert!(e.to_string().contains("kept in"), "{e}");
        assert!(read(&root, &r).unwrap().is_some());
        assert!(!git::is_dirty(&root).unwrap());
    }
}
