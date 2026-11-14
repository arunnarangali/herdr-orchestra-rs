//! Worker worktrees and the run's git operations (§13): the feature branch, one *detached*
//! worktree per coder at `../<repo>-wt/<name>` (never a per-worker branch), the starting point
//! of a milestone (`prepare`), saving a milestone's work (`park`) and `cleanup`.
//!
//! The rule everywhere: uncommitted work is never destroyed. A dirty tree is saved to its
//! milestone's hidden ref (see [`crate::refs`]) or refused with a message naming the owner.

use crate::error::{EXIT_USAGE, Error, Result};
use crate::git;
use crate::refs::{self, RefState, SaveMode, Saved, task_ref};
use crate::run_store::{AgentRole, RunSettings, is_active};
use crate::setup::{self, has_setup};
use crate::store::Store;
use crate::task::State;
use std::path::{Path, PathBuf};

/// The folder orch's task contract folders live in, inside each work dir.
pub const TASK_DIR: &str = ".orch-task";

fn die(msg: impl Into<String>) -> Error {
    Error::wrap(std::io::Error::other(msg.into()), EXIT_USAGE)
}

fn io(context: &str, e: std::io::Error) -> Error {
    Error::io(context, e)
}

// ---------------------------------------------------------------- worktree API

/// `<parent of root>/<repo>-wt`: where the worker worktrees live, outside the repo.
pub fn worktrees_dir(root: &Path) -> PathBuf {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".into());
    root.parent()
        .unwrap_or(root)
        .join(format!("{name}-wt"))
}

/// The worktree path of a worker: `../<repo>-wt/<name>`.
pub fn worktree_path(root: &Path, worker: &str) -> PathBuf {
    worktrees_dir(root).join(worker)
}

/// The folder a worker codes in: its worktree, or the main folder in shared mode.
pub fn worker_dir(root: &Path, settings: &RunSettings, worker: &str) -> PathBuf {
    if settings.isolation == "worktree" {
        worktree_path(root, worker)
    } else {
        root.to_path_buf()
    }
}

/// Whether `git status --porcelain` is empty in `dir`.
pub fn is_clean(dir: &Path) -> Result<bool> {
    Ok(!git::is_dirty(dir)?)
}

/// Create the worker's detached worktree on `branch`. `Some(path)` when it was created, `None`
/// when the folder was already there (so running `start` twice changes nothing).
pub fn create(root: &Path, worker: &str, branch: &str) -> Result<Option<PathBuf>> {
    let p = worktree_path(root, worker);
    if p.exists() {
        return Ok(None);
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io(&format!("creating {}", parent.display()), e))?;
    }
    git::check(
        root,
        &["worktree", "add", "--detach", &p.to_string_lossy(), branch],
    )?;
    Ok(Some(p))
}

/// What [`remove`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// There was no such worktree.
    Absent,
    Removed(PathBuf),
    /// It has unsaved changes, so it was left alone.
    Dirty(PathBuf),
}

/// Remove a worker's worktree when it is clean. Never forces: a tree with unsaved changes stays.
pub fn remove(root: &Path, worker: &str) -> Result<Removal> {
    let p = worktree_path(root, worker);
    if !p.exists() {
        return Ok(Removal::Absent);
    }
    if git::is_dirty(&p)? {
        return Ok(Removal::Dirty(p));
    }
    git::check(root, &["worktree", "remove", &p.to_string_lossy()])?;
    Ok(Removal::Removed(p))
}

/// `git worktree prune`.
pub fn prune(root: &Path) -> Result<()> {
    git::check(root, &["worktree", "prune"])?;
    Ok(())
}

/// Keep `.orchestrate/` and `.orch-task/` out of `git status` through `info/exclude` — the
/// repo's local, untracked ignore file — so no tracked file is edited.
pub fn ensure_excluded(root: &Path) -> Result<()> {
    let common = git::check(root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let exclude = Path::new(&common).join("info").join("exclude");
    let text = std::fs::read_to_string(&exclude).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let before = lines.len();
    for pat in [format!("{}/", crate::paths::ORCH_DIR), format!("{TASK_DIR}/")] {
        if !lines.contains(&pat) {
            lines.push(pat);
        }
    }
    if lines.len() == before {
        return Ok(());
    }
    if let Some(parent) = exclude.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io(&format!("creating {}", parent.display()), e))?;
    }
    std::fs::write(&exclude, lines.join("\n") + "\n")
        .map_err(|e| io(&format!("writing {}", exclude.display()), e))
}

// ---------------------------------------------------------------- the run's operations

fn settings_of(store: &Store) -> Result<RunSettings> {
    store.run_settings()?.ok_or_else(|| {
        die("no run in .orchestrate/state.db — run `orch sync` after writing plan.md")
    })
}

/// The run's coders, in plan order.
fn worker_names(store: &Store) -> Result<Vec<String>> {
    Ok(store
        .load_run()?
        .map(|r| {
            r.agents
                .into_iter()
                .filter(|a| a.role == AgentRole::Worker)
                .map(|a| a.name)
                .collect()
        })
        .unwrap_or_default())
}

