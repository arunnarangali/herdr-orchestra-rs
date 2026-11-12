//! Locating the main repository root and its `.orchestrate/` folder.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Name of the run folder under the main repository root.
pub const ORCH_DIR: &str = ".orchestrate";

/// The main repository root as seen from `start`. Inside a linked git worktree this is the
/// main checkout, not the worktree (uses `git rev-parse --git-common-dir`, like Python's
/// `main_root()`).
pub fn main_root(start: &Path) -> Result<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(start)
        .output()
        .map_err(Error::GitUnavailable)?;
    if !out.status.success() {
        return Err(Error::NotInGitRepo(start.to_path_buf()));
    }
    let common = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    common
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| Error::NotInGitRepo(start.to_path_buf()))
}

/// The `.orchestrate/` folder under the main repository root (not created).
pub fn orchestrate_dir(start: &Path) -> Result<PathBuf> {
    Ok(main_root(start)?.join(ORCH_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?} failed");
    }

    fn repo() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
        (t, root)
    }

    #[test]
    fn root_from_root_and_subdir() {
        let (_t, root) = repo();
        let sub = root.join("a/b");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(main_root(&root).unwrap(), root);
        assert_eq!(main_root(&sub).unwrap(), root);
        assert_eq!(orchestrate_dir(&sub).unwrap(), root.join(".orchestrate"));
    }

    #[test]
    fn root_from_linked_worktree() {
        let (t, root) = repo();
        let wt = t.path().canonicalize().unwrap().join("wt");
        git(
            &root,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "feat"],
        );
        assert_eq!(main_root(&wt).unwrap(), root);
        assert_eq!(orchestrate_dir(&wt).unwrap(), root.join(".orchestrate"));
    }

    #[test]
    fn outside_repo_is_a_clear_error() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().canonicalize().unwrap();
        // Skip if the temp dir happens to sit inside a git repo.
        if main_root(&dir).is_ok() {
            return;
        }
        let e = main_root(&dir).unwrap_err();
        assert!(matches!(e, Error::NotInGitRepo(_)));
        assert!(e.to_string().contains("not inside a git repository"));
    }
}
