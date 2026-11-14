//! `orch start`, `prepare`, `setup-done`, `park` and `cleanup`: the git side of a run, with the
//! arguments, output lines and exit codes of Python v4.10.1 (`2` for every refusal). The git work
//! lives in `orch_core::{worktree, refs, setup}`; this file parses arguments, prints, and moves the
//! milestone's state through the store. Nothing here pushes or touches a remote.

use orch_core::claim::ClaimError;
use orch_core::paths;
use orch_core::plan_edit::py_repr;
use orch_core::reconcile::park_with_lost;
use orch_core::run_store::{AgentRole, Run, is_active};
use orch_core::setup::{self, SetupNeed};
use orch_core::store::{DB_FILE, Store};
use orch_core::task::State;
use orch_core::worktree::{self, TASK_DIR};
use orch_herdr::RuntimeAdapter;
use std::path::PathBuf;

const NO_RUN: &str = "no run in .orchestrate/state.db — run `orch sync` after writing plan.md";

fn die(msg: impl std::fmt::Display) -> i32 {
    eprintln!("ERROR: {msg}");
    2
}

fn fail(e: orch_core::Error) -> i32 {
    eprintln!("ERROR: {e}");
    e.exit_code()
}

struct Ctx {
    root: PathBuf,
    store: Store,
    run: Run,
}

fn open() -> Result<Ctx, i32> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let root = paths::main_root(&cwd).map_err(fail)?;
    let dir = root.join(paths::ORCH_DIR);
    if !dir.join(DB_FILE).exists() {
        return Err(die(NO_RUN));
    }
    let store = Store::open_in(&dir).map_err(die)?;
    match store.load_run().map_err(die)? {
        Some(run) => Ok(Ctx { root, store, run }),
        None => Err(die(NO_RUN)),
    }
}

fn known_task(ctx: &Ctx, tid: &str) -> Result<orch_core::task::Task, i32> {
    ctx.store.get_task(tid).map_err(|_| {
        let known: Vec<&str> = ctx.run.plan.iter().map(|p| p.id.as_str()).collect();
        die(format!(
            "unknown task {} — known: {}",
            py_repr(tid),
            if known.is_empty() { "none".to_string() } else { known.join(", ") }
        ))
    })
}

fn need(args: &[String], n: usize, usage: &str) -> Result<(), i32> {
    if args.len() < n {
        Err(die(format!("usage: orch {usage}")))
    } else {
        Ok(())
    }
}

/// How herdr sees one agent, worded as Python's `start` prints it.
fn herdr_state(rt: &dyn RuntimeAdapter, name: &str, pane: Option<&str>, detect: bool) -> String {
    if let Some(p) = pane {
        return if rt.pane(p).is_ok() {
            format!("pane {p} detect:off")
        } else {
            format!("STALE pane {p} — find its current id with `herdr pane list` and fix plan.md, then `orch sync`")
        };
    }
    if !detect {
        return "detect:off (relying on its done file)".into();
    }
    if rt.agent(name).is_ok() { "found" } else { "NOT FOUND" }.into()
}

/// `orch start`.
pub fn start(_args: &[String], rt: &dyn RuntimeAdapter) -> i32 {
    let mut ctx = match open() {
        Ok(c) => c,
        Err(c) => return c,
    };
    let report = match worktree::start(&mut ctx.store, &ctx.root) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    for p in &report.created {
        println!("CREATED worktree {}", p.display());
    }
    println!("BRANCH {} (base {})", report.branch, report.base);
    for (name, dir) in &report.workers {
        let agent = ctx
            .run
            .agents
            .iter()
            .find(|a| a.role == AgentRole::Worker && &a.name == name);
        let (kind, pane, detect) = agent.map_or(("", None, true), |a| (a.kind.as_str(), a.pane.as_deref(), a.detect));
        println!(
            "WORKER {name} kind={kind} dir={} herdr={}",
            dir.display(),
            herdr_state(rt, name, pane, detect)
        );
    }
    if let Some(r) = ctx.run.agents.iter().find(|a| a.role == AgentRole::Reviewer) {
        println!(
            "REVIEWER {} kind={} herdr={}",
            r.name,
            r.kind,
            herdr_state(rt, &r.name, r.pane.as_deref(), r.detect)
        );
    }
    0
}

