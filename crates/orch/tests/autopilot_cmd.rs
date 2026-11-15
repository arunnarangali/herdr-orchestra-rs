//! `orch autopilot` against the real binary in a temporary repo: the single-driver
//! lease (AC1), the `--once` cycle with its log, and the usage exit.

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
    fn new() -> Sandbox {
        Sandbox::with("cargo test", false)
    }

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
        std::fs::write(root.join(".orchestrate/plan.md"), PLAN.replace("CHECK", check)).unwrap();
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

#[test]
fn one_cycle_with_once_reconciles_and_writes_the_log() {
    let sb = Sandbox::new();
    let o = sb.orch(&["autopilot", "--once"]);
    // the main folder is not on the run branch, so the baseline check cannot run: the run stops
    // (exit 3) before any work, after the start line
    assert_eq!(o.status.code(), Some(3), "{}{}", out(&o), err(&o));
    let text = format!("{}{}", out(&o), err(&o));
    assert!(text.contains("autopilot started"), "{text}");
    assert!(text.contains("Baseline check fails"), "{text}");
    assert!(text.contains("not on the run branch feature/demo"), "{text}");
    // the cycle ran and logged its lines with timestamps
    let log = std::fs::read_to_string(sb.orch_dir().join("autopilot.log")).unwrap();
    assert!(log.contains("autopilot started"), "{log}");
    assert!(log.contains("] "), "the lines are timestamped: {log}");
    // the lease was released: a second --once cycle runs too
    let o = sb.orch(&["autopilot", "--once"]);
    assert_eq!(o.status.code(), Some(3), "{}{}", out(&o), err(&o));
}

#[test]
fn a_second_driver_is_refused_with_the_holder_line() {
    let sb = Sandbox::new();
    // hold the lease from this process: a second `orch autopilot` must refuse (AC1)
    let _lease = orch_core::lease::take_lease(&orch_core::lease::driver_lock_path(
        &sb.orch_dir(),
    ))
    .unwrap();
    let o = sb.orch(&["autopilot"]);
    assert_eq!(o.status.code(), Some(2), "{}{}", out(&o), err(&o));
    let text = err(&o);
    assert!(text.contains("already driven by"), "{text}");
    assert!(text.contains("orch autopilot (pid"), "{text}");
    // dropping the lease frees the run
    drop(_lease);
    let o = sb.orch(&["autopilot", "--once"]);
    assert_eq!(o.status.code(), Some(3), "{}{}", out(&o), err(&o));
}

#[test]
fn an_unknown_flag_prints_the_usage_and_exits_two() {
    let sb = Sandbox::new();
    let o = sb.orch(&["autopilot", "--badflag"]);
    assert_eq!(o.status.code(), Some(2), "{}{}", out(&o), err(&o));
    assert!(err(&o).contains("usage: orch autopilot [--once]"), "{}", err(&o));
}

#[test]
fn a_red_baseline_stops_the_run_before_any_work_and_leaves_no_file() {
    let sb = Sandbox::with("echo BASELINE-BAD-LINE; exit 1", true);
    let o = sb.orch(&["autopilot", "--once"]);
    assert_eq!(o.status.code(), Some(3), "{}{}", out(&o), err(&o));
    let text = format!("{}{}", out(&o), err(&o));
    assert!(text.contains("Baseline check fails on the feature branch before any work"), "{text}");
    assert!(text.contains("BASELINE-BAD-LINE"), "the failing lines are shown: {text}");
    assert!(!sb.orch_dir().join("baseline.txt").exists());
    // no milestone was started
    assert!(!text.contains("M1 ->"), "{text}");
}

