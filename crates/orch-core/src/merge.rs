//! Landing a milestone (§13, §14, §15): squash its saved work onto the feature branch in the main
//! folder, run the plan's check on the merged result, and create the commit only when it passes.
//!
//! The rules this module keeps:
//!
//! * **Nothing unverified lands.** The squash is staged, not committed; the check runs on that
//!   staged result; only a green check is followed by `git commit`. A red check, a conflict or any
//!   error leaves the branch where it was and the main tree clean.
//! * **One commit per milestone**, with the coder's commit line and a `Milestone: Mn` trailer —
//!   never an `orch(Mn)`/`wip(Mn)` commit, never an attribution line.
//! * **A crash is repairable.** A journal row ([`crate::journal`]) is written before the first
//!   change to the branch and removed in the transaction that marks the milestone merged.
//!   [`recover`] finishes a merge whose commit landed and undoes one whose commit did not, so the
//!   milestone is merged again from its saved work (the hidden ref is only deleted once landed).
//! * **One merge at a time**, by a lock file held for the whole landing (and the recovery in it).

use crate::check;
use crate::conflict::{Issue, IssueKind};
use crate::error::{EXIT_USAGE, Error, Result};
use crate::git;
use crate::journal::Step;
use crate::manager::{CommitOutcome, MAX_CONFLICT};
use crate::refs::{self, task_ref};
use crate::run_store::RunSettings;
use crate::setup;
use crate::store::Store;
use crate::task::{State, Task};
use crate::worktree;
use fs2::FileExt;
use std::fs::File;
use std::path::Path;
use std::time::Duration;

fn die(msg: impl Into<String>) -> Error {
    Error::wrap(std::io::Error::other(msg.into()), EXIT_USAGE)
}

// ---------------------------------------------------------------- failure injection

/// A place where a test can kill the merge, like a crash would: the function returns at once and
/// runs no cleanup. The CLI turns this into exit 99 (Python's `ORCH_FAULT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultPoint {
    /// The squash is staged; nothing else happened.
    AfterSquash,
    /// The check ran (and passed); nothing is committed.
    AfterCheck,
    /// The commit exists; the journal does not know yet.
    AfterCommit,
    /// The commit and the ref cleanup are done; the journal row is still there.
    BeforeClear,
}

impl FaultPoint {
    fn parse(s: &str) -> Option<FaultPoint> {
        match s {
            "after-squash" => Some(FaultPoint::AfterSquash),
            "after-check" => Some(FaultPoint::AfterCheck),
            "after-commit" | "after-merge-commit" => Some(FaultPoint::AfterCommit),
            "before-clear" => Some(FaultPoint::BeforeClear),
            _ => None,
        }
    }
}

/// Which fault point, if any, to trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fault(pub Option<FaultPoint>);

/// The error a tripped fault point returns; its exit code is 99.
#[derive(Debug, thiserror::Error)]
#[error("injected fault at {0:?}")]
pub struct InjectedFault(pub FaultPoint);

/// The exit code of an injected fault.
pub const FAULT_EXIT: i32 = 99;

impl Fault {
    pub fn none() -> Fault {
        Fault(None)
    }

    pub fn at(p: FaultPoint) -> Fault {
        Fault(Some(p))
    }

    /// `ORCH_FAULT=<point>` (a test hook, as in Python v4.10.1).
    pub fn from_env() -> Fault {
        Fault(
            std::env::var("ORCH_FAULT")
                .ok()
                .and_then(|v| FaultPoint::parse(&v)),
        )
    }

