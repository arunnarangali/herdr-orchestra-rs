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

/// `orch` run with the developer's `HERDR_*` variables removed, so the result is the same
/// inside and outside a real herdr pane; `extra` sets what the test wants.
fn orch_clean(args: &[&str], extra: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_orch"));
    c.args(args);
    for (k, _) in std::env::vars().filter(|(k, _)| k.starts_with("HERDR")) {
        c.env_remove(k);
    }
    for (k, v) in extra {
        c.env(k, v);
    }
    c.output().unwrap()
}

/// The `fake-herdr` executable the workspace builds, next to the `orch` binary.
fn fake_herdr() -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_BIN_EXE_orch")).with_file_name("fake-herdr");
    assert!(p.exists(), "{} is missing: run `cargo build --workspace` or `cargo test --workspace`", p.display());
    p
}

fn fake_state(current_pane: bool) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    if current_pane {
        std::fs::create_dir_all(d.path().join("panes")).unwrap();
        std::fs::write(
            d.path().join("panes/w1:p1.json"),
            r#"{"pane_id":"w1:p1","workspace_id":"w1","tab_id":"w1:t1","agent_status":"idle"}"#,
        )
        .unwrap();
        std::fs::write(d.path().join("current_pane"), "w1:p1\n").unwrap();
    }
    d
}

#[test]
fn lead_check_ok_inside_a_herdr_pane() {
    // HERDR points at a binary that does not exist: HERDR_ENV=1 is enough.
    let o = orch_clean(
        &["lead", "check"],
        &[("HERDR_ENV", "1"), ("HERDR_PANE_ID", "w1:p4"), ("HERDR", "/nonexistent/herdr")],
    );
    assert_eq!(o.status.code(), Some(0));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.starts_with("OK inside a Herdr pane"), "{out}");
    assert!(out.contains("w1:p4"));
}

#[test]
fn lead_check_ok_when_herdr_pane_current_succeeds() {
    let state = fake_state(true);
    let fake = fake_herdr();
    let o = orch_clean(
        &["lead", "check"],
        &[("HERDR", fake.to_str().unwrap()), ("FAKE_HERDR_DIR", state.path().to_str().unwrap())],
    );
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.starts_with("OK herdr is reachable"), "{out}");
    assert!(out.contains("orch lead whoami"));
    let calls = std::fs::read_to_string(state.path().join("calls.jsonl")).unwrap();
    assert!(calls.contains(r#"["pane","current"]"#), "{calls}");
}

#[test]
fn lead_check_fails_with_the_fix_outside_a_herdr_pane() {
    // The fake has no current pane, as a herdr server with no pane to report.
    let state = fake_state(false);
    let fake = fake_herdr();
    let o = orch_clean(
        &["lead", "check"],
        &[("HERDR", fake.to_str().unwrap()), ("FAKE_HERDR_DIR", state.path().to_str().unwrap())],
    );
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.starts_with("ERROR: Herdr not detected: HERDR_ENV is not 1"), "{err}");
    assert!(err.contains("Start this agent from a Herdr pane"));
    assert!(err.contains("HERDR=/path/to/herdr"));
    assert!(err.contains("client_env_passthrough: [HERDR_ENV, HERDR_PANE_ID, HERDR_SOCKET_PATH, HERDR_BIN_PATH]"));
}

#[test]
fn lead_check_fails_when_herdr_is_missing() {
    let o = orch_clean(&["lead", "check"], &[("HERDR", "/nonexistent/herdr")]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("Herdr not detected"));
}
