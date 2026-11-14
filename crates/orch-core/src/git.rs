//! The one place orch runs `git` (§13): a narrow wrapper over the Git CLI (no libgit2).
//! Everything else in orch-core and the CLI goes through [`run`] / [`check`]; nothing builds a
//! second `Command::new("git")`. Every call returns its exit status, so a caller either looks at
//! [`GitOutput::status`] or uses [`check`], which turns a non-zero exit into an error.

use crate::error::{EXIT_USAGE, Error, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// What a finished `git` run said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOutput {
    /// The exit status (`-1` when git was killed by a signal).
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl GitOutput {
    pub fn success(&self) -> bool {
        self.status == 0
    }

    /// stdout and stderr together, trimmed: what Python prints after a failed git call.
    pub fn text(&self) -> String {
        format!("{}{}", self.stdout, self.stderr).trim().to_string()
    }

    /// stdout without its trailing whitespace.
    pub fn out(&self) -> &str {
        self.stdout.trim()
    }
}

/// A git command that exited non-zero (see [`check`]).
#[derive(Debug, thiserror::Error)]
#[error("git {args} failed in {dir}:\n{output}")]
pub struct GitFailed {
    pub args: String,
    pub dir: String,
    pub status: i32,
    pub output: String,
}

impl From<GitFailed> for Error {
    fn from(e: GitFailed) -> Self {
        Error::wrap(e, EXIT_USAGE)
    }
}

fn command(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.args(args).current_dir(dir).env("GIT_TERMINAL_PROMPT", "0");
    c
}

fn finish(out: std::process::Output) -> GitOutput {
    GitOutput {
        status: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Run `git <args>` in `dir`. An `Err` means git could not be started at all; a git that ran
/// and failed is an `Ok` with a non-zero [`GitOutput::status`].
pub fn run(dir: &Path, args: &[&str]) -> Result<GitOutput> {
    command(dir, args)
        .output()
        .map(finish)
        .map_err(Error::GitUnavailable)
}

/// [`run`] with extra environment variables (for a temporary index, an author, ...).
pub fn run_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<GitOutput> {
    let mut c = command(dir, args);
    for (k, v) in env {
        c.env(k, v);
    }
    c.output().map(finish).map_err(Error::GitUnavailable)
}

/// [`run`] with `input` on git's stdin.
pub fn run_stdin(dir: &Path, args: &[&str], input: &str) -> Result<GitOutput> {
    let mut child = command(dir, args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(Error::GitUnavailable)?;
    if let Some(mut stdin) = child.stdin.take() {
        // A git that exits early closes the pipe; its status says why, so a write error is moot.
        let _ = stdin.write_all(input.as_bytes());
    }
    child
        .wait_with_output()
        .map(finish)
        .map_err(Error::GitUnavailable)
}

/// Run `git <args>` in `dir` and demand success: the trimmed stdout, or a [`GitFailed`] carrying
/// what git said.
pub fn check(dir: &Path, args: &[&str]) -> Result<String> {
    let out = run(dir, args)?;
    if out.success() {
        Ok(out.out().to_string())
    } else {
        Err(failed(dir, args, &out))
    }
}

/// The error for a non-zero `out` of `git <args>`.
pub fn failed(dir: &Path, args: &[&str], out: &GitOutput) -> Error {
    GitFailed {
        args: args.join(" "),
        dir: dir.display().to_string(),
        status: out.status,
        output: out.text(),
    }
    .into()
}

/// True when `git status --porcelain` lists anything (untracked files count; ignored ones and
/// orch's excluded folders do not).
pub fn is_dirty(dir: &Path) -> Result<bool> {
    Ok(!check(dir, &["status", "--porcelain"])?.is_empty())
}

/// The checked-out branch name, or `HEAD` when detached.
pub fn current_branch(dir: &Path) -> Result<String> {
    check(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
}

pub fn branch_exists(dir: &Path, name: &str) -> Result<bool> {
    let full = format!("refs/heads/{name}");
    Ok(run(dir, &["rev-parse", "--verify", "--quiet", &full])?.success())
}

/// The commit `rev` names, or `None` when it names nothing (or a missing object).
pub fn resolve_commit(dir: &Path, rev: &str) -> Result<Option<String>> {
    let spec = format!("{rev}^{{commit}}");
    let out = run(dir, &["rev-parse", "--verify", "--quiet", &spec])?;
    Ok(out.success().then(|| out.out().to_string()))
}

/// How many commits `HEAD` in `dir` has that `branch` has not.
pub fn ahead_of(dir: &Path, branch: &str) -> Result<u64> {
    let range = format!("{branch}..HEAD");
    let out = run(dir, &["rev-list", "--count", &range])?;
    Ok(if out.success() {
        out.out().parse().unwrap_or(0)
    } else {
        0
    })
}

#[cfg(test)]
pub(crate) mod testutil {
    //! Real temporary repositories for the tests of this crate.
    use super::*;
    use std::path::PathBuf;

    pub fn g(dir: &Path, args: &[&str]) -> String {
        let out = run_env(
            dir,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ]
            .iter()
            .chain(args)
            .copied()
            .collect::<Vec<_>>(),
            &[],
        )
        .unwrap();
        assert!(out.success(), "git {args:?} failed: {}", out.text());
        out.out().to_string()
    }

    /// A repo `<tmp>/repo` on `main` with one commit containing `Cargo.toml`, `src/a.txt`.
    pub fn repo() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir(&root).unwrap();
        g(&root, &["init", "-q", "-b", "main"]);
        g(&root, &["config", "user.name", "t"]);
        g(&root, &["config", "user.email", "t@t"]);
        std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.txt"), "a\n").unwrap();
        g(&root, &["add", "-A"]);
        g(&root, &["commit", "-q", "-m", "init"]);
        (t, root)
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    #[test]
    fn run_reports_status_stdout_and_stderr() {
        let (_t, root) = repo();
        let ok = run(&root, &["rev-parse", "HEAD"]).unwrap();
        assert!(ok.success() && ok.out().len() == 40);
        let bad = run(&root, &["rev-parse", "--verify", "nope"]).unwrap();
        assert!(!bad.success());
        assert!(!bad.stderr.is_empty());
    }

    #[test]
    fn check_turns_a_failure_into_an_error_with_git_output() {
        let (_t, root) = repo();
        let e = check(&root, &["checkout", "no-such-branch"]).unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("git checkout no-such-branch failed"), "{msg}");
        assert!(msg.contains("no-such-branch"), "{msg}");
    }

    #[test]
    fn stdin_reaches_git() {
        let (_t, root) = repo();
        let o = run_stdin(&root, &["hash-object", "--stdin"], "hello\n").unwrap();
        assert_eq!(o.out(), "ce013625030ba8dba906f756967f9e9ca394464a");
    }

    #[test]
    fn dirty_and_branch_helpers() {
        let (_t, root) = repo();
        assert!(!is_dirty(&root).unwrap());
        std::fs::write(root.join("new.txt"), "x").unwrap();
        assert!(is_dirty(&root).unwrap());
        assert_eq!(current_branch(&root).unwrap(), "main");
        assert!(branch_exists(&root, "main").unwrap());
        assert!(!branch_exists(&root, "other").unwrap());
        assert_eq!(ahead_of(&root, "main").unwrap(), 0);
        assert!(resolve_commit(&root, "main").unwrap().is_some());
        assert!(resolve_commit(&root, "nope").unwrap().is_none());
    }
}
