//! `orch dispatch` (alias `retry`), `answer`, `fix`, `approve` and `escalate`: the lead's levers,
//! with the arguments, messages and exit codes of Python v4.10.1. `args` are the words after
//! the command. Each handler returns the exit code.
//!
//! State changes go through the store (`orch_core::plan_edit`, `Store::transition`), each as one
//! transaction; nothing here touches the database or `plan.md` directly. `orch answer` and
//! `orch fix` replace `plan.md` only after their transaction commits.

use orch_core::lease::{driver_alive, driver_info, driver_lock_path};
use orch_core::paths;
use orch_core::plan_edit::{EditError, py_repr};
use orch_core::run_store::{AgentRole, Run};
use orch_core::scheduler::{TIERS, need_rank, rank};
use orch_core::store::{DB_FILE, Store, TransitionRequest};
use orch_core::task::State;
use std::path::PathBuf;

const NO_RUN: &str = "no run in .orchestrate/state.db — run `orch sync` after writing plan.md";
const WOKEN: &str =
    "You will be woken with `orch events` when something needs you. Stop here; do not poll.";

fn die(msg: impl std::fmt::Display) -> i32 {
    eprintln!("ERROR: {msg}");
    2
}

/// Python's `need(args, n, usage)`.
fn need(args: &[String], n: usize, usage: &str) -> Result<(), i32> {
    if args.len() < n {
        Err(die(format!("usage: orch {usage}")))
    } else {
        Ok(())
    }
}

/// Take `flag <value>` out of `args` (Python: `args.index(flag)`, `del args[i:i+2]`).
fn take_flag(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, i32> {
    let Some(i) = args.iter().position(|a| a == flag) else {
        return Ok(None);
    };
    let Some(v) = args.get(i + 1).cloned() else {
        return Err(die(format!("{flag} needs a value")));
    };
    args.drain(i..i + 2);
    Ok(Some(v))
}

struct Ctx {
    store: Store,
    run: Run,
    dir: PathBuf,
}

/// The store and run of the repo around the current directory.
fn open() -> Result<Ctx, i32> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let root = paths::main_root(&cwd).map_err(die)?;
    let dir = root.join(paths::ORCH_DIR);
    if !dir.join(DB_FILE).exists() {
        return Err(die(NO_RUN));
    }
    let store = Store::open_in(&dir).map_err(die)?;
    match store.load_run().map_err(die)? {
        Some(run) => Ok(Ctx { store, run, dir }),
        None => Err(die(NO_RUN)),
    }
}

fn edit_err(e: EditError) -> i32 {
    die(e)
}

/// Python `task()`: the unknown-milestone refusal, for a read before any change.
fn known_task(ctx: &Ctx, tid: &str) -> Result<orch_core::task::Task, i32> {
    match ctx.store.get_task(tid) {
        Ok(t) => Ok(t),
        Err(_) => {
            let known: Vec<&str> = ctx.run.plan.iter().map(|p| p.id.as_str()).collect();
            Err(die(format!(
                "unknown task {} — known: {}",
                py_repr(tid),
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            )))
        }
    }
}

/// Workers allowed to take a milestone: its stored `--to`, else its tier or higher (unrated
/// coders count as any tier); when nobody reaches the tier, the strongest coders (Python
/// `eligible_workers`).
fn eligible_workers(ctx: &Ctx, tid: &str) -> Vec<String> {
    let intent = ctx.store.intent(tid).ok().flatten().unwrap_or_default();
    let workers: Vec<(&str, usize)> = ctx
        .run
        .agents
        .iter()
        .filter(|a| a.role == AgentRole::Worker)
        .map(|a| (a.name.as_str(), rank(a.tier.as_deref())))
        .collect();
    if let Some(a) = intent.assign.as_deref()
        && workers.iter().any(|(n, _)| *n == a)
    {
        return vec![a.to_string()];
    }
    let plan_tier = ctx
        .run
        .plan
        .iter()
        .find(|p| p.id == tid)
        .and_then(|p| p.tier.as_deref());
    let need = need_rank(plan_tier, intent.min_tier.as_deref());
    let ok: Vec<String> = workers
        .iter()
        .filter(|(_, r)| need == 0 || *r == 0 || *r >= need)
        .map(|(n, _)| n.to_string())
        .collect();
    if !ok.is_empty() {
        return ok;
    }
    let top = workers.iter().map(|(_, r)| *r).max().unwrap_or(0);
    workers
        .iter()
        .filter(|(_, r)| *r == top)
        .map(|(n, _)| n.to_string())
        .collect()
}

