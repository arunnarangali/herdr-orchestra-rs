//! `orch send-job`, `orch handoff` and `orch needs-fix` (Python v4.10.1 `cmd_send_job`,
//! `cmd_handoff`, `cmd_needs_fix`): the manual levers over a milestone's queued job and its
//! coder, with Python's arguments, messages and exit codes. `args` are the words after the
//! command; each handler returns the exit code.
//!
//! A queued job lives in `.orchestrate/driver.json`, which the running event manager owns and
//! rewrites every cycle, so `send-job` and `needs-fix` refuse while `orch autopilot` runs (it
//! sends and queues these itself) rather than lose the job to a race.

use crate::listing::die;
use orch_config::tunables::Tunables;
use orch_core::conflict::{self, IssueKind};
use orch_core::fix::{self, FixContext, FixKind};
use orch_core::lease::{driver_alive, driver_lock_path};
use orch_core::manager::{DriverState, QueuedJob, coder_prompt};
use orch_core::paths::{self, ORCH_DIR};
use orch_core::plan_edit::py_repr;
use orch_core::refs::task_ref;
use orch_core::run_store::Run;
use orch_core::store::{Counter, DeliveryKind, Store, TransitionRequest};
use orch_core::task::{State, Task};
use orch_core::worktree;
use orch_herdr::cli::CliHerdr;
use orch_herdr::delivery::{Deliverer, NewDelivery, SendOutcome};
use std::collections::HashSet;
use std::path::PathBuf;

const NO_RUN: &str = "no run in .orchestrate/state.db — run `orch sync` after writing plan.md";
/// Python's exit code for a prompt that did not reach the coder.
const EXIT_DELIVERY: i32 = 5;

struct Ctx {
    root: PathBuf,
    orch: PathBuf,
    store: Store,
    run: Run,
}

fn open() -> Result<Ctx, i32> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let root = paths::main_root(&cwd).map_err(die)?;
    let orch = root.join(ORCH_DIR);
    if !orch.join(orch_core::store::DB_FILE).exists() {
        return Err(die(NO_RUN));
    }
    let store = Store::open_in(&orch).map_err(die)?;
    match store.load_run().map_err(die)? {
        Some(run) => Ok(Ctx { root, orch, store, run }),
        None => Err(die(NO_RUN)),
    }
}

/// Python `task(st, tid)`.
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

fn usage(args: &[String], what: &str) -> Result<(), i32> {
    if args.is_empty() { Err(die(format!("usage: orch {what}"))) } else { Ok(()) }
}

fn refuse_while_driving(ctx: &Ctx, what: &str) -> Result<(), i32> {
    if driver_alive(&driver_lock_path(&ctx.orch)) {
        Err(die(format!("`orch autopilot` is running — it {what} itself; stop it first to do this by hand")))
    } else {
        Ok(())
    }
}

/// The milestone's queued job: one the event manager kept in `driver.json`, else the landing
/// problem `orch merge` stored (a conflict, or a check that fails on the run branch).
fn queued_job(ctx: &Ctx, state: &DriverState, t: &Task) -> Option<QueuedJob> {
    if let Some(j) = state.jobs.iter().find(|j| j.task == t.id) {
        return Some(j.clone());
    }
    let issue = ctx.store.issue(&t.id).ok().flatten()?;
    let s = &ctx.run.settings;
    Some(match issue.kind {
        IssueKind::Conflict => QueuedJob {
            task: t.id.clone(),
            file: format!("conflict-{}.md", t.conflict_rounds.max(1)),
            body: conflict::job_body(&t.id, &t.title, &s.branch, &s.check_command, &issue.files),
        },
        IssueKind::Check => {
            let round = t.fix_rounds.max(1);
            let result_file = format!("result-{}.json", t.attempt + 1);
            QueuedJob {
                task: t.id.clone(),
                file: format!("fix-{round}.md"),
                body: fix::body(
                    &FixKind::Check { tail: issue.detail },
                    &FixContext { task: &t.id, title: &t.title, check: &s.check_command, round, result_file: &result_file },
                ),
            }
        }
    })
}

/// `orch send-job Mn`: write the queued fix/conflict job into the coder's folder and send it.
pub fn send_job(args: &[String]) -> i32 {
    run_send_job(args).unwrap_or_else(|c| c)
}

