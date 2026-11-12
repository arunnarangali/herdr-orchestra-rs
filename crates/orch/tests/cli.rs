use std::process::{Command, Output};

fn orch(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_orch"))
        .args(args)
        .output()
        .unwrap()
}

const NAMES: &[&str] = &[
    "sync",
    "start",
    "ready",
    "pick",
    "prepare",
    "setup-done",
    "send",
    "review",
    "poll",
    "bump",
    "commit",
    "merge",
    "park",
    "cooldown",
    "set",
    "status",
    "cleanup",
    "write-task",
    "nudge",
    "autopilot",
    "send-job",
    "handoff",
    "needs-fix",
    "review-ok",
    "requeue-review",
    "finish",
    "resend",
    "approve",
    "report",
    "events",
    "dispatch",
    "answer",
    "escalate",
    "fix",
    "team",
    "lead",
    "agents",
    "doctor",
    "models",
];

#[test]
fn no_args_prints_usage_exit_0() {
    let o = orch(&[]);
    assert_eq!(o.status.code(), Some(0));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.starts_with("usage: orch"));
    for n in NAMES.iter().chain(&["version"]) {
        assert!(out.contains(n), "usage lacks {n}");
    }
}

#[test]
fn unknown_command_exit_2() {
    let o = orch(&["frobnicate"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stdout).contains("usage: orch"));
}

#[test]
fn version_forms() {
    for a in ["version", "--version", "-v"] {
        let o = orch(&[a]);
        assert_eq!(o.status.code(), Some(0), "{a}");
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(out.contains(env!("CARGO_PKG_VERSION")));
        assert!(out.contains("5.0.0"));
    }
}

#[test]
fn every_command_is_accepted_and_not_implemented() {
    for n in NAMES {
        let o = orch(&[n]);
        assert_eq!(o.status.code(), Some(70), "{n}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("not implemented in this build"));
    }
    for args in [
        &["lead", "plan"][..],
        &["lead", "run"],
        &["lead", "check"],
        &["lead", "whoami"],
        &["agents", "launch", "claude", "--model", "x"],
        &["agents", "sync"],
        &["agents", "check"],
        &["models", "show", "m"],
        &["models", "check", "m"],
        &["team", "up", "--dry-run"],
    ] {
        assert_eq!(orch(args).status.code(), Some(70), "{args:?}");
    }
}

#[test]
fn logging_quiet_by_default_and_on_stderr() {
    let o = orch(&["sync"]);
    assert_eq!(String::from_utf8_lossy(&o.stderr).lines().count(), 1);
    let o = Command::new(env!("CARGO_BIN_EXE_orch"))
        .env("ORCH_LOG", "debug")
        .arg("sync")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("dispatch"));
    assert!(o.stdout.is_empty());
}
