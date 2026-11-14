//! `orch finish [--no-push]` (§13, §17): the only place a run touches the remote.
//!
//! The order is the one Python v4.10.1's `cmd_finish` uses, and nothing is pushed or deleted
//! before every check has passed: the milestones must all have landed, the main tree must be
//! clean and on the feature branch, the plan's check command must pass — only then are the
//! worker worktrees removed and the run's hidden refs deleted, and only then is the feature
//! branch pushed (never anything else, never force-pushed) and a PR opened into the base.
//! Every step is idempotent, so a rerun after a failed push or PR step is safe: worktrees and
//! refs that are already gone are simply not there.

use crate::error::Result;
use crate::git;
use crate::legacy;
use crate::refs;
use crate::run_store::RunSettings;
use crate::store::Store;
use crate::task::{State, Task};
use crate::worktree;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// A finish that could not complete: the lines printed before the failure and the message the
/// CLI prints as `ERROR: <message>` (exit 2, like Python's `die`).
#[derive(Debug)]
pub struct Failure {
    pub lines: Vec<String>,
    pub message: String,
}

fn fail(lines: &mut Vec<String>, msg: impl Into<String>) -> Failure {
    Failure {
        lines: std::mem::take(lines),
        message: msg.into(),
    }
}

fn fail_err(lines: &mut Vec<String>, e: crate::Error) -> Failure {
    fail(lines, e.to_string())
}

/// How `finish` runs.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// `--no-push`: stop before any remote call.
    pub no_push: bool,
    /// The `gh` binary to open the PR with, when one is available; `None` skips the PR step.
    pub gh: Option<PathBuf>,
}

/// Whether a `gh` binary is on `PATH` (Python's `shutil.which("gh")`).
pub fn gh_available() -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| executable_file(dir.join("gh")))
}

#[cfg(unix)]
fn executable_file(p: PathBuf) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable_file(p: PathBuf) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file())
}

/// The branch a run finishes into when none was recorded: origin's default branch, else
/// `main`, else `master` (Python's `default_base`).
fn default_base(root: &Path, branch: &str) -> Result<String> {
    let head = git::run(root, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])?;
    let mut candidates: Vec<String> = Vec::new();
    if head.success() {
        let name = head.out();
        candidates.push(name.split_once('/').map_or(name, |(_, b)| b).to_string());
    }
    candidates.extend(["main".to_string(), "master".to_string()]);
    for b in candidates {
        if !b.is_empty() && b != branch && git::branch_exists(root, &b)? {
            return Ok(b);
        }
    }
    Err(crate::error::Error::wrap(
        std::io::Error::other(format!(
            "can't tell which branch {branch} starts from — add `Base: <branch>` to the plan header and run `orch sync`"
        )),
        crate::error::EXIT_USAGE,
    ))
}

