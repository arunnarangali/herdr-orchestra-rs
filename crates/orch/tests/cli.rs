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
    // `agents`, `models` and `doctor` are real now (see the listing and agents_discovery tests).
    for n in NAMES.iter().filter(|n| !["agents", "models", "doctor"].contains(n)) {
        let o = orch(&[n]);
        assert_eq!(o.status.code(), Some(70), "{n}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("not implemented in this build"));
    }
    for args in [
        &["lead", "plan"][..],
        &["lead", "run"],
        &["lead", "whoami"],
        &["team", "claude"],
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
    assert!(
        p.exists(),
        "{} is missing: run `cargo build --workspace` or `cargo test --workspace`",
        p.display()
    );
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
        &[
            ("HERDR_ENV", "1"),
            ("HERDR_PANE_ID", "w1:p4"),
            ("HERDR", "/nonexistent/herdr"),
        ],
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
        &[
            ("HERDR", fake.to_str().unwrap()),
            ("FAKE_HERDR_DIR", state.path().to_str().unwrap()),
        ],
    );
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.starts_with("OK herdr is reachable"), "{out}");
    assert!(out.contains("orch lead whoami"));
    let calls = std::fs::read_to_string(state.path().join("calls.jsonl")).unwrap();
    assert!(
        calls.contains(r#"["pane","current","--current"]"#),
        "{calls}"
    );
}

#[test]
fn lead_check_fails_with_the_fix_outside_a_herdr_pane() {
    // The fake has no current pane, as a herdr server with no pane to report.
    let state = fake_state(false);
    let fake = fake_herdr();
    let o = orch_clean(
        &["lead", "check"],
        &[
            ("HERDR", fake.to_str().unwrap()),
            ("FAKE_HERDR_DIR", state.path().to_str().unwrap()),
        ],
    );
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.starts_with("ERROR: Herdr not detected: HERDR_ENV is not 1"),
        "{err}"
    );
    assert!(err.contains("Start this agent from a Herdr pane"));
    assert!(err.contains("HERDR=/path/to/herdr"));
    assert!(err.contains(
        "client_env_passthrough: [HERDR_ENV, HERDR_PANE_ID, HERDR_SOCKET_PATH, HERDR_BIN_PATH]"
    ));
}

#[test]
fn lead_check_fails_when_herdr_is_missing() {
    let o = orch_clean(&["lead", "check"], &[("HERDR", "/nonexistent/herdr")]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("Herdr not detected"));
}

#[test]
fn lead_check_honours_the_herdr_timeout_tunable() {
    let state = fake_state(true);
    std::fs::write(state.path().join("sim.json"), r#"{"slow_ms": 20000}"#).unwrap();
    let fake = fake_herdr();
    let started = std::time::Instant::now();
    let o = orch_clean(
        &["lead", "check"],
        &[
            ("HERDR", fake.to_str().unwrap()),
            ("FAKE_HERDR_DIR", state.path().to_str().unwrap()),
            ("ORCH_HERDR_TIMEOUT_S", "1"),
        ],
    );
    assert!(
        started.elapsed().as_secs() < 10,
        "must stop at ORCH_HERDR_TIMEOUT_S, not the old fixed 20s"
    );
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("timed out after 1s"));
}

/// `orch` with a temporary HOME, no user config files and a PATH holding only `bins`.
fn orch_cfg(
    args: &[&str],
    home: &std::path::Path,
    path: &std::path::Path,
    extra: &[(&str, &str)],
) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_orch"));
    c.args(args)
        .env("HOME", home)
        .env("PATH", path)
        .env_remove("ORCH_AGENTS_FILE")
        .env_remove("ORCH_MODELS_FILE");
    for (k, v) in extra {
        c.env(k, v);
    }
    c.output().unwrap()
}

