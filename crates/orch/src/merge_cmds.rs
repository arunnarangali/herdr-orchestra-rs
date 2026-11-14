//! `orch commit` and `orch merge`: save a milestone's work, then squash it onto the feature branch
//! only after the plan's check passes on the merged result. Output lines and exit codes are
//! Python v4.10.1's (`4` conflict, `6` check fails on the branch, `7` held for approval, `3` cap
//! or a merge already running, `2` refusal). The git work and the crash-safe journal live in
//! `orch_core::merge`; this file moves the milestone's state according to what happened.

use orch_core::manager::MAX_FIX;
use orch_core::merge::{self, FAULT_EXIT, Fault, Landed, Landing};
use orch_core::paths;
use orch_core::plan_edit::py_repr;
use orch_core::refs::{self, task_ref};
use orch_core::run_store::{Run, RunSettings, is_active};
use orch_core::store::{Counter, DB_FILE, Store, TransitionRequest};
use orch_core::task::{State, Task};
use std::path::PathBuf;

const NO_RUN: &str = "no run in .orchestrate/state.db — run `orch sync` after writing plan.md";

fn die(msg: impl std::fmt::Display) -> i32 {
    eprintln!("ERROR: {msg}");
    2
}

/// Report an error with its own exit code; an injected fault ends the process like a crash.
fn fail(e: orch_core::Error) -> i32 {
    let code = e.exit_code();
    if code == FAULT_EXIT {
        std::process::exit(FAULT_EXIT);
    }
    eprintln!("ERROR: {e}");
    code
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

fn known_task(ctx: &Ctx, tid: &str) -> Result<Task, i32> {
    ctx.store.get_task(tid).map_err(|_| {
        let known: Vec<&str> = ctx.run.plan.iter().map(|p| p.id.as_str()).collect();
        die(format!(
            "unknown task {} — known: {}",
            py_repr(tid),
            if known.is_empty() { "none".to_string() } else { known.join(", ") }
        ))
    })
}

fn need(args: &[String], usage: &str) -> Result<(), i32> {
    if args.is_empty() {
        Err(die(format!("usage: orch {usage}")))
    } else {
        Ok(())
    }
}

/// Before `orch start` touches the branch: finish or undo a merge a crash interrupted (a half-done
/// merge leaves the main folder dirty, which `start` would refuse). Silent without a run.
/// `Some(code)` when recovery itself failed.
pub fn recover_on_start() -> Option<i32> {
    let cwd = std::env::current_dir().ok()?;
    let root = paths::main_root(&cwd).ok()?;
    if !root.join(paths::ORCH_DIR).join(DB_FILE).exists() {
        return None;
    }
    let mut store = Store::open_in(&root.join(paths::ORCH_DIR)).ok()?;
    store.run_settings().ok().flatten()?;
    match merge::recover(&mut store, &root) {
        Ok(Some(r)) => {
            println!("{r}");
            None
        }
        Ok(None) => None,
        Err(e) => Some(fail(e)),
    }
}

fn who(t: &Task) -> String {
    match &t.worker {
        Some(w) if is_active(t.status) => w.clone(),
        _ => "any free coder".into(),
    }
}

/// `HELD`: stop the milestone until `orch approve` (exit 7).
fn held(ctx: &mut Ctx, t: &Task, risks: &[String]) -> i32 {
    let tid = &t.id;
    if ctx.run.settings.isolation == "worktree" && t.worker.is_some() {
        let _ = ctx.store.release_claim(tid); // its work is saved in the hidden ref: free the coder
    }
    let reason = format!(
        "waiting for `orch approve {tid}`: {}{}",
        risks.iter().take(3).cloned().collect::<Vec<_>>().join("; "),
        if risks.len() > 3 { format!(" (+{} more)", risks.len() - 3) } else { String::new() }
    );
    if let Err(e) = ctx.store.transition(tid, &TransitionRequest::auto(State::Held, Some(&reason))) {
        return die(e);
    }
    println!("HELD {tid} for approval — {}", risks.join("; "));
    println!("Look at it, then run `orch approve {tid}` (or `orch set {tid} paused` to stop it)");
    7
}

/// Act on what landing came to: print Python's lines, move the state, return the exit code.
fn report(ctx: &mut Ctx, tid: &str, landing: Landing) -> i32 {
    if let Some(r) = &landing.recovered {
        println!("{r}");
    }
    let s: &RunSettings = &ctx.run.settings;
    let branch = s.branch.clone();
    let Ok(t) = ctx.store.get_task(tid) else {
        return die(format!("unknown task {}", py_repr(tid)));
    };
    match landing.outcome {
        Landed::Merged { empty, shared } => {
            if empty && !shared {
                println!("NOTE {tid} had no changes to merge");
            }
            if shared {
                println!("MERGED {tid} (shared mode: committed on {branch})");
            } else {
                println!("MERGED {tid} into {branch}");
            }
            0
        }
        Landed::Held { risks } => held(ctx, &t, &risks),
        Landed::Conflict { files } => {
            let _ = ctx.store.bump(tid, Counter::Conflict);
            let reason = format!(
                "merge conflict in {}",
                if files.is_empty() { "(see git status)".to_string() } else { files.join(", ") }
            );
            if let Err(e) = ctx.store.transition(tid, &TransitionRequest::auto(State::Conflict, Some(&reason))) {
                return die(e);
            }
            println!(
                "CONFLICT {tid} files={} -> conflict job queued for {} (`orch send-job {tid}`)",
                files.join(","),
                who(&t)
            );
            4
        }
        Landed::Unresolved { files } => {
            let reason = format!(
                "merge conflict in {} still unresolved after {} attempts",
                files.join(", "),
                orch_core::manager::MAX_CONFLICT
            );
            let _ = ctx.store.release_claim(tid);
            if let Err(e) = ctx.store.transition(tid, &TransitionRequest::auto(State::Failed, Some(&reason))) {
                return die(e);
            }
            println!("CAP {tid}: {reason}");
            3
        }
        Landed::BranchFails { tail } => {
            let n = match ctx.store.bump(tid, Counter::Fix) {
                Ok(n) => n,
                Err(e) => return die(e),
            };
            if n > MAX_FIX {
                let reason = format!("passes alone but fails the check on {branch} after {MAX_FIX} fix rounds");
                let _ = ctx.store.release_claim(tid);
                if let Err(e) = ctx.store.transition(tid, &TransitionRequest::auto(State::Failed, Some(&reason))) {
                    return die(e);
                }
                println!("CAP {tid}: still fails the check on {branch} after {MAX_FIX} fix rounds\n{tail}");
                return 3;
            }
            let reason = format!("fails the check on {branch} once merged (round {n})");
            if let Err(e) = ctx.store.transition(tid, &TransitionRequest::auto(State::NeedsFix, Some(&reason))) {
                return die(e);
            }
            println!(
                "CHECK_FAILED {tid} on {branch} -> fix job queued for {} (`orch send-job {tid}`); nothing was committed\n{tail}",
                who(&t)
            );
            6
        }
    }
}

/// `orch commit <Mn>`: keep the milestone's work in its hidden ref and mark it committed (worktree
/// mode); in shared mode commit it in place.
pub fn commit(args: &[String]) -> i32 {
    if let Err(c) = need(args, "commit <Mn>") {
        return c;
    }
    let tid = args[0].as_str();
    let mut ctx = match open() {
        Ok(c) => c,
        Err(c) => return c,
    };
    let t = match known_task(&ctx, tid) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let s = ctx.run.settings.clone();
    if s.isolation == "shared" {
        return match merge::land(&mut ctx.store, &ctx.root, tid, &Fault::from_env()) {
            Ok(l) => report(&mut ctx, tid, l),
            Err(e) => fail(e),
        };
    }
    let tref = task_ref(&s.branch, tid);
    let holds = t.worker.is_some() && is_active(t.status);
    if !holds {
        match refs::read(&ctx.root, &tref) {
            Ok(Some(_)) => {}
            Ok(None) => return die(format!("{tid} has no saved work ({tref})")),
            Err(e) => return fail(e),
        }
    } else if let Err(e) = merge::saved_work(&ctx.store, &ctx.root, &s, &t) {
        return fail(e);
    }
    if let Err(e) = ctx.store.transition(tid, &TransitionRequest::auto(State::Committed, None)) {
        return die(e);
    }
    if holds {
        println!("COMMITTED {tid} (saved in {tref})");
    } else {
        println!("COMMITTED {tid} (saved in {tref} since handoff)");
    }
    0
}

/// `orch merge <Mn>`: squash the committed milestone onto the feature branch, check, commit only
/// when green.
pub fn merge(args: &[String]) -> i32 {
    if let Err(c) = need(args, "merge <Mn>") {
        return c;
    }
    let tid = args[0].as_str();
    let mut ctx = match open() {
        Ok(c) => c,
        Err(c) => return c,
    };
    let t = match known_task(&ctx, tid) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let s = ctx.run.settings.clone();
    if s.isolation == "shared" {
        return die("shared mode has no merge step (orch commit already landed it)");
    }
    if t.status != State::Committed {
        return die(format!("{tid} is {}, expected committed", t.status));
    }
    let tref = task_ref(&s.branch, tid);
    match refs::read(&ctx.root, &tref) {
        Ok(Some(_)) => {}
        Ok(None) => {
            return die(format!("{tid} has no saved work ({tref}) — run `orch commit {tid}` first"));
        }
        Err(e) => return fail(e),
    }
    match merge::land(&mut ctx.store, &ctx.root, tid, &Fault::from_env()) {
        Ok(l) => report(&mut ctx, tid, l),
        Err(e) => fail(e),
    }
}