/// Run the plan's check command through the shell in `root`; `Err` carries its last 30 lines
/// of output, as Python shows them.
fn run_check(command: &str, root: &Path) -> std::result::Result<(), String> {
    let out = Command::new("sh")
        .args(["-c", command])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("could not run `{command}`: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<&str> = text.lines().collect();
    Err(lines[lines.len().saturating_sub(30)..].join("\n"))
}

/// `orch finish` (§13): validate, run the plan's check command, clean the run's worktrees and
/// hidden refs up, push the feature branch and open a PR (or say what to do instead). Returns
/// the lines to print, in order; a [`Failure`] carries the lines printed before it stopped.
pub fn run(
    store: &mut Store,
    root: &Path,
    opts: &Options,
) -> std::result::Result<Vec<String>, Failure> {
    let mut lines: Vec<String> = Vec::new();
    let s = match store.run_settings() {
        Ok(Some(s)) => s,
        Ok(None) => {
            return Err(fail(
                &mut lines,
                "no run in .orchestrate/state.db — run `orch sync` after writing plan.md",
            ));
        }
        Err(e) => return Err(fail_err(&mut lines, e.into())),
    };

    // every milestone must have landed (merged or deliberately skipped)
    let tasks = match store.list_tasks() {
        Ok(t) => t,
        Err(e) => return Err(fail_err(&mut lines, e.into())),
    };
    let left: Vec<String> = tasks
        .iter()
        .filter(|t| !matches!(t.status, State::Merged | State::Skipped))
        .map(|t| format!("{} ({})", t.id, t.status))
        .collect();
    if !left.is_empty() {
        return Err(fail(
            &mut lines,
            format!("not every milestone has landed: {}", left.join(", ")),
        ));
    }

    // the main tree must be clean and on the feature branch
    let branch = match git::current_branch(root) {
        Ok(b) => b,
        Err(e) => return Err(fail_err(&mut lines, e)),
    };
    let dirty = match git::is_dirty(root) {
        Ok(d) => d,
        Err(e) => return Err(fail_err(&mut lines, e)),
    };
    if branch != s.branch || dirty {
        return Err(fail(
            &mut lines,
            format!("main tree must be clean and on {}", s.branch),
        ));
    }

    let base = match s.base.as_deref().filter(|b| !b.is_empty()) {
        Some(b) => b.to_string(),
        None => match default_base(root, &s.branch) {
            Ok(b) => b,
            Err(e) => return Err(fail_err(&mut lines, e)),
        },
    };

    // the plan's check command, before anything is pushed or deleted
    if !s.check_command.is_empty()
        && let Err(tail) = run_check(&s.check_command, root)
    {
        return Err(fail(
            &mut lines,
            format!(
                "the check fails on {} — fix it before finishing:\n{}",
                s.branch, tail
            ),
        ));
    }

    let plan_order: Vec<String> = match store.load_run() {
        Ok(Some(r)) => r.plan.into_iter().map(|p| p.id).collect(),
        Ok(None) => Vec::new(),
        Err(e) => return Err(fail_err(&mut lines, e.into())),
    };

    // `Commits: single`: one commit for the whole run, built from the title and the milestones
    if s.commits == "single" {
        match squash_single(root, &s, &base, &plan_order, &tasks) {
            Ok(Some(line)) => lines.push(line),
            Ok(None) => {}
            Err(e) => return Err(fail_err(&mut lines, e)),
        }
    }

    // task branches left by older orch versions are picked up before the report
    match legacy::pickup(root, &s.branch, &plan_order) {
        Ok(pickups) => {
            for p in pickups {
                lines.push(p.line());
            }
        }
        Err(e) => return Err(fail_err(&mut lines, e)),
    }

    // the worker worktrees, then the run's hidden refs
    match worktree::cleanup(store, root) {
        Ok(Some(cleanup)) => {
            for l in cleanup {
                lines.push(l.to_string());
            }
        }
        Ok(None) => {}
        Err(e) => return Err(fail_err(&mut lines, e)),
    }
    let hidden = match refs::list(root, &s.branch) {
        Ok(h) => h,
        Err(e) => return Err(fail_err(&mut lines, e)),
    };
    for r in &hidden {
        if let Err(e) = refs::delete(root, &r.name) {
            return Err(fail_err(&mut lines, e));
        }
    }
    lines.push(format!(
        "CLEAN {} is the only branch this run made ({} hidden ref(s) removed)",
        s.branch,
        hidden.len()
    ));

    // the remote: only the feature branch, only with upstream set, never forced
    if opts.no_push {
        lines.push(format!(
            "NOT PUSHED (--no-push): push {} and open a PR into {} when ready",
            s.branch, base
        ));
        return Ok(lines);
    }
    let remote = match git::run(root, &["remote", "get-url", "origin"]) {
        Ok(r) => r,
        Err(e) => return Err(fail_err(&mut lines, e)),
    };
    if !remote.success() {
        lines.push(format!(
            "NO REMOTE: no `origin` remote — merge {} into {} yourself",
            s.branch, base
        ));
        return Ok(lines);
    }
    let url = remote.out().to_string();
    if let Err(e) = git::check(root, &["push", "-q", "-u", "origin", &s.branch]) {
        return Err(fail_err(&mut lines, e));
    }
    lines.push(format!("PUSHED {} to origin", s.branch));

    let title = pr_title(&s);
    let compare = github_compare_url(&url, &base, &s.branch);
    if compare.is_some()
        && let Some(gh) = opts.gh.as_deref()
    {
        let body = match pr_body(root, &base, &title) {
            Ok(b) => b,
            Err(e) => return Err(fail_err(&mut lines, e)),
        };
        match gh_pr_create(root, gh, &base, &s.branch, &title, &body) {
            Ok(url) => {
                lines.push(format!("PR {url}"));
                return Ok(lines);
            }
            Err(why) => lines.push(format!("NOTE gh pr create failed: {why}")),
        }
    }
    lines.push(match compare {
        Some(url) => format!("OPEN A PR from {} into {}: {url}", s.branch, base),
        None => format!("OPEN A PR from {} into {}", s.branch, base),
    });
    Ok(lines)
}

/// The run title for the PR and the squash commit: the plan title, else the branch.
fn pr_title(s: &RunSettings) -> String {
    if s.title.is_empty() {
        s.branch.clone()
    } else {
        s.title.clone()
    }
}

/// The commit subjects between `base` and the branch tip, in order.
fn subjects(root: &Path, base: &str) -> Result<(String, Vec<String>)> {
    let mb = git::check(root, &["merge-base", base, "HEAD"])?;
    let out = git::check(
        root,
        &["log", "--reverse", "--format=%s", &format!("{mb}..HEAD")],
    )?;
    let subjects = out
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    Ok((mb, subjects))
}

/// The milestone commit lines for the PR body. After a `Commits: single` squash the run is one
/// commit whose body already lists the milestone lines (`- <subject>`), so those are used; a
/// squashed commit's own subject (the plan title) is never presented as a milestone line.
fn milestone_lines(root: &Path, base: &str) -> Result<Vec<String>> {
    let (mb, subjects) = subjects(root, base)?;
    if subjects.len() != 1 {
        return Ok(subjects);
    }
    let body = git::check(root, &["log", "-1", "--format=%b", &format!("{mb}..HEAD")])?;
    let bullets: Vec<String> = body
        .lines()
        .filter_map(|l| l.strip_prefix("- "))
        .map(str::to_string)
        .collect();
    Ok(if bullets.is_empty() { subjects } else { bullets })
}

/// `Commits: single`: when the run has more than one commit, squash it into one on the feature
/// branch — the plan title, one line per commit, and the landed milestones as a trailer.
/// Returns the `SQUASHED` line, or `None` when there was nothing to squash.
fn squash_single(
    root: &Path,
    s: &RunSettings,
    base: &str,
    order: &[String],
    tasks: &[Task],
) -> Result<Option<String>> {
    let (mb, subjects) = subjects(root, base)?;
    if subjects.len() <= 1 {
        return Ok(None);
    }
    let landed: Vec<&str> = order
        .iter()
        .filter(|id| tasks.iter().any(|t| &t.id == *id && t.status == State::Merged))
        .map(String::as_str)
        .collect();
    git::check(root, &["reset", "--soft", "-q", &mb])?;
    let mut msg = format!("{}\n\n", pr_title(s));
    msg.push_str(
        &subjects
            .iter()
            .map(|x| format!("- {x}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    if !landed.is_empty() {
        msg.push_str(&format!("\n\nMilestones: {}", landed.join(", ")));
    }
    git::check(root, &["commit", "-q", "-m", &msg])?;
    Ok(Some(format!(
        "SQUASHED {} commits into one (Commits: single)",
        subjects.len()
    )))
}

/// Python's `github_compare_url`: a GitHub remote URL (`git@github.com:o/r(.git)` or
/// `https://[user@]github.com/o/r(.git)`) becomes its compare view, anything else `None`.
pub fn github_compare_url(remote: &str, base: &str, branch: &str) -> Option<String> {
    let s = remote.trim();
    let rest = if let Some(r) = s.strip_prefix("git@github.com:") {
        r
    } else {
        let r = s.strip_prefix("https://")?;
        let r = r.split_once('@').map_or(r, |(_, after)| after);
        r.strip_prefix("github.com/")?
    };
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut it = rest.split('/');
    let owner = it.next()?;
    let repo = it.next()?;
    if it.next().is_some() || owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!(
        "https://github.com/{owner}/{repo}/compare/{base}...{branch}?expand=1"
    ))
}

/// The default PR body: the plan title and one line per milestone commit, and nothing else —
/// no attribution, no "Generated with" line. When the lead left a `.orchestrate/summary.md`,
/// that is the body instead ([`gh_pr_create`] decides).
fn pr_body(root: &Path, base: &str, title: &str) -> Result<String> {
    let mut body = title.to_string();
    let subjects = milestone_lines(root, base)?;
    if !subjects.is_empty() {
        body.push_str("\n\n");
        body.push_str(
            &subjects
                .iter()
                .map(|s| format!("- {s}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    Ok(body)
}

/// `gh pr create` into `base`, with a 120s timeout. `Ok` carries the last line of gh's stdout
/// ("created" when it printed nothing); `Err` carries what gh said.
fn gh_pr_create(
    root: &Path,
    gh: &Path,
    base: &str,
    branch: &str,
    title: &str,
    body: &str,
) -> std::result::Result<String, String> {
    let mut args = vec![
        "pr".to_string(),
        "create".to_string(),
        "--base".to_string(),
        base.to_string(),
        "--head".to_string(),
        branch.to_string(),
        "--title".to_string(),
        title.to_string(),
    ];
    let summary = root.join(crate::paths::ORCH_DIR).join("summary.md");
    if summary.exists() {
        args.push("--body-file".into());
        args.push(summary.to_string_lossy().into_owned());
    } else {
        args.push("--body".into());
        args.push(body.to_string());
    }
    let mut child = Command::new(gh)
        .args(&args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let drain = |s: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut buf = String::new();
            if let Some(mut s) = s {
                let _ = s.read_to_string(&mut buf);
            }
            buf
        })
    };
    let out = drain(child.stdout.take().map(|s| Box::new(s) as _));
    let err = drain(child.stderr.take().map(|s| Box::new(s) as _));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if start.elapsed() > Duration::from_secs(120) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    match status {
        None => Err("timed out after 120s".into()),
        Some(st) if st.success() => Ok(stdout
            .trim()
            .lines()
            .last()
            .unwrap_or("created")
            .to_string()),
        Some(_) => Err(format!("{}{}", stdout, stderr).trim().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_urls_for_github_remotes_only() {
        assert_eq!(
            github_compare_url("https://github.com/acme/demo.git", "main", "feature/demo")
                .as_deref(),
            Some("https://github.com/acme/demo/compare/main...feature/demo?expand=1")
        );
        for remote in [
            "git@github.com:acme/demo.git",
            "git@github.com:acme/demo",
            "https://github.com/acme/demo",
            "https://user@github.com/acme/demo.git",
            "https://github.com/acme/demo/",
        ] {
            assert!(github_compare_url(remote, "main", "b").is_some(), "{remote}");
        }
        for remote in [
            "git@gitlab.com:acme/demo.git",
            "https://gitlab.com/acme/demo.git",
            "ssh://git@github.com/acme/demo.git",
            "https://github.com/acme/demo/extra",
            "https://github.com/acme",
            "/local/path",
        ] {
            assert!(github_compare_url(remote, "main", "b").is_none(), "{remote}");
        }
    }
}
