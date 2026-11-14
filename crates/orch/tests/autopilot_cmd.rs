//! `orch autopilot` against the real binary in a temporary repo: the single-driver
//! lease (AC1), the `--once` cycle with its log, and the usage exit.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLAN: &str = "\
# Plan: Demo
Branch: feature/demo
Check command: cargo test
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
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let (root, home) = (base.join("repo"), base.join("home"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
        std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
        std::fs::write(root.join(".orchestrate/plan.md"), PLAN).unwrap();
        let sb = Sandbox { _t: t, root, home };
        let o = sb.orch(&["sync"]);
        assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
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
    // the milestone work is Phase 7: the stub parks M1 and the run stops for a
    // human (exit 3) — the cycle itself ran
    assert_eq!(o.status.code(), Some(3), "{}{}", out(&o), err(&o));
    let text = format!("{}{}", out(&o), err(&o));
    assert!(text.contains("autopilot started"), "{text}");
    assert!(text.contains("STOPPED — needs you"), "{text}");
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
