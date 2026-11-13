//! `orch status` and `orch set`: read and override the run `orch sync` stored. Both read only
//! the SQLite store (no git, no herdr).

use orch_config::launch::repr;
use orch_core::paths;
use orch_core::run_store::{Run, is_active};
use orch_core::store::{DB_FILE, Store};
use orch_core::task::{State, Task};
use std::io::Write;
use std::path::Path;

const NO_RUN: &str = "no run in .orchestrate/state.db — run `orch sync` after writing plan.md";

fn die(msg: impl std::fmt::Display) -> i32 {
    eprintln!("ERROR: {msg}");
    2
}

/// Open the store of the repo around `start` and load its run, or the Python-style error.
fn open_run(start: &Path) -> Result<(Store, Run), String> {
    let root = paths::main_root(start).map_err(|e| e.to_string())?;
    let dir = root.join(paths::ORCH_DIR);
    if !dir.join(DB_FILE).exists() {
        return Err(NO_RUN.into());
    }
    let store = Store::open_in(&dir).map_err(|e| e.to_string())?;
    match store.load_run().map_err(|e| e.to_string())? {
        Some(run) => Ok((store, run)),
        None => Err(NO_RUN.into()),
    }
}

/// `orch status`.
pub fn run_status(start: &Path, out: &mut dyn Write) -> i32 {
    let (store, run) = match open_run(start) {
        Ok(x) => x,
        Err(e) => return die(e),
    };
    let tasks = match store.list_tasks() {
        Ok(t) => t,
        Err(e) => return die(e),
    };
    let s = &run.settings;
    let _ = writeln!(
        out,
        "branch={} base={} isolation={} max_parallel={} reviewer={} (busy: None)",
        s.branch,
        s.base.as_deref().unwrap_or("None"),
        s.isolation,
        s.max_parallel,
        s.reviewer.as_deref().unwrap_or("None")
    );
    for w in run
        .agents
        .iter()
        .filter(|a| a.role == orch_core::run_store::AgentRole::Worker)
    {
        let held = tasks
            .iter()
            .find(|t| t.worker.as_deref() == Some(w.name.as_str()))
            .map_or("-", |t| t.id.as_str());
        let model = w.model.as_ref().map_or(String::new(), |m| {
            format!("model={m} ({}) ", w.tier.as_deref().unwrap_or("unrated"))
        });
        let _ = writeln!(
            out,
            "  worker {:<12} {:<8} task={:<5} {}{}",
            w.name,
            w.kind,
            held,
            model,
            if w.detect { "" } else { " detect:off" }
        );
    }
    let find = |id: &str| tasks.iter().find(|t| t.id == id);
    let mut shown: Vec<(&Task, &orch_core::run_store::PlannedTask)> = Vec::new();
    for p in &run.plan {
        let Some(t) = find(&p.id) else { continue };
        let _ = writeln!(
            out,
            "  {:<4} {:<15} deps={:<10} worker={:<10} {}{}{}",
            t.id,
            t.status.as_str(),
            if t.deps.is_empty() {
                "-".to_string()
            } else {
                t.deps.join(",")
            },
            t.worker.as_deref().unwrap_or("-"),
            p.tier
                .as_ref()
                .map_or(String::new(), |x| format!("tier={x} ")),
            t.title,
            if t.reason.is_empty() {
                String::new()
            } else {
                format!("  [{}]", t.reason)
            }
        );
        shown.push((t, p));
    }
    let merged = |id: &str| find(id).is_some_and(|t| t.status == State::Merged);
    if shown
        .iter()
        .all(|(t, _)| matches!(t.status, State::Merged | State::Skipped))
    {
        let _ = writeln!(
            out,
            "ALL_DONE — run `orch finish` to push {} and open a PR into {}",
            s.branch,
            s.base.as_deref().unwrap_or("None")
        );
    } else if !shown.iter().any(|(t, _)| is_active(t.status))
        && !shown
            .iter()
            .any(|(t, _)| t.status == State::Pending && t.deps.iter().all(|d| merged(d)))
    {
        let _ = writeln!(
            out,
            "STUCK — nothing running and nothing ready; resolve paused/needs-answer/failed tasks with the developer"
        );
    }
    0
}

/// `orch set <Mn> <status> [reason] [--force]`.
pub fn run_set(start: &Path, args: &[String], out: &mut dyn Write) -> i32 {
    let force = args.iter().any(|a| a == "--force");
    let args: Vec<&String> = args.iter().filter(|a| *a != "--force").collect();
    if args.len() < 2 {
        return die("usage: orch set <Mn> <status> [reason] [--force]");
    }
    let (tid, status) = (args[0].as_str(), args[1].as_str());
    let mut reason = args[2..]
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let Ok(to) = status.parse::<State>() else {
        let mut all: Vec<&str> = State::ALL.iter().map(|s| s.as_str()).collect();
        all.sort_unstable();
        return die(format!(
            "unknown status {} — one of {}",
            repr(status),
            all.join(", ")
        ));
    };
    if !State::MANUAL_OK.contains(&to) && !force {
        let mut ok: Vec<&str> = State::MANUAL_OK.iter().map(|s| s.as_str()).collect();
        ok.sort_unstable();
        return die(format!(
            "`orch set` only moves a task to {}; setting {} by hand skips the steps that make it true — add --force if you really mean it",
            ok.join(", "),
            repr(status)
        ));
    }
    let (mut store, run) = match open_run(start) {
        Ok(x) => x,
        Err(e) => return die(e),
    };
    if !run.plan.iter().any(|p| p.id == tid) {
        let known: Vec<&str> = run.plan.iter().map(|p| p.id.as_str()).collect();
        return die(format!(
            "unknown task {} — known: {}",
            repr(tid),
            if known.is_empty() {
                "none".to_string()
            } else {
                known.join(", ")
            }
        ));
    }
    if force {
        reason = format!("manual override: {reason}").trim().to_string();
    }
    match store.manual_set(tid, to, &reason, force) {
        Ok(_) => {
            let _ = writeln!(out, "OK");
            0
        }
        Err(e) => die(e),
    }
}