    fn hit(&self, p: FaultPoint) -> Result<()> {
        if self.0 == Some(p) {
            Err(Error::wrap(InjectedFault(p), FAULT_EXIT))
        } else {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------- the lock

/// The exclusive merge lock (`.orchestrate/merge.lock`): two merges never run at once. A process
/// that dies releases it, so a crashed merge never blocks the next one.
pub struct MergeLock(File);

impl MergeLock {
    /// Take the lock, or fail at once (exit 3) with a line saying a merge is in progress.
    pub fn acquire(root: &Path) -> Result<MergeLock> {
        let dir = root.join(crate::paths::ORCH_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
        let path = dir.join("merge.lock");
        let f = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| Error::io(format!("opening {}", path.display()), e))?;
        f.try_lock_exclusive().map_err(|_| {
            Error::wrap(
                std::io::Error::other(
                    "another merge is in progress (.orchestrate/merge.lock is held) — wait for it, then run this again",
                ),
                3,
            )
        })?;
        Ok(MergeLock(f))
    }
}

impl Drop for MergeLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

// ---------------------------------------------------------------- the commit message

/// The coder's commit line for a milestone: the `commit` of its newest `result-N.json`, else the
/// last `COMMIT:` line of its newest `report-N.md`, else `None`. Looked up in the run's
/// `.orch-task/Mn/` and in the coder's worktree.
pub fn coder_commit_line(root: &Path, dirs: &[std::path::PathBuf], task: &str) -> Option<String> {
    let mut folders = vec![root.join(".orch-task").join(task)];
    folders.extend(dirs.iter().map(|d| d.join(".orch-task").join(task)));
    let numbered = |dir: &Path, prefix: &str, ext: &str| -> Vec<(u32, std::path::PathBuf)> {
        let mut v: Vec<(u32, std::path::PathBuf)> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let n = name.strip_prefix(prefix)?.strip_suffix(ext)?.parse().ok()?;
                Some((n, e.path()))
            })
            .collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.0));
        v
    };
    for dir in &folders {
        for (_, p) in numbered(dir, "result-", ".json") {
            let line = std::fs::read_to_string(&p)
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .and_then(|v| v.get("commit")?.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty());
            if let Some(l) = line {
                return Some(l.chars().take(120).collect());
            }
        }
    }
    for dir in &folders {
        for (_, p) in numbered(dir, "report-", ".md") {
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            let last = text
                .lines()
                .filter_map(|l| {
                    let l = l.trim();
                    (l.len() >= 7 && l[..7].eq_ignore_ascii_case("commit:"))
                        .then(|| l[7..].trim().trim_matches('`').trim().to_string())
                })
                .rfind(|l| !l.is_empty());
            if let Some(l) = last {
                return Some(l.chars().take(120).collect());
            }
        }
    }
    None
}

/// The commit message of a landed milestone: the coder's commit line (else the milestone's title),
/// a blank line, and `Milestone: Mn`. Nothing else — no attribution lines.
pub fn commit_message(root: &Path, s: &RunSettings, t: &Task) -> String {
    let dirs: Vec<std::path::PathBuf> = t
        .last_worker
        .iter()
        .chain(t.worker.iter())
        .map(|w| worktree::worker_dir(root, s, w))
        .collect();
    let subject = coder_commit_line(root, &dirs, &t.id).unwrap_or_else(|| t.title.trim().to_string());
    format!("{subject}\n\nMilestone: {}", t.id)
}

// ---------------------------------------------------------------- risk hold

/// What kinds of risky path `path` is (Python's `RISK_RULES`).
fn risk_kinds(path: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    let lower = path.to_lowercase();
    let comps: Vec<&str> = lower.split('/').collect();
    let name = *comps.last().unwrap_or(&"");
    if comps[..comps.len().saturating_sub(1)]
        .iter()
        .any(|c| matches!(*c, "migration" | "migrations" | "migrate" | "alembic"))
    {
        out.push("database migration");
    }
    let base = path.rsplit('/').next().unwrap_or(path);
    if path.starts_with(".github/")
        || path.starts_with(".circleci/")
        || matches!(base, ".gitlab-ci.yml" | ".gitlab-ci.yaml" | "Jenkinsfile" | "azure-pipelines.yml" | "azure-pipelines.yaml")
    {
        out.push("CI configuration");
    }
    let env_file = name == ".env"
        || name
            .strip_prefix(".env.")
            .is_some_and(|x| !x.is_empty() && !matches!(x, "example" | "sample" | "template"));
    let key_file = [".pem", ".key", ".p12"].iter().any(|e| name.ends_with(e));
    if env_file || key_file {
        out.push("secrets or environment file");
    }
    out
}