/// `orch prepare <Mn> <worker>`: reserve the worker and put its folder on the milestone's saved
/// work (or the feature branch tip).
pub fn prepare(args: &[String]) -> i32 {
    if let Err(c) = need(args, 2, "prepare <Mn> <worker>") {
        return c;
    }
    let (tid, worker) = (args[0].as_str(), args[1].as_str());
    let mut ctx = match open() {
        Ok(c) => c,
        Err(c) => return c,
    };
    let t = match known_task(&ctx, tid) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let s = ctx.run.settings.clone();
    if !ctx.run.agents.iter().any(|a| a.role == AgentRole::Worker && a.name == worker) {
        return die(format!("unknown worker {worker}"));
    }
    if !matches!(t.status, State::Pending | State::Paused | State::Conflict | State::NeedsFix) {
        return die(format!(
            "{tid} is {}, expected pending/paused/needs-fix/conflict",
            t.status
        ));
    }
    let tasks = match ctx.store.list_tasks() {
        Ok(t) => t,
        Err(e) => return die(e),
    };
    if let Some(other) = tasks
        .iter()
        .find(|o| o.id != tid && o.worker.as_deref() == Some(worker) && is_active(o.status))
    {
        return die(format!("{worker} is busy with {}", other.id));
    }
    let holds = t.worker.is_some() && is_active(t.status);
    let busy = if s.isolation == "worktree" {
        let mut w: Vec<&str> = tasks
            .iter()
            .filter(|o| is_active(o.status))
            .filter_map(|o| o.worker.as_deref())
            .collect();
        w.sort_unstable();
        w.dedup();
        w.len() as i64
    } else {
        tasks.iter().filter(|o| is_active(o.status)).count() as i64
    };
    let fix_in_shared = s.isolation == "shared" && matches!(t.status, State::Conflict | State::NeedsFix);
    if !holds && s.max_parallel - busy <= 0 && !fix_in_shared {
        return die("no free slot (max parallel reached)");
    }
    let prepared = match worktree::prepare_workdir(&mut ctx.store, &ctx.root, tid, worker) {
        Ok(p) => p,
        Err(e) => return fail(e),
    };
    for n in &prepared.notes {
        eprintln!("{n}");
    }
    let taskdir = prepared.dir.join(TASK_DIR).join(tid);
    if let Err(e) = std::fs::create_dir_all(&taskdir) {
        return die(format!("creating {}: {e}", taskdir.display()));
    }
    match ctx.store.claim(tid, worker, None) {
        Ok(()) => {}
        Err(e @ (ClaimError::NotClaimable { .. } | ClaimError::WorkerBusy { .. })) => return die(e),
        Err(e) => return die(e),
    }
    println!("DIR {}", prepared.dir.display());
    println!("TASKDIR {}", taskdir.display());
    if s.isolation == "worktree" {
        match setup::setup_need(&ctx.store, &s, &prepared.dir) {
            Ok(SetupNeed::No) => {}
            Ok(need) => println!(
                "SETUP_NEEDED run `{}` in {}, then `orch setup-done {worker}`{}",
                s.setup_command,
                prepared.dir.display(),
                if need == SetupNeed::Changed {
                    " (dependency files changed since its last setup)"
                } else {
                    ""
                }
            ),
            Err(e) => return fail(e),
        }
    }
    0
}

/// `orch setup-done <worker>`: record the worker folder's dependency fingerprint.
pub fn setup_done(args: &[String]) -> i32 {
    if let Err(c) = need(args, 1, "setup-done <worker>") {
        return c;
    }
    let mut ctx = match open() {
        Ok(c) => c,
        Err(c) => return c,
    };
    let worker = args[0].as_str();
    if !ctx.run.agents.iter().any(|a| a.role == AgentRole::Worker && a.name == worker) {
        return die(format!("unknown worker {worker}"));
    }
    let dir = worktree::worker_dir(&ctx.root, &ctx.run.settings, worker);
    if !dir.exists() {
        return die(format!("{} does not exist — run `orch start` first", dir.display()));
    }
    match setup::mark_setup_done(&mut ctx.store, &ctx.run.settings, &dir) {
        Ok(()) => {
            println!("OK");
            0
        }
        Err(e) => fail(e),
    }
}

/// `orch park <Mn> paused|failed|pending [--lost] [reason...]`: save the milestone's work to its
/// hidden ref, free its coder, set the state. When the save fails nothing changes and the work is
/// still in its folder.
pub fn park(args: &[String]) -> i32 {
    let lost = args.iter().any(|a| a == "--lost");
    let args: Vec<String> = args.iter().filter(|a| *a != "--lost").cloned().collect();
    if let Err(c) = need(&args, 2, "park <Mn> paused|failed|pending [--lost] [reason]") {
        return c;
    }
    let (tid, status, reason) = (args[0].as_str(), args[1].as_str(), args[2..].join(" "));
    let to = match status {
        "paused" => State::Paused,
        "failed" => State::Failed,
        "pending" => State::Pending,
        _ => return die("status must be paused, failed or pending"),
    };
    let mut ctx = match open() {
        Ok(c) => c,
        Err(c) => return c,
    };
    if let Err(c) = known_task(&ctx, tid) {
        return c;
    }
    let why = if reason.is_empty() { status.to_string() } else { reason.clone() };
    // save first: if this fails the milestone keeps its coder and its state, and so does the work
    if let Err(e) = worktree::save_task_work(&ctx.store, &ctx.root, tid, &why) {
        return die(format!("{tid} was NOT parked: {e}"));
    }
    let max_lost = orch_config::tunables::Tunables::from_env().map_or(4, |t| t.max_lost);
    let (final_status, final_reason) =
        match park_with_lost(&mut ctx.store, None, max_lost, tid, to, &reason, lost) {
            Ok(s) => {
                let r = ctx.store.get_task(tid).map(|t| t.reason).unwrap_or(reason);
                (s, r)
            }
            Err(e) => return die(e),
        };
    println!("PARKED {tid} as {final_status}: {final_reason}");
    0
}

/// `orch cleanup`: remove the clean worker worktrees; one with unsaved changes stays.
pub fn cleanup(_args: &[String]) -> i32 {
    let mut ctx = match open() {
        Ok(c) => c,
        Err(c) => return c,
    };
    match worktree::cleanup(&mut ctx.store, &ctx.root) {
        Ok(None) => {
            println!("nothing to clean (shared mode)");
            0
        }
        Ok(Some(lines)) => {
            for l in lines {
                println!("{l}");
            }
            0
        }
        Err(e) => fail(e),
    }
}