/// A temp HOME and a PATH dir with an executable `claude` (never run) in it.
fn sandbox() -> (tempfile::TempDir, tempfile::TempDir) {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let f = bin.path().join("claude");
    std::fs::write(&f, "#!/bin/sh\nexit 99\n").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    (home, bin)
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn agents_table_shows_installed_from_path() {
    let (home, bin) = sandbox();
    let o = orch_cfg(&["agents"], home.path(), bin.path(), &[]);
    assert_eq!(o.status.code(), Some(0));
    let out = text(&o.stdout);
    assert!(
        out.starts_with("kind      installed detect verified roles"),
        "{out}"
    );
    let row = |k: &str| out.lines().find(|l| l.starts_with(k)).unwrap().to_string();
    assert!(
        row("claude").contains("claude      yes")
            || row("claude").split_whitespace().nth(1) == Some("yes"),
        "{out}"
    );
    assert_eq!(row("codex").split_whitespace().nth(1), Some("no"));
    assert!(row("claude").contains("lead,coder,reviewer"));
    assert!(row("claude").ends_with("claude --permission-mode acceptEdits"));
    assert!(out.contains("Override or add kinds in "));
    assert!(
        out.contains(
            &home
                .path()
                .join(".config/herdr-orchestra/agents.toml")
                .display()
                .to_string()
        )
    );
}

#[test]
fn agents_table_reads_the_user_registry_and_names_a_broken_file() {
    let (home, bin) = sandbox();
    let f = home.path().join("my-agents.toml");
    std::fs::write(
        &f,
        "[claude]\nlaunch = \"claude --foo\"\nroles = [\"tester\", \"specialist\"]\n",
    )
    .unwrap();
    let env = [("ORCH_AGENTS_FILE", f.to_str().unwrap())];
    let o = orch_cfg(&["agents"], home.path(), bin.path(), &env);
    assert_eq!(o.status.code(), Some(0));
    let out = text(&o.stdout);
    assert!(out.contains("tester,specialist"), "{out}");
    assert!(out.contains("claude --foo"));
    std::fs::write(&f, "[claude]\nroles = [\"boss\"]\n").unwrap();
    let o = orch_cfg(&["agents"], home.path(), bin.path(), &env);
    assert_eq!(o.status.code(), Some(2));
    let err = text(&o.stderr);
    assert!(
        err.contains("my-agents.toml") && err.contains("`roles`"),
        "{err}"
    );
}

#[test]
fn agents_launch_prints_the_command() {
    let (home, bin) = sandbox();
    let o = orch_cfg(
        &["agents", "launch", "claude"],
        home.path(),
        bin.path(),
        &[],
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(text(&o.stdout), "claude --permission-mode acceptEdits\n");
    let o = orch_cfg(
        &["agents", "launch", "claude", "--model", "opus"],
        home.path(),
        bin.path(),
        &[],
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        text(&o.stdout),
        "claude --permission-mode acceptEdits --model opus\n"
    );
    // flag before the kind works too
    let o = orch_cfg(
        &["agents", "launch", "--model", "opus", "claude"],
        home.path(),
        bin.path(),
        &[],
    );
    assert_eq!(
        text(&o.stdout),
        "claude --permission-mode acceptEdits --model opus\n"
    );
}

#[test]
fn agents_launch_failures_use_pythons_messages() {
    let (home, bin) = sandbox();
    let run = |args: &[&str]| orch_cfg(args, home.path(), bin.path(), &[]);
    let o = run(&["agents", "launch", "nope"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        text(&o.stderr).starts_with("ERROR: unknown agent kind 'nope' — known: "),
        "{}",
        text(&o.stderr)
    );
    let o = run(&["agents", "launch", "claude", "--model", "fable-5"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        text(&o.stderr)
            .contains("'fable-5' is not allowed by the model policy (pattern '*fable*')"),
        "{}",
        text(&o.stderr)
    );
    assert!(o.stdout.is_empty());
    let o = run(&["agents", "launch", "amp", "--model", "opus"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        text(&o.stderr).contains("the registry has no `model_flag` for amp"),
        "{}",
        text(&o.stderr)
    );
    let o = run(&["agents", "launch"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o.stderr).contains("usage: orch agents launch <kind> [--model <id>]"));
    let o = run(&["agents", "launch", "claude", "--model"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn agents_launch_warns_for_a_model_of_another_kind() {
    let (home, bin) = sandbox();
    let o = orch_cfg(
        &["agents", "launch", "codex", "--model", "opus"],
        home.path(),
        bin.path(),
        &[],
    );
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o.stdout).trim_end().ends_with("--model opus"));
    assert!(
        text(&o.stderr).contains("WARN: the catalog lists opus for claude, not codex"),
        "{}",
        text(&o.stderr)
    );
}

#[test]
fn models_prints_the_catalog_and_the_denied_patterns() {
    let (home, bin) = sandbox();
    let o = orch_cfg(&["models"], home.path(), bin.path(), &[]);
    assert_eq!(o.status.code(), Some(0));
    let out = text(&o.stdout);
    assert!(out.starts_with("model              tier    kinds"), "{out}");
    assert!(
        out.lines()
            .any(|l| l.starts_with("opus") && l.contains("high")),
        "{out}"
    );
    assert!(out.contains("Tiers (a model needs this reasoning rating to serve the tier):"));
    assert!(out.contains("Not allowed in runs: "), "{out}");
    assert!(out.contains("Override or add models in "));
    assert!(
        !out.lines().any(|l| l.starts_with("fable-5")),
        "denied models are not listed"
    );
}

#[test]
fn models_show_and_check() {
    let (home, bin) = sandbox();
    let run = |args: &[&str]| orch_cfg(args, home.path(), bin.path(), &[]);
    let o = run(&["models", "show", "OPUS"]);
    assert_eq!(o.status.code(), Some(0));
    let out = text(&o.stdout);
    assert!(out.starts_with("OK opus: tier high\n"), "{out}");
    assert!(
        out.contains("  reasoning     5\n") && out.contains("  evidence      "),
        "{out}"
    );
    let o = run(&["models", "check", "opus"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(text(&o.stdout), "OK opus: tier high\n");
    let o = run(&["models", "check", "fable-5"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        text(&o.stdout).starts_with("DENIED fable-5: matches policy pattern '"),
        "{}",
        text(&o.stdout)
    );
    let o = run(&["models", "show", "fable-5"]);
    assert_eq!(o.status.code(), Some(1));
    let o = run(&["models", "check", "mystery-1"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o.stdout).starts_with("UNRATED mystery-1: not in the catalog"));
    let o = run(&["models", "show", "mystery-1"]);
    assert_eq!(o.status.code(), Some(1));
    let o = run(&["models", "check"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o.stderr).contains("usage: orch models check <model id>"));
    let o = run(&["models", "frob"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn models_use_the_user_catalog() {
    let (home, bin) = sandbox();
    let f = home.path().join("m.toml");
    std::fs::write(&f, "[policy]\ndeny = [\"acme-*\"]\ndeny_why = \"no acme\"\n[models.my-1]\nkinds = [\"claude\"]\ntier = \"small\"\nreasoning = 1\n").unwrap();
    let env = [("ORCH_MODELS_FILE", f.to_str().unwrap())];
    let o = orch_cfg(
        &["models", "check", "acme-9"],
        home.path(),
        bin.path(),
        &env,
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(
        text(&o.stdout).contains("'acme-*' — no acme"),
        "{}",
        text(&o.stdout)
    );
    let o = orch_cfg(&["models", "check", "my-1"], home.path(), bin.path(), &env);
    assert_eq!(text(&o.stdout), "OK my-1: tier small\n");
    let o = orch_cfg(
        &["agents", "launch", "claude", "--model", "acme-1"],
        home.path(),
        bin.path(),
        &env,
    );
    assert_eq!(o.status.code(), Some(2));
}
