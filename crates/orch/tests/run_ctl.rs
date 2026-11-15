//! `orch pause`, `orch resume` and `orch stop` against the real binary in a temporary repo:
//! the stored request, what `orch status` shows, and what the event manager does with it.

use orch_core::actions::RunControl;
use orch_core::store::{Store, TransitionRequest};
use orch_core::task::State;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLAN: &str = "\
# Plan: Demo
Branch: feature/demo
Check command: CHECK
Workers: claude-1 (claude: sonnet)
Reviewer: none

## [x] M1: One
Depends on: none
Tier: medium
Model why: normal work
Goal:
- g
Acceptance criteria:
- works
";

struct Sandbox {
    _t: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(st.success());
}

impl Sandbox {
    /// A sandbox whose plan has `check` as its check command; `on_branch` puts the main folder
    /// on the run branch, as `orch start` leaves it.
    fn with(check: &str, on_branch: bool) -> Sandbox {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let (root, home) = (base.join("repo"), base.join("home"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
        std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
        std::fs::write(
            root.join(".orchestrate/plan.md"),
            PLAN.replace("CHECK", check),
        )
        .unwrap();
        let sb = Sandbox { _t: t, root, home };
        let o = sb.orch(&["sync"]);
        assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
        if on_branch {
            git(&sb.root, &["switch", "-q", "-c", "feature/demo"]);
        }
        sb
    }

    fn orch(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_orch"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("HERDR", self.home.join("no-herdr"))
            .env_remove("ORCH_AGENTS_FILE")
            .env_remove("ORCH_MODELS_FILE")
            .env_remove("ORCH_LOG")
            .output()
            .unwrap()
    }

    fn orch_dir(&self) -> PathBuf {
        self.root.join(".orchestrate")
    }
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn control(sb: &Sandbox) -> RunControl {
    Store::open_in(&sb.orch_dir())
        .unwrap()
        .run_control()
        .unwrap()
}

#[test]
fn pause_resume_and_stop_store_the_request_and_status_shows_it() {
    let sb = Sandbox::with("true", true);
    let plain = out(&sb.orch(&["status"]));
    assert!(
        !plain.contains("run:"),
        "a normal run shows no control line: {plain}"
    );

    let o = sb.orch(&["pause"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    assert!(out(&o).starts_with("PAUSED:"), "{}", out(&o));
    assert!(
        out(&o).contains("takes effect when `orch autopilot` starts"),
        "no event manager runs: {}",
        out(&o)
    );
    assert_eq!(control(&sb), RunControl::Paused);
    assert!(
        out(&sb.orch(&["status"])).contains("  run: paused — "),
        "{}",
        out(&sb.orch(&["status"]))
    );

    let o = sb.orch(&["resume"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    assert!(out(&o).starts_with("RESUMED:"));
    assert_eq!(control(&sb), RunControl::Running);
    assert_eq!(out(&sb.orch(&["status"])), plain);

    // No event manager: there is nothing to end, so the run is already stopped.
    assert_eq!(sb.orch(&["stop"]).status.code(), Some(0));
    assert_eq!(control(&sb), RunControl::Stopped);
    assert!(out(&sb.orch(&["status"])).contains("  run: stopped — "));

    // Each action is audited as a CLI action.
    let acts: Vec<(String, String)> = Store::open_in(&sb.orch_dir())
        .unwrap()
        .actions()
        .unwrap()
        .into_iter()
        .map(|a| (a.source, a.action))
        .collect();
    assert_eq!(
        acts,
        [("cli", "pause"), ("cli", "resume"), ("cli", "stop")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
    );
}

#[test]
fn with_a_running_event_manager_stop_is_a_request() {
    let sb = Sandbox::with("true", true);
    let _lease =
        orch_core::lease::take_lease(&orch_core::lease::driver_lock_path(&sb.orch_dir())).unwrap();
    let o = sb.orch(&["stop"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    assert!(!out(&o).contains("not running"), "{}", out(&o));
    assert_eq!(control(&sb), RunControl::Stopping);
}

#[test]
fn the_commands_refuse_arguments_and_a_missing_run() {
    let sb = Sandbox::with("true", true);
    for c in ["pause", "resume", "stop"] {
        let o = sb.orch(&[c, "extra"]);
        assert_eq!(o.status.code(), Some(2), "{c}");
        assert!(err(&o).contains(&format!("usage: orch {c}")), "{}", err(&o));
    }
    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    git(&base, &["init", "-q"]);
    let o = Command::new(env!("CARGO_BIN_EXE_orch"))
        .arg("pause")
        .current_dir(&base)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(
        err(&o).contains("no run in .orchestrate/state.db"),
        "{}",
        err(&o)
    );
    assert!(
        !base.join(".orchestrate").exists(),
        "pause must not create a run"
    );
}

#[test]
fn a_paused_event_manager_starts_nothing_and_the_pause_survives_it() {
    let sb = Sandbox::with("true", true);
    assert_eq!(sb.orch(&["pause"]).status.code(), Some(0));
    let o = sb.orch(&["autopilot", "--once"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    assert!(
        out(&o).contains("PAUSED — no new work starts"),
        "{}",
        out(&o)
    );
    let status = out(&sb.orch(&["status"]));
    assert!(
        status.contains("M1   pending"),
        "nothing was started: {status}"
    );
    assert!(
        status.contains("run: paused"),
        "the pause survived the event manager: {status}"
    );
    assert_eq!(control(&sb), RunControl::Paused);

    // Resumed: the next cycle no longer says it is paused.
    assert_eq!(sb.orch(&["resume"]).status.code(), Some(0));
    let o = sb.orch(&["autopilot", "--once"]);
    assert!(!out(&o).contains("PAUSED"), "{}", out(&o));
}

#[test]
fn a_stop_request_saves_running_work_ends_cleanly_and_a_new_start_carries_on() {
    let sb = Sandbox::with("true", true);
    {
        let mut s = Store::open_in(&sb.orch_dir()).unwrap();
        s.assign_worker("M1", Some("claude-1"), Some("sonnet"))
            .unwrap();
        s.transition("M1", &TransitionRequest::auto(State::Coding, Some("test")))
            .unwrap();
        s.set_run_control(RunControl::Stopping, orch_core::actions::Source::Cli)
            .unwrap();
    }
    let o = sb.orch(&["autopilot"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    let text = out(&o);
    assert!(text.contains("STOP requested"), "{text}");
    assert!(text.contains("stopped — "), "{text}");
    let s = Store::open_in(&sb.orch_dir()).unwrap();
    assert_eq!(s.run_control().unwrap(), RunControl::Stopped);
    let m1 = s.get_task("M1").unwrap();
    assert_eq!(
        m1.status,
        State::Pending,
        "the coder's milestone goes back to the queue"
    );
    assert!(m1.worker.is_none(), "its coder is freed");
    let why = s.history("M1").unwrap().last().unwrap().why.clone();
    assert!(why.contains("stopped by the developer"), "{why}");
    drop(s);

    // A fresh start clears the finished stop.
    let o = sb.orch(&["autopilot", "--once"]);
    assert!(!out(&o).contains("STOP requested"), "{}", out(&o));
    assert_eq!(control(&sb), RunControl::Running);
}