/// The branch a run starts from when none was recorded: origin's default branch, else `main`,
/// else `master`.
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
    Err(die(format!(
        "can't tell which branch {branch} starts from — add `Base: <branch>` to the plan header and run `orch sync`"
    )))
}

/// What [`start`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartReport {
    pub branch: String,
    pub base: String,
    /// Worktrees created by this call.
    pub created: Vec<PathBuf>,
    /// Every coder and the folder it codes in.
    pub workers: Vec<(String, PathBuf)>,
}

/// `orch start`: put the main folder on the feature branch (created from the base, or from the
/// current branch, which is then recorded as the base), keep orch's folders out of git status,
/// create a detached worktree for each coder, and note the main folder's setup state. Safe to
/// run twice. Touches no remote.
pub fn start(store: &mut Store, root: &Path) -> Result<StartReport> {
    let s = settings_of(store)?;
    ensure_excluded(root)?;
    let cur = git::current_branch(root)?;
    let mut base = s.base.clone().filter(|b| *b != s.branch);
    if cur != s.branch {
        if git::is_dirty(root)? {
            return Err(die("main working tree has uncommitted changes — commit or stash first"));
        }
        if base.is_none() && cur != "HEAD" {
            base = Some(cur);
        }
        if git::branch_exists(root, &s.branch)? {
            git::check(root, &["switch", &s.branch])?;
        } else {
            let from = match base.clone() {
                Some(b) => b,
                None => default_base(root, &s.branch)?,
            };
            git::check(root, &["switch", "-c", &s.branch, &from])?;
            base.get_or_insert(from);
        }
    }
    let base = match base {
        Some(b) => b,
        None => default_base(root, &s.branch)?,
    };
    store.set_run_base(&base)?;
    let mut created = Vec::new();
    let mut workers = Vec::new();
    for name in worker_names(store)? {
        let dir = worker_dir(root, &s, &name);
        if s.isolation == "worktree"
            && let Some(p) = create(root, &name, &s.branch)?
        {
            store.forget_setup(&p)?;
            created.push(p);
        }
        workers.push((name, dir));
    }
    if has_setup(&s) && store.setup_fingerprint(root)?.is_none() {
        // the developer's main folder is already set up for this branch
        setup::mark_setup_done(store, &s, root)?;
    }
    Ok(StartReport {
        branch: s.branch,
        base,
        created,
        workers,
    })
}

/// Whose uncommitted work a dirty worker folder holds.
enum Owner {
    /// The milestone being prepared.
    Mine,
    /// Another milestone (named).
    Other(String),
    /// Nobody can be told.
    Nobody,
}

fn terminal(s: State) -> bool {
    matches!(s, State::Merged | State::Skipped)
}

fn classify_dirty(
    store: &Store,
    root: &Path,
    branch: &str,
    task: &str,
    worker: &str,
    dir: &Path,
) -> Result<Owner> {
    let tasks = store.list_tasks()?;
    // the coder is on another milestone right now
    if let Some(t) = tasks
        .iter()
        .find(|t| t.id != task && t.worker.as_deref() == Some(worker) && is_active(t.status))
    {
        return Ok(Owner::Other(t.id.clone()));
    }
    // the folder sits on (or past) a milestone's saved work
    let mut sits_on = Vec::new();
    for h in refs::list(root, branch)? {
        if git::run(dir, &["merge-base", "--is-ancestor", &h.sha, "HEAD"])?.success() {
            sits_on.push(h.task);
        }
    }
    if sits_on.iter().any(|t| t == task) {
        return Ok(Owner::Mine);
    }
    if let Some(o) = sits_on.into_iter().next() {
        return Ok(Owner::Other(o));
    }
    // the coder last worked on one
    let was_here = |t: &crate::task::Task| {
        t.last_worker.as_deref() == Some(worker) || t.worker.as_deref() == Some(worker)
    };
    if tasks.iter().any(|t| t.id == task && was_here(t)) {
        return Ok(Owner::Mine);
    }
    if let Some(t) = tasks.iter().find(|t| t.id != task && !terminal(t.status) && was_here(t)) {
        return Ok(Owner::Other(t.id.clone()));
    }
    Ok(Owner::Nobody)
}

/// Where [`prepare_workdir`] left a coder's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub dir: PathBuf,
    /// Things worth telling the caller (a saved-work ref whose object is gone, work saved
    /// before the reset).
    pub notes: Vec<String>,
}

