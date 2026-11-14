//! `orch review`, `orch review-ok` and `orch requeue-review` (Python v4.10.1 `cmd_review`,
//! `cmd_review_ok`, `cmd_requeue_review`): the manual levers over a milestone's review, with
//! Python's arguments, messages and exit codes. `args` are the words after the command; each
//! handler returns the exit code.
//!
//! `orch review` and `orch requeue-review` change the reviewer's round and the milestone's state,
//! which the running event manager owns, so `orch review` refuses while `orch autopilot` runs.

use crate::listing::die;
use crate::run_work::RunWork;
use crate::send_cmds::{Ctx, known_task, open, refuse_while_driving, usage};
use orch_config::tunables::Tunables;
use orch_core::manager::Work;
use orch_core::review;
use orch_core::run_store::AgentRole;
use orch_core::store::{Counter, DeliveryKind, TransitionRequest};
use orch_core::task::State;
use orch_herdr::cli::CliHerdr;
use orch_herdr::delivery::{Deliverer, NewDelivery, SendOutcome};
use std::collections::HashSet;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

const EXIT_BUSY: i32 = 3;
const EXIT_DELIVERY: i32 = 5;

/// `orch review Mn`: snapshot the diff into `.orchestrate/review/<Mn>/` and prompt the reviewer.
pub fn review(args: &[String]) -> i32 {
    run_review(args).unwrap_or_else(|c| c)
}

fn to(ctx: &mut Ctx, tid: &str, state: State, why: &str) -> Result<(), i32> {
    ctx.store.transition(tid, &TransitionRequest::auto(state, Some(why))).map(|_| ()).map_err(die)
}

fn run_review(args: &[String]) -> Result<i32, i32> {
    usage(args, "review <Mn>")?;
    let tid = &args[0];
    let mut ctx = open()?;
    let t = known_task(&ctx, tid)?;
    let Some(reviewer) = ctx.run.agents.iter().find(|a| a.role == AgentRole::Reviewer).map(|a| a.name.clone()) else {
        return Err(die("no reviewer configured"));
    };
    refuse_while_driving(&ctx, "sends reviews")?;
    if t.worker.as_deref().or(t.last_worker.as_deref()) == Some(reviewer.as_str()) {
        return Err(die(format!("{reviewer} wrote {tid}: a milestone is never reviewed by its own coder")));
    }
    let on_other = ctx
        .store
        .list_tasks()
        .map_err(die)?
        .into_iter()
        .find(|o| o.id != *tid && o.status == State::Reviewing)
        .map(|o| o.id);
    let benched = ctx.store.active_benches(now()).map_err(die)?.iter().any(|b| b.agent == reviewer);
    if on_other.is_some() || benched {
        if t.status != State::AwaitingReview {
            to(&mut ctx, tid, State::AwaitingReview, "reviewer busy")?;
        }
        let why = on_other.map_or_else(|| "is benched (cooldown)".to_string(), |o| format!("is on {o}"));
        println!("BUSY reviewer {why}; {tid} queued as awaiting-review");
        return Ok(EXIT_BUSY);
    }
    if t.status != State::AwaitingReview {
        to(&mut ctx, tid, State::AwaitingReview, "review requested")?;
    }

    let k = t.review_rounds + 1;
    let body = RunWork::new(ctx.root.clone()).review_body(tid, k).map_err(die)?;
    let dir = review::review_dir(&ctx.orch, tid);
    let file = dir.join(format!("review-task-{k}.md"));
    std::fs::write(&file, body).map_err(|e| die(format!("writing {}: {e}", file.display())))?;
    ctx.store.bump(tid, Counter::Review).map_err(die)?;
    to(&mut ctx, tid, State::Reviewing, &format!("review round {k}"))?;

    let tun = Tunables::from_env().map_err(|_| die("could not read the tunables from the environment"))?;
    let herdr = CliHerdr::from_tunables(&tun);
    let watching: HashSet<String> =
        ctx.run.agents.iter().filter(|a| a.detect).map(|a| a.name.clone()).collect();
    let watchable = move |n: &str| watching.contains(n);
    let prompt = review::reviewer_message(tid, k);
    let sent = Deliverer::new(&mut ctx.store, &herdr, &watchable)
        .send(NewDelivery { agent: &reviewer, task: tid, attempt: t.attempt.max(1), kind: DeliveryKind::Reviewer, prompt: &prompt })
        .map_err(die)?;
    match sent {
        SendOutcome::Sent { .. } => {
            println!("SENT review {tid} -> {reviewer} (round {k})");
            Ok(0)
        }
        SendOutcome::Failed { error, .. } => {
            // the same round waits for review again; the reviewer is benched briefly
            ctx.store.drop_review_round(tid).map_err(die)?;
            to(&mut ctx, tid, State::AwaitingReview, "reviewer unreachable")?;
            ctx.store.bench_agent(&reviewer, "unreachable", now() + tun.send_retry_s).map_err(die)?;
            println!("DELIVERY_FAILED review {tid} -> {reviewer}: {error}");
            Ok(EXIT_DELIVERY)
        }
    }
}

/// `orch review-ok Mn`: record that the reviewer approved Mn (a later conflict fix will not need
/// a second review).
pub fn review_ok(args: &[String]) -> i32 {
    let run = || -> Result<i32, i32> {
        usage(args, "review-ok <Mn>")?;
        let tid = &args[0];
        let mut ctx = open()?;
        known_task(&ctx, tid)?;
        ctx.store.clear_rereview(tid).map_err(die)?;
        println!("OK {tid} review approved");
        Ok(0)
    };
    run().unwrap_or_else(|c| c)
}

/// `orch requeue-review Mn`: the reviewer vanished mid-review; queue the same round again.
pub fn requeue_review(args: &[String]) -> i32 {
    let run = || -> Result<i32, i32> {
        usage(args, "requeue-review <Mn>")?;
        let tid = &args[0];
        let mut ctx = open()?;
        let t = known_task(&ctx, tid)?;
        if t.status != State::Reviewing {
            return Err(die(format!("{tid} is {}, expected reviewing", t.status.as_str())));
        }
        ctx.store.drop_review_round(tid).map_err(die)?;
        to(&mut ctx, tid, State::AwaitingReview, "review requeued")?;
        println!("REQUEUED {tid} for review");
        Ok(0)
    };
    run().unwrap_or_else(|c| c)
}