/// The line limit above which a change is held (`ORCH_RISK_MAX_LINES`, default 1500).
pub fn risk_max_lines() -> i64 {
    std::env::var("ORCH_RISK_MAX_LINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1500)
}

/// Deterministic reasons to hold a milestone for the developer's approval before it lands
/// (Python's `risk_flags`): deleted files, migrations, CI configuration, secret files, a
/// dependency file the plan did not list, a very large change. `diff_args` select what is
/// compared (`[merge-base, src]`, or `["--cached", "HEAD"]`). Empty when `Risk hold: off`.
pub fn risk_flags(
    dir: &Path,
    settings: &RunSettings,
    likely: &[String],
    diff_args: &[&str],
    max_lines: i64,
) -> Result<Vec<String>> {
    if !settings.risk_hold {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    let mut args = vec!["diff", "--name-status", "--no-renames"];
    args.extend(diff_args);
    for line in git::check(dir, &args)?.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let (st, path) = (parts[0], parts[parts.len() - 1]);
        if st.starts_with('D') {
            out.push(format!("deletes {path}"));
        }
        out.extend(risk_kinds(path).into_iter().map(|w| format!("{w}: {path}")));
        if setup::is_lockfile(path) && !setup::path_allowed(path, likely) {
            out.push(format!("dependency change not listed in Likely files: {path}"));
        }
    }
    let mut args = vec!["diff", "--numstat"];
    args.extend(diff_args);
    let total: i64 = git::check(dir, &args)?
        .lines()
        .map(|l| {
            let mut it = l.split('\t');
            let n = |s: Option<&str>| s.and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
            n(it.next()) + n(it.next())
        })
        .sum();
    if total > max_lines {
        out.push(format!("large change: {total} lines (limit ORCH_RISK_MAX_LINES={max_lines})"));
    }
    out.truncate(20);
    Ok(out)
}

/// The `Held` line the manager records, and the CLI prints after `HELD Mn for approval — `.
fn held_line(task: &str, risks: &[String]) -> String {
    let shown: Vec<&str> = risks.iter().take(3).map(String::as_str).collect();
    format!(
        "{task} held for approval: {}{}",
        shown.join("; "),
        if risks.len() > 3 { format!(" (+{} more)", risks.len() - 3) } else { String::new() }
    )
}

// ---------------------------------------------------------------- the check

/// Run the plan's check command in `dir` — as written, through the shell, never interpreted — by
/// the one runner ([`crate::check::run`]). `(passed, last 60 lines of its output)`; an empty
/// command passes.
pub fn run_check(command: &str, dir: &Path, limit: Duration) -> (bool, String) {
    let r = check::run(command, dir, limit, 60);
    (r.ok, r.tail)
}

// ---------------------------------------------------------------- landing

/// What landing a milestone came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landed {
    /// It is on the feature branch and marked merged. `empty`: it changed nothing (no commit).
    /// `shared`: committed in place (shared mode).
    Merged { empty: bool, shared: bool },
    /// Risky changes: nothing was merged; wait for `orch approve`.
    Held { risks: Vec<String> },
    /// The squash conflicts with what landed since; the tree is clean again.
    Conflict { files: Vec<String> },
    /// It conflicts again after `MAX_CONFLICT` resolution rounds.
    Unresolved { files: Vec<String> },
    /// It passes alone but fails the check on the feature branch; nothing was committed.
    BranchFails { tail: String },
}