/// Put `worker`'s folder on the right starting point for `task`: the milestone's hidden ref when
/// it has saved work, else the feature branch tip (worktree mode); or restore the milestone's
/// parked work into the clean main folder (shared mode).
///
/// Uncommitted changes are never wiped: when they belong to `task` they are saved to its hidden
/// ref first; when they belong to another milestone, or to nobody, this refuses and says which.
pub fn prepare_workdir(
    store: &mut Store,
    root: &Path,
    task: &str,
    worker: &str,
) -> Result<Prepared> {
    let s = settings_of(store)?;
    let tref = task_ref(&s.branch, task);
    let mut notes = Vec::new();
    let state = refs::read_state(root, &tref)?;
    if let RefState::Broken(sha) = &state {
        notes.push(format!(
            "WARN {tref} points at {sha}, which is gone: {task}'s saved work is lost; starting from {}",
            s.branch
        ));
    }
    let dir = worker_dir(root, &s, worker);
    if s.isolation == "worktree" {
        if !dir.exists() {
            create(root, worker, &s.branch)?;
            store.forget_setup(&dir)?;
        }
        if git::is_dirty(&dir)? {
            match classify_dirty(store, root, &s.branch, task, worker, &dir)? {
                Owner::Mine => {
                    let msg = format!("orch wip {task}: saved before restarting the milestone");
                    if let Saved::Saved(sha) =
                        refs::save_tree(&dir, root, &s.branch, &tref, &msg, SaveMode::Worktree)?
                    {
                        notes.push(format!("SAVED {task}'s uncommitted work in {} to {tref} ({})", dir.display(), &sha[..sha.len().min(10)]));
                    }
                }
                Owner::Other(o) => {
                    return Err(die(format!(
                        "{} has uncommitted changes that belong to {o} — park {o} first (`orch park {o} paused`) so they are saved, or clean that worker",
                        dir.display()
                    )));
                }
                Owner::Nobody => {
                    return Err(die(format!(
                        "{} has uncommitted changes that no milestone owns — commit, stash or discard them first",
                        dir.display()
                    )));
                }
            }
        }
        let target = refs::read(root, &tref)?.unwrap_or_else(|| s.branch.clone());
        git::check(&dir, &["switch", "-q", "--detach", &target])?;
    } else {
        if git::current_branch(root)? != s.branch {
            return Err(die(format!("main tree is not on {}", s.branch)));
        }
        let dirty = git::is_dirty(root)?;
        if matches!(state, RefState::Valid(_)) {
            if dirty {
                return Err(die(format!(
                    "main tree has uncommitted changes, so {task}'s parked work ({tref}) can't be restored — finish or discard them first"
                )));
            }
            refs::restore_shared(root, &tref, task)?;
        } else if dirty {
            let t = store.get_task(task)?;
            let holds = t.worker.is_some() && is_active(t.status);
            if t.status == State::Pending && !holds {
                return Err(die(
                    "main tree has uncommitted changes that no milestone owns — commit, stash or discard them first",
                ));
            }
        }
    }
    Ok(Prepared { dir, notes })
}

/// Save a milestone's uncommitted work to its hidden ref before its coder is freed: the coder's
/// worktree (worktree mode) or the main folder (shared mode). Works whether or not the milestone
/// is still held by its coder, and never discards anything: on an error the work is still in the
/// folder and the error says so. A folder whose coder now holds a different milestone is left
/// alone — that work is not this milestone's.
pub fn save_task_work(store: &Store, root: &Path, task: &str, why: &str) -> Result<Saved> {
    let s = settings_of(store)?;
    let t = store.get_task(task)?;
    let tasks = store.list_tasks()?;
    let msg = format!("orch wip {task}: parked — {why}");
    let tref = task_ref(&s.branch, task);
    if s.isolation == "worktree" {
        let Some(worker) = t.worker.clone().or(t.last_worker.clone()) else {
            return Ok(Saved::Nothing);
        };
        let dir = worktree_path(root, &worker);
        let other_holds = tasks
            .iter()
            .any(|o| o.id != task && o.worker.as_deref() == Some(&worker) && is_active(o.status));
        if !dir.exists() || other_holds {
            return Ok(Saved::Nothing);
        }
        refs::save_tree(&dir, root, &s.branch, &tref, &msg, SaveMode::Worktree)
    } else {
        let other_holds = tasks
            .iter()
            .any(|o| o.id != task && o.worker.is_some() && is_active(o.status));
        if other_holds {
            return Ok(Saved::Nothing);
        }
        refs::save_tree(root, root, &s.branch, &tref, &msg, SaveMode::Shared)
    }
}

/// One line of [`cleanup`], worded as Python v4.10.1 prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupLine {
    Removed(PathBuf),
    Skipped(PathBuf),
}

impl std::fmt::Display for CleanupLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CleanupLine::Removed(p) => write!(f, "REMOVED {}", p.display()),
            CleanupLine::Skipped(p) => write!(f, "SKIP {}: has uncommitted changes", p.display()),
        }
    }
}

/// `orch cleanup`: remove every clean worker worktree; one with unsaved changes stays. Returns
/// `None` in shared mode (nothing to clean).
pub fn cleanup(store: &mut Store, root: &Path) -> Result<Option<Vec<CleanupLine>>> {
    let s = settings_of(store)?;
    if s.isolation != "worktree" {
        return Ok(None);
    }
    let mut lines = Vec::new();
    for name in worker_names(store)? {
        match remove(root, &name)? {
            Removal::Absent => {}
            Removal::Removed(p) => {
                store.forget_setup(&p)?;
                lines.push(CleanupLine::Removed(p));
            }
            Removal::Dirty(p) => lines.push(CleanupLine::Skipped(p)),
        }
    }
    prune(root)?;
    Ok(Some(lines))
}