#[test]
fn a_green_baseline_writes_the_file_and_a_second_start_does_not_rerun_it() {
    let sb = Sandbox::with("echo ran >> ../ran.log; echo GREEN-OUT", true);
    let ran = sb.root.parent().unwrap().join("ran.log");
    let first = sb.orch(&["autopilot", "--once"]);
    let text = format!("{}{}", out(&first), err(&first));
    assert!(!text.contains("Baseline check fails"), "{text}");
    let base = std::fs::read_to_string(sb.orch_dir().join("baseline.txt")).unwrap();
    assert!(base.contains("GREEN-OUT"), "{base}");
    assert_eq!(std::fs::read_to_string(&ran).unwrap().lines().count(), 1, "{text}");
    let _ = sb.orch(&["autopilot", "--once"]);
    assert_eq!(std::fs::read_to_string(&ran).unwrap().lines().count(), 1, "the baseline is not re-run");
}

// ---- graceful shutdown: SIGTERM/SIGINT end the run's driver cleanly, `orch stop` is separate

impl Sandbox {
    /// `orch autopilot` as a child whose check command takes ~2 s, so the baseline check is
    /// still running when the test sends its signal.
    fn spawn_slow_autopilot(&self) -> std::process::Child {
        Command::new(env!("CARGO_BIN_EXE_orch"))
            .arg("autopilot")
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("HERDR", self.home.join("no-herdr"))
            .env_remove("ORCH_LOG")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }

    /// Block until the child has logged its start (its signal handlers are in by then).
    fn wait_started(&self) {
        for _ in 0..200 {
            let log = std::fs::read_to_string(self.orch_dir().join("autopilot.log")).unwrap_or_default();
            if log.contains("autopilot started") {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("autopilot never started");
    }

    fn events(&self) -> String {
        std::fs::read_to_string(self.orch_dir().join("events.jsonl")).unwrap_or_default()
    }
}

fn signal(child: &std::process::Child, sig: &str) {
    let st = Command::new("kill").args([sig, &child.id().to_string()]).status().unwrap();
    assert!(st.success());
}

#[test]
fn sigterm_lets_the_running_check_finish_then_exits_zero_and_frees_the_lease() {
    let sb = Sandbox::with("sleep 2", true);
    let mut child = sb.spawn_slow_autopilot();
    sb.wait_started();
    signal(&child, "-TERM");
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    // the check was not cut off: the step finished, then the loop ended
    let log = std::fs::read_to_string(sb.orch_dir().join("autopilot.log")).unwrap();
    assert!(log.contains("shutdown on a stop signal"), "{log}");
    assert!(sb.events().contains("autopilot-shutdown"), "{}", sb.events());
    // the lease is free and no run-level stop was written
    assert!(!orch_core::lease::driver_alive(&orch_core::lease::driver_lock_path(&sb.orch_dir())));
    let store = orch_core::store::Store::open_in(&sb.orch_dir()).unwrap();
    assert_eq!(store.run_control().unwrap(), orch_core::actions::RunControl::Running);
    assert!(store.actions().unwrap().is_empty());

    // starting it again resumes (SIGINT this time); both are recorded
    let mut child = sb.spawn_slow_autopilot();
    for _ in 0..200 {
        if sb.events().matches("autopilot-started").count() == 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    signal(&child, "-INT");
    assert_eq!(child.wait().unwrap().code(), Some(0));
    let log = std::fs::read_to_string(sb.orch_dir().join("autopilot.log")).unwrap();
    assert!(log.contains("resuming after a stop signal"), "{log}");
    assert!(sb.events().contains("\"resumed\":true"), "{}", sb.events());
}

#[test]
fn a_second_signal_while_shutting_down_exits_at_once() {
    let sb = Sandbox::with("sleep 5", true);
    let mut child = sb.spawn_slow_autopilot();
    sb.wait_started();
    signal(&child, "-TERM");
    std::thread::sleep(std::time::Duration::from_millis(300));
    let t = std::time::Instant::now();
    signal(&child, "-TERM");
    let status = child.wait().unwrap();
    assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
    assert_eq!(status.code(), Some(130));
    assert!(!orch_core::lease::driver_alive(&orch_core::lease::driver_lock_path(&sb.orch_dir())));
    // killed mid-step: no shutdown line, so the next start is a plain start that reconciles
    assert!(!sb.events().contains("autopilot-shutdown"));
}