fn run_send_job(args: &[String]) -> Result<i32, i32> {
    usage(args, "send-job <Mn>")?;
    let tid = &args[0];
    let mut ctx = open()?;
    let t = known_task(&ctx, tid)?;
    let mut state = DriverState::load(&ctx.orch);
    let Some(job) = queued_job(&ctx, &state, &t) else {
        return Err(die(format!("{tid} has no queued job")));
    };
    let Some(worker) = t.worker.clone() else {
        return Err(die(format!(
            "{tid} has no coder yet — `orch pick {tid}` then `orch prepare {tid} <worker>` first"
        )));
    };
    refuse_while_driving(&ctx, "sends queued jobs")?;
    let dir = worktree::worker_dir(&ctx.root, &ctx.run.settings, &worker).join(".orch-task").join(tid);
    std::fs::create_dir_all(&dir).map_err(|e| die(format!("creating {}: {e}", dir.display())))?;
    std::fs::write(dir.join(&job.file), &job.body)
        .map_err(|e| die(format!("writing {}: {e}", dir.join(&job.file).display())))?;

    // record the intent first (the delivery is durable before anything is sent)
    let attempt = ctx.store.bump(tid, Counter::Attempt).map_err(die)?;
    ctx.store
        .transition(tid, &TransitionRequest::auto(State::Coding, Some(&format!("sent {}", job.file))))
        .map_err(die)?;
    let tun = Tunables::from_env().map_err(|_| die("could not read the tunables from the environment"))?;
    let herdr = CliHerdr::from_tunables(&tun);
    let watching: HashSet<String> =
        ctx.run.agents.iter().filter(|a| a.detect).map(|a| a.name.clone()).collect();
    let watchable = move |n: &str| watching.contains(n);
    let prompt = coder_prompt(tid, &job.file, attempt);
    let sent = Deliverer::new(&mut ctx.store, &herdr, &watchable)
        .send(NewDelivery { agent: &worker, task: tid, attempt, kind: DeliveryKind::Coder, prompt: &prompt })
        .map_err(die)?;
    match sent {
        SendOutcome::Sent { .. } => {
            state.jobs.retain(|j| !(j.task == *tid && j.file == job.file));
            state.save(&ctx.orch).map_err(die)?;
            println!("SENT {tid} -> {worker} (attempt {attempt})");
            Ok(0)
        }
        SendOutcome::Failed { error, .. } => {
            // the job stays queued; the milestone goes back to waiting for it
            let _ = ctx.store.transition(
                tid,
                &TransitionRequest::auto(State::NeedsFix, Some(&format!("could not prompt {worker}"))),
            );
            println!("DELIVERY_FAILED {tid} -> {worker}: {error}");
            Ok(EXIT_DELIVERY)
        }
    }
}

/// `orch handoff Mn`: (worktree mode) save the milestone to its hidden ref and free the coder
/// so it can take other work while the reviewer reviews.
pub fn handoff(args: &[String]) -> i32 {
    run_handoff(args).unwrap_or_else(|c| c)
}

fn run_handoff(args: &[String]) -> Result<i32, i32> {
    usage(args, "handoff <Mn>")?;
    let tid = &args[0];
    let mut ctx = open()?;
    let t = known_task(&ctx, tid)?;
    if ctx.run.settings.isolation != "worktree" {
        println!("SKIP shared mode keeps the coder until the task lands");
        return Ok(0);
    }
    let Some(worker) = t.worker.clone() else {
        return Err(die(format!("{tid} does not hold a coder")));
    };
    worktree::save_task_work(&ctx.store, &ctx.root, tid, "for review").map_err(die)?;
    ctx.store.release_claim(tid).map_err(die)?;
    println!(
        "HANDED_OFF {tid}: saved to {}; {worker} is free",
        task_ref(&ctx.run.settings.branch, tid)
    );
    Ok(0)
}

/// `orch needs-fix Mn`: queue the latest review's BLOCKER lines as a fix job for any free coder.
pub fn needs_fix(args: &[String]) -> i32 {
    run_needs_fix(args).unwrap_or_else(|c| c)
}

fn run_needs_fix(args: &[String]) -> Result<i32, i32> {
    usage(args, "needs-fix <Mn>")?;
    let tid = &args[0];
    let mut ctx = open()?;
    let t = known_task(&ctx, tid)?;
    let k = t.review_rounds;
    let (blockers, _, _) = fix::review_findings(&ctx.orch, tid, k);
    if blockers.is_empty() {
        return Err(die(format!(
            "no blockers in review round {k} of {tid} (.orchestrate/review/{tid}/review-{k}.json or .md)"
        )));
    }
    refuse_while_driving(&ctx, "queues review fixes")?;
    let result_file = format!("result-{}.json", t.attempt + 1);
    let kind = FixKind::Blockers { round: k, lines: blockers.clone() };
    let body = fix::body(
        &kind,
        &FixContext { task: tid, title: &t.title, check: &ctx.run.settings.check_command, round: k, result_file: &result_file },
    );
    let mut state = DriverState::load(&ctx.orch);
    state.jobs.retain(|j| j.task != *tid);
    state.jobs.push(QueuedJob { task: tid.clone(), file: format!("review-fix-{k}.md"), body });
    state.save(&ctx.orch).map_err(die)?;
    ctx.store
        .transition(tid, &TransitionRequest::auto(State::NeedsFix, Some(&format!("{} review blocker(s)", blockers.len()))))
        .map_err(die)?;
    println!(
        "QUEUED fix job for {tid} ({} blocker(s)){}",
        blockers.len(),
        if t.worker.is_some() { "" } else { " — any free coder" }
    );
    Ok(0)
}
