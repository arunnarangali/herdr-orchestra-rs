//! Handlers for `orch sync`, `orch status` and `orch set`. Only the plan-validation and
//! run-storage milestone edits this module. The logic lives in `sync.rs` (validation and the
//! sync run) and `sync_status.rs` (status and set); this file only wires them to the command
//! table. Each handler returns `Some(exit code)`. `args` are the words after the command.

#[path = "sync.rs"]
pub mod sync;
#[path = "sync_status.rs"]
pub mod sync_status;

use orch_config::tunables::Tunables;
use orch_herdr::cli::CliHerdr;

fn cwd() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| ".".into())
}

fn fail(e: impl std::fmt::Display) -> Option<i32> {
    eprintln!("ERROR: {e}");
    Some(2)
}

/// `orch sync`: parse `.orchestrate/plan.md`, validate it and store the run.
pub fn sync(_args: &[String]) -> Option<i32> {
    let reg = match orch_config::agents::load(None) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    let cat = match orch_config::models::load(None) {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    let tunables = match Tunables::from_env() {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    let herdr = CliHerdr::from_tunables(&tunables);
    let mut out = std::io::stdout();
    Some(sync::run_sync(
        &cwd(),
        &herdr,
        &reg,
        &cat,
        tunables.max_tasks,
        &mut out,
    ))
}

/// `orch status`: the run, from the store only.
pub fn status(_args: &[String]) -> Option<i32> {
    Some(sync_status::run_status(&cwd(), &mut std::io::stdout()))
}

/// `orch set <Mn> <status> [reason] [--force]`.
pub fn set(args: &[String]) -> Option<i32> {
    Some(sync_status::run_set(&cwd(), args, &mut std::io::stdout()))
}