/// Python `start_manager`: the lead's request needs the event manager to carry it out. Says what
/// happened; starts `orch autopilot` detached when none runs (unless `ORCH_NO_AUTOSTART=1`).
fn start_manager(dir: &std::path::Path) -> String {
    use std::os::unix::process::CommandExt;
    let lock = driver_lock_path(dir);
    if driver_alive(&lock) {
        return format!(
            "the event manager is running ({})",
            driver_info(&lock).holder_line()
        );
    }
    if std::env::var("ORCH_NO_AUTOSTART").is_ok_and(|v| v == "1") {
        return "the event manager is NOT running — ask the developer to run `orch autopilot` in a terminal".into();
    }
    let root = dir.parent().unwrap_or(dir).to_path_buf();
    let spawned = std::env::current_exe().ok().and_then(|exe| {
        let out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("manager.out"))
            .ok()?;
        let err = out.try_clone().ok()?;
        let mut c = std::process::Command::new(exe);
        c.arg("autopilot")
            .current_dir(root)
            .stdin(std::process::Stdio::null())
            .stdout(out)
            .stderr(err)
            .process_group(0);
        for (k, _) in std::env::vars().filter(|(k, _)| k.starts_with("ORCH_DRIVER")) {
            c.env_remove(k);
        }
        c.spawn().ok()
    });
    if spawned.is_some() {
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if driver_alive(&lock) {
                return "started the event manager (`orch autopilot`, log: .orchestrate/autopilot.log)".into();
            }
        }
    }
    "could NOT start the event manager from here (see .orchestrate/manager.out) — ask the developer to run `orch autopilot` in a terminal".into()
}

/// `orch dispatch <Mn> [Mn...] [--to <worker>] [--tier small|medium|high]` (alias `retry`).
pub fn dispatch(args: &[String]) -> i32 {
    let mut args = args.to_vec();
    let (to, tier) = match (take_flag(&mut args, "--to"), take_flag(&mut args, "--tier")) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(c), _) | (_, Err(c)) => return c,
    };
    if let Err(c) = need(
        &args,
        1,
        "dispatch <Mn> [Mn...] [--to <worker>] [--tier small|medium|high]",
    ) {
        return c;
    }
    let mut ctx = match open() {
        Ok(c) => c,
        Err(c) => return c,
    };
    if let Err(e) = ctx
        .store
        .dispatch_milestones(&args, to.as_deref(), tier.as_deref())
    {
        return edit_err(e);
    }
    for tid in &args {
        let t = match known_task(&ctx, tid) {
            Ok(t) => t,
            Err(c) => return c,
        };
        let plan_tier = ctx
            .run
            .plan
            .iter()
            .find(|p| &p.id == tid)
            .and_then(|p| p.tier.clone());
        let min = ctx
            .store
            .intent(tid)
            .ok()
            .flatten()
            .and_then(|i| i.min_tier);
        let nr = need_rank(plan_tier.as_deref(), min.as_deref());
        let waits: Vec<&str> = t
            .deps
            .iter()
            .filter(|d| {
                ctx.store
                    .get_task(d)
                    .is_ok_and(|x| x.status != State::Merged)
            })
            .map(String::as_str)
            .collect();
        println!(
            "DISPATCH {tid} -> {}{}{}",
            eligible_workers(&ctx, tid).join(" or "),
            if nr > 0 {
                format!(" (tier {})", TIERS[nr - 1])
            } else {
                String::new()
            },
            if waits.is_empty() {
                String::new()
            } else {
                format!(" — starts when {} merged", waits.join(", "))
            }
        );
    }
    println!("{}", start_manager(&ctx.dir));
    println!("{WOKEN}");
    0
}

