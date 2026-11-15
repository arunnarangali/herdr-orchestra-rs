//! `orch pause`, `orch resume` and `orch stop`: the developer's run-level controls. Each only
//! stores a request (`run_control` in the database); the event manager carries it out, so with
//! no event manager running the request waits. The REST API writes the same request.

use orch_core::actions::{RunControl, Source};
use orch_core::lease::{driver_alive, driver_lock_path};
use orch_core::paths;
use orch_core::store::{DB_FILE, Store};

const NO_RUN: &str = "no run in .orchestrate/state.db — run `orch sync` after writing plan.md";

/// What a stored request means, in one phrase (shown by `orch status`).
pub fn explain(c: RunControl) -> &'static str {
    match c {
        RunControl::Running => "carrying on",
        RunControl::Paused => "the event manager starts no new work; `orch resume` carries on",
        RunControl::Stopping => "the event manager is saving each running coder's work and ending",
        RunControl::Stopped => {
            "the event manager ended on request; `orch autopilot` starts it again"
        }
    }
}

fn die(msg: impl std::fmt::Display) -> i32 {
    eprintln!("ERROR: {msg}");
    2
}

fn apply(args: &[String], usage: &str, want: RunControl, done: &str) -> i32 {
    if !args.is_empty() {
        return die(format!("usage: orch {usage}"));
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let root = match paths::main_root(&cwd) {
        Ok(r) => r,
        Err(e) => return die(e),
    };
    let dir = root.join(paths::ORCH_DIR);
    if !dir.join(DB_FILE).exists() {
        return die(NO_RUN);
    }
    let mut store = match Store::open_in(&dir) {
        Ok(s) => s,
        Err(e) => return die(e),
    };
    match store.load_run() {
        Ok(Some(_)) => {}
        Ok(None) => return die(NO_RUN),
        Err(e) => return die(e),
    }
    let running = driver_alive(&driver_lock_path(&dir));
    // A stop with no event manager has nothing to end: it is already stopped.
    let stored = if want == RunControl::Stopping && !running {
        RunControl::Stopped
    } else {
        want
    };
    if let Err(e) = store.set_run_control(stored, Source::Cli) {
        return die(e);
    }
    let _ = store.record_action(Source::Cli, args_name(want), None, "");
    println!("{done}");
    if !running {
        println!(
            "The event manager is not running: this is recorded and takes effect when `orch autopilot` starts."
        );
    }
    0
}

fn args_name(c: RunControl) -> &'static str {
    match c {
        RunControl::Running => "resume",
        RunControl::Paused => "pause",
        RunControl::Stopping | RunControl::Stopped => "stop",
    }
}

/// `orch pause`.
pub fn pause(args: &[String]) -> i32 {
    apply(
        args,
        "pause",
        RunControl::Paused,
        "PAUSED: no new work starts; running milestones finish their step. `orch resume` carries on.",
    )
}

/// `orch resume`.
pub fn resume(args: &[String]) -> i32 {
    apply(
        args,
        "resume",
        RunControl::Running,
        "RESUMED: the event manager carries on.",
    )
}

/// `orch stop`.
pub fn stop(args: &[String]) -> i32 {
    apply(
        args,
        "stop",
        RunControl::Stopping,
        "STOP requested: the event manager saves each running coder's work and ends. `orch autopilot` starts it again.",
    )
}
