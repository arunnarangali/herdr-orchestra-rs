//! `orch status` and `orch set`: read and override the run `orch sync` stored. Both read only
//! the SQLite store (no git, no herdr).

use orch_core::actions::{self, Source};
use orch_core::paths;
use orch_core::run_store::Run;
use orch_core::store::{DB_FILE, Store};
use orch_core::view;
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
    let orch_dir = store.path().parent().map(Path::to_path_buf).unwrap_or_default();
    let v = match view::status_view(&store, &run, &orch_dir) {
        Ok(v) => v,
        Err(e) => return die(e),
    };
    let s = &v.settings;
    let _ = writeln!(
        out,
        "branch={} base={} isolation={} max_parallel={} reviewer={} (busy: None)",
        s.branch,
        s.base.as_deref().unwrap_or("None"),
        s.isolation,
        s.max_parallel,
        s.reviewer.as_deref().unwrap_or("None")
    );
    if let Some(b) = &v.broken {
        let _ = writeln!(out, "  {}", orch_core::integration::status_line(&s.branch, &b.reason));
    }
    match store.run_control() {
        Ok(c) if c != actions::RunControl::Running => {
            let _ = writeln!(out, "  run: {} — {}", c.as_str(), crate::run_ctl_cmds::explain(c));
        }
        _ => {}
    }
    for w in &v.workers {
        let model = w.model.as_ref().map_or(String::new(), |m| {
            format!("model={m} ({}) ", w.tier.as_deref().unwrap_or("unrated"))
        });
        let _ = writeln!(
            out,
            "  worker {:<12} {:<8} task={:<5} {}{}",
            w.name,
            w.kind,
            w.task.as_deref().unwrap_or("-"),
            model,
            if w.detect { "" } else { " detect:off" }
        );
    }
    for t in &v.tasks {
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
            t.tier
                .as_ref()
                .map_or(String::new(), |x| format!("tier={x} ")),
            t.title,
            if t.reason.is_empty() {
                String::new()
            } else {
                format!("  [{}]", t.reason)
            }
        );
    }
    if v.all_done {
        let _ = writeln!(
            out,
            "ALL_DONE — run `orch finish` to push {} and open a PR into {}",
            s.branch,
            s.base.as_deref().unwrap_or("None")
        );
    } else if v.stuck {
        let _ = writeln!(
            out,
            "STUCK — nothing running and nothing ready; resolve paused/needs-answer/failed tasks with the developer"
        );
    }
    0
}

/// `orch report`: per milestone the tries, fix, review and conflict rounds and lost
/// attempts, the totals, and, while the run branch fails its check, the broken line. Read from the
/// store only.
pub fn run_report(start: &Path, out: &mut dyn Write) -> i32 {
    let (store, run) = match open_run(start) {
        Ok(x) => x,
        Err(e) => return die(e),
    };
    let v = match view::report_view(&store, &run) {
        Ok(v) => v,
        Err(e) => return die(e),
    };
    let _ = writeln!(
        out,
        "{:<5} {:<15} {:>5} {:>4} {:>6} {:>5} {:>4}",
        "task", "status", "tries", "fix", "review", "confl", "lost"
    );
    for t in &v.rows {
        let _ = writeln!(
            out,
            "{:<5} {:<15} {:>5} {:>4} {:>6} {:>5} {:>4}",
            t.id,
            t.status.as_str(),
            t.attempt,
            t.fix_rounds,
            t.review_rounds,
            t.conflict_rounds,
            t.lost
        );
    }
    let _ = writeln!(
        out,
        "\nmerged {}/{}, failed {}; conflicts {}; lost attempts {}; fix rounds {}",
        v.merged,
        v.rows.len(),
        v.failed,
        v.conflicts,
        v.lost,
        v.fix_rounds
    );
    if let Some(b) = &v.broken {
        let _ = writeln!(out, "{}", orch_core::integration::status_line(&v.branch, &b.reason));
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
    let reason = args[2..]
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let to = match actions::manual_status(status, force) {
        Ok(to) => to,
        Err(e) => return die(e),
    };
    let (mut store, _run) = match open_run(start) {
        Ok(x) => x,
        Err(e) => return die(e),
    };
    match actions::set_status(&mut store, tid, to, &reason, force, Source::Cli) {
        Ok(_) => {
            let _ = writeln!(out, "OK");
            0
        }
        Err(e) => die(e),
    }
}