impl Landed {
    /// The outcome the event manager's `Work::commit` reports.
    pub fn outcome(&self, task: &str) -> CommitOutcome {
        match self {
            Landed::Merged { .. } => CommitOutcome::Merged,
            Landed::Held { risks } => CommitOutcome::Held { line: held_line(task, risks) },
            Landed::Conflict { files } => CommitOutcome::Conflict {
                line: format!("{task}: merge conflict in {}", files_text(files)),
            },
            Landed::Unresolved { files } => CommitOutcome::Conflict {
                line: format!(
                    "{task}: merge conflict in {} still unresolved after {MAX_CONFLICT} attempts",
                    files_text(files)
                ),
            },
            Landed::BranchFails { tail } => CommitOutcome::BranchFails {
                line: format!(
                    "{task} passes alone but fails the check on the feature branch once merged\n{tail}"
                ),
            },
        }
    }
}

fn files_text(files: &[String]) -> String {
    if files.is_empty() { "(see git status)".into() } else { files.join(", ") }
}

/// What [`recover`] did about an interrupted merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// Its commit had landed: the milestone is marked merged and its saved work dropped.
    Finished(String),
    /// Its commit had not: the half-done merge was undone; it will be merged again.
    Undone(String),
    /// The journal was stale (the branch moved for another reason, or nothing had changed).
    Cleared(String),
}

impl std::fmt::Display for Recovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Recovery::Finished(t) => write!(f, "RECOVERED {t}: its commit had landed before the crash"),
            Recovery::Undone(t) => write!(
                f,
                "RECOVERED {t}: undid the half-done merge; it will be merged again from its saved work"
            ),
            Recovery::Cleared(t) => write!(f, "RECOVERED {t}: cleared a stale merge journal"),
        }
    }
}

/// A landing and what recovery did first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landing {
    pub outcome: Landed,
    pub recovered: Option<Recovery>,
}

fn settings_of(store: &Store) -> Result<RunSettings> {
    store
        .run_settings()?
        .ok_or_else(|| die("no run in .orchestrate/state.db — run `orch sync` after writing plan.md"))
}

/// A milestone counts as approved once the developer ran `orch approve` on it: its history has a
/// `held -> committed` step.
fn approved(store: &Store, task: &str) -> Result<bool> {
    Ok(store
        .history(task)?
        .iter()
        .any(|h| h.from == State::Held && h.to == State::Committed))
}

fn was_reviewed(store: &Store, task: &str) -> Result<bool> {
    Ok(store.history(task)?.iter().any(|h| h.to == State::Reviewed))
}

fn likely_files(store: &Store, task: &str) -> Result<Vec<String>> {
    Ok(store
        .load_run()?
        .and_then(|r| r.plan.into_iter().find(|p| p.id == task))
        .map(|p| p.files)
        .unwrap_or_default())
}

fn git_identity(dir: &Path) -> Result<Vec<&'static str>> {
    let have = git::run(dir, &["config", "user.email"])?.success()
        && git::run(dir, &["config", "user.name"])?.success();
    Ok(if have { vec![] } else { vec!["-c", "user.name=orch", "-c", "user.email=orch@localhost"] })
}

/// Undo a staged or half-merged tree back to `rev`. An error here is surfaced, never swallowed:
/// the journal then stays so recovery tries again.
fn reset_hard(root: &Path, rev: &str) -> Result<()> {
    git::check(root, &["reset", "--hard", "-q", rev])?;
    Ok(())
}

/// Land milestone `task` on the feature branch. First repairs an interrupted merge, if any (under
/// the same lock), then squash-merges the milestone's saved work, checks, commits. In shared mode
/// the work is already in the main folder and is committed in place.
pub fn land(store: &mut Store, root: &Path, task: &str, fault: &Fault) -> Result<Landing> {
    let _lock = MergeLock::acquire(root)?;
    let recovered = recover_locked(store, root)?;
    let t = store.get_task(task)?;
    if t.status == State::Merged {
        return Ok(Landing { outcome: Landed::Merged { empty: false, shared: false }, recovered });
    }
    // refuse before touching anything when the milestone cannot become `merged` from here
    if let Err(e) = crate::task::check_transition(task, t.status, State::Merged, crate::task::Mode::Auto) {
        return Err(die(e.to_string()));
    }
    let s = settings_of(store)?;
    let outcome = if s.isolation == "shared" {
        land_shared(store, root, &s, &t)?
    } else {
        land_worktree(store, root, &s, &t, fault)?
    };
    Ok(Landing { outcome, recovered })
}