/// `orch answer <Mn> <the developer's answer>`.
pub fn answer(args: &[String]) -> i32 {
    if let Err(c) = need(args, 2, "answer <Mn> <answer text>") {
        return c;
    }
    let (tid, text) = (&args[0], args[1..].join(" ").trim().to_string());
    let mut ctx = match open() {
        Ok(c) => c,
        Err(c) => return c,
    };
    let plan = ctx.dir.join("plan.md");
    let done = match ctx.store.answer_milestone(&plan, tid, &text) {
        Ok(d) => d,
        Err(e) => return edit_err(e),
    };
    println!(
        "ANSWERED {tid}: written to plan.md; it goes back to {} with its work so far",
        done.last_worker.as_deref().unwrap_or("the next free coder")
    );
    println!("{}", start_manager(&ctx.dir));
    println!("{WOKEN}");
    0
}

/// `orch fix <Mn> <what should change> [--to <worker>]`.
pub fn fix(args: &[String]) -> i32 {
    let mut args = args.to_vec();
    let to = match take_flag(&mut args, "--to") {
        Ok(v) => v,
        Err(c) => return c,
    };
    if let Err(c) = need(&args, 2, "fix <Mn> <what should change> [--to <worker>]") {
        return c;
    }
    let tid = args[0].clone();
    let text = args[1..]
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return die("say what should change: orch fix <Mn> \"<what should change>\"");
    }
    let mut ctx = match open() {
        Ok(c) => c,
        Err(c) => return c,
    };
    let plan = ctx.dir.join("plan.md");
    let f = match ctx.store.fix_milestone(&plan, &tid, &text, to.as_deref()) {
        Ok(f) => f,
        Err(e) => return edit_err(e),
    };
    println!(
        "FIX {} \"{}\" -> {}; added to plan.md, the check{} still run",
        f.id,
        f.title,
        f.who
            .clone()
            .unwrap_or_else(|| format!("the next free coder (nobody on the team built {tid})")),
        if f.review { " and the review" } else { "" }
    );
    println!("{}", start_manager(&ctx.dir));
    println!(
        "It lands like any milestone: `orch status` shows {}, and the summary is written again when it is in.",
        f.id
    );
    0
}

/// `orch approve <Mn>`: let a held milestone land.
pub fn approve(args: &[String]) -> i32 {
    if let Err(c) = need(args, 1, "approve <Mn>") {
        return c;
    }
    let tid = &args[0];
    let mut ctx = match open() {
        Ok(c) => c,
        Err(c) => return c,
    };
    let t = match known_task(&ctx, tid) {
        Ok(t) => t,
        Err(c) => return c,
    };
    if t.status != State::Held {
        return die(format!("{tid} is {}, not held", t.status));
    }
    if let Err(e) = ctx.store.transition(
        tid,
        &TransitionRequest::auto(State::Committed, Some("approved by the developer")),
    ) {
        return die(e);
    }
    println!("APPROVED {tid}: it lands on the next autopilot step (or `orch merge {tid}`)");
    0
}

/// `orch escalate <Mn> [reason]`: out of rounds, one more go on a stronger model.
pub fn escalate(args: &[String]) -> i32 {
    if let Err(c) = need(args, 1, "escalate <Mn> [reason]") {
        return c;
    }
    let (tid, reason) = (&args[0], args[1..].join(" "));
    let mut ctx = match open() {
        Ok(c) => c,
        Err(c) => return c,
    };
    if let Err(c) = known_task(&ctx, tid) {
        return c;
    }
    match ctx.store.escalate_milestone(tid, &reason) {
        Ok(Some(tier)) => {
            println!("ESCALATED {tid} -> {tier}");
            0
        }
        Ok(None) => {
            println!("NO_ESCALATION {tid}");
            3
        }
        Err(e) => edit_err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_are_taken_out_with_their_value() {
        let mut a: Vec<String> = ["M1", "--to", "w", "M2"].map(String::from).to_vec();
        assert_eq!(take_flag(&mut a, "--to").unwrap(), Some("w".into()));
        assert_eq!(a, ["M1", "M2"]);
        assert_eq!(take_flag(&mut a, "--tier").unwrap(), None);
        let mut b: Vec<String> = vec!["M1".into(), "--tier".into()];
        assert_eq!(take_flag(&mut b, "--tier"), Err(2));
    }
}