fn land_shared(store: &mut Store, root: &Path, s: &RunSettings, t: &Task) -> Result<Landed> {
    git::check(root, &["add", "-A"])?;
    if !approved(store, &t.id)? {
        let risks = risk_flags(root, s, &likely_files(store, &t.id)?, &["--cached", "HEAD"], risk_max_lines())?;
        if !risks.is_empty() {
            git::check(root, &["reset", "-q"])?; // unstage; the work stays in the tree until approved
            return Ok(Landed::Held { risks });
        }
    }
    let empty = git::run(root, &["diff", "--cached", "--quiet"])?.success();
    if !empty {
        let mut args = git_identity(root)?;
        let msg = commit_message(root, s, t);
        args.extend(["commit", "-q", "-m", &msg]);
        let out = git::run(root, &args)?;
        if !out.success() {
            return Err(die(out.text()));
        }
    }
    store.journal_finish(&t.id)?;
    Ok(Landed::Merged { empty, shared: true })
}

/// The commit holding the milestone's saved work: its hidden ref after saving whatever its coder's
/// folder holds. A milestone whose coder changed nothing has the branch tip as its work.
pub fn saved_work(store: &Store, root: &Path, s: &RunSettings, t: &Task) -> Result<String> {
    worktree::save_task_work(store, root, &t.id, "ready to land")?;
    let tref = task_ref(&s.branch, &t.id);
    if let Some(sha) = refs::read(root, &tref)? {
        return Ok(sha);
    }
    for w in t.worker.iter().chain(t.last_worker.iter()) {
        let dir = worktree::worktree_path(root, w);
        if dir.exists() {
            let head = git::check(&dir, &["rev-parse", "HEAD"])?;
            refs::write(root, &tref, &head)?;
            return Ok(head);
        }
    }
    Err(die(format!("{} has no saved work ({tref})", t.id)))
}

fn land_worktree(
    store: &mut Store,
    root: &Path,
    s: &RunSettings,
    t: &Task,
    fault: &Fault,
) -> Result<Landed> {
    let task = t.id.as_str();
    if git::current_branch(root)? != s.branch || git::is_dirty(root)? {
        return Err(die(format!("main tree must be clean and on {}", s.branch)));
    }
    let src = saved_work(store, root, s, t)?;
    if !approved(store, task)? {
        let mb = git::check(root, &["merge-base", &s.branch, &src])?;
        let risks = risk_flags(root, s, &likely_files(store, task)?, &[&mb, &src], risk_max_lines())?;
        if !risks.is_empty() {
            return Ok(Landed::Held { risks });
        }
    }
    let head = git::check(root, &["rev-parse", "HEAD"])?;
    // the journal first: from here a crash is recoverable
    store.journal_begin(task, &head, &src)?;
    let mut sq_args = git_identity(root)?;
    sq_args.extend(["merge", "--squash", &src]);
    let squash = git::run(root, &sq_args)?;
    if squash.success() {
        store.journal_step(Step::Squashed, None)?;
    }
    fault.hit(FaultPoint::AfterSquash)?;
    if !squash.success() {
        let files: Vec<String> = git::run(root, &["diff", "--name-only", "--diff-filter=U"])?
            .stdout
            .split_whitespace()
            .map(str::to_string)
            .collect();
        reset_hard(root, "HEAD")?;
        if files.is_empty() {
            // not a content conflict: git could not merge at all, so say what it said
            store.journal_clear()?;
            return Err(die(format!("git merge --squash of {task} failed, nothing landed:\n{}", squash.text())));
        }
        let rereview = was_reviewed(store, task)? && plan_wants_review(store, task)?;
        store.record_issue(&Issue {
            task: task.to_string(),
            kind: IssueKind::Conflict,
            files: files.clone(),
            detail: String::new(),
            resolution_base: rereview.then(|| src.clone()),
            rereview,
        })?;
        store.journal_clear()?;
        return Ok(if t.conflict_rounds >= MAX_CONFLICT {
            Landed::Unresolved { files }
        } else {
            Landed::Conflict { files }
        });
    }
    let empty = git::run(root, &["diff", "--cached", "--quiet"])?.success();
    if !empty {
        // check BEFORE committing: nothing red ever lands on the feature branch
        let (ok, tail) = match setup::ensure_setup(store, s, root) {
            Ok(_) => run_check(&s.check_command, root, check::timeout_from_env()),
            Err(e) => (false, e.to_string()),
        };
        if ok {
            store.journal_step(Step::Checked, None)?;
        }
        fault.hit(FaultPoint::AfterCheck)?;
        if !ok {
            reset_hard(root, "HEAD")?;
            store.record_issue(&Issue {
                task: task.to_string(),
                kind: IssueKind::Check,
                files: vec![],
                detail: tail.clone(),
                resolution_base: None,
                rereview: false,
            })?;
            store.journal_clear()?;
            return Ok(Landed::BranchFails { tail });
        }
        let mut args = git_identity(root)?;
        let msg = commit_message(root, s, t);
        args.extend(["commit", "-q", "-m", &msg]);
        let out = git::run(root, &args)?;
        if !out.success() {
            reset_hard(root, &head)?;
            store.journal_clear()?;
            return Err(die(format!("git commit failed, nothing landed:\n{}", out.text())));
        }
        fault.hit(FaultPoint::AfterCommit)?;
        let sha = git::check(root, &["rev-parse", "HEAD"])?;
        store.journal_step(Step::Committed, Some(&sha))?;
    }
    refs::delete(root, &task_ref(&s.branch, task))?;
    fault.hit(FaultPoint::BeforeClear)?;
    store.journal_finish(task)?;
    Ok(Landed::Merged { empty, shared: false })
}

fn plan_wants_review(store: &Store, task: &str) -> Result<bool> {
    Ok(store
        .load_run()?
        .and_then(|r| r.plan.into_iter().find(|p| p.id == task))
        .is_some_and(|p| p.review))
}

/// Repair an interrupted merge (the journal row), under the merge lock: finish it when its commit
/// landed, undo it when not. `Ok(None)` when there was nothing to repair or a live merge holds the
/// lock (that merge is not interrupted).
pub fn recover(store: &mut Store, root: &Path) -> Result<Option<Recovery>> {
    match MergeLock::acquire(root) {
        Ok(_lock) => recover_locked(store, root),
        Err(_) => Ok(None),
    }
}

fn recover_locked(store: &mut Store, root: &Path) -> Result<Option<Recovery>> {
    let Some(j) = store.journal()? else {
        return Ok(None);
    };
    let task = j.task.clone();
    let head = git::check(root, &["rev-parse", "HEAD"])?;
    let branch_ref = store.run_settings()?.map(|s| task_ref(&s.branch, &task));
    if head != j.head_before {
        let body = git::check(root, &["log", "-1", "--format=%B"])?;
        let landed = j.commit.as_deref() == Some(head.as_str())
            || body.lines().any(|l| l == format!("Milestone: {task}"));
        let status = store.get_task(&task).map(|t| t.status).ok();
        if landed && status.is_some() {
            if let Some(r) = &branch_ref {
                refs::delete(root, r)?;
            }
            store.journal_finish(&task)?;
            return Ok(Some(Recovery::Finished(task)));
        }
        // HEAD moved for another reason: leave the branch alone
        store.journal_clear()?;
        return Ok(Some(Recovery::Cleared(task)));
    }
    if git::is_dirty(root)? {
        if git::resolve_commit(root, &j.src)?.is_none() {
            return Err(die(format!(
                "an interrupted merge of {task} left changes in the main folder, and its saved work {} is gone — look at `git status` before doing anything else",
                j.src
            )));
        }
        reset_hard(root, &j.head_before)?;
        store.journal_clear()?;
        return Ok(Some(Recovery::Undone(task)));
    }
    store.journal_clear()?;
    Ok(Some(Recovery::Cleared(task)))
}
