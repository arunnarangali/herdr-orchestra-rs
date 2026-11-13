//! `orch lead plan|run|whoami`: the playbook text (shipped inside the binary) and the
//! lead's own pane, through the real adapter and the workspace's `fake-herdr`.

use std::process::{Command, Output};

/// The orch binary run with a clean herdr environment plus `extra`.
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

// AC11: the playbooks, shipped inside the binary, byte for byte.
#[test]
fn the_plan_playbook_prints_whole() {
    let o = orch_clean(&["lead", "plan"], &[]);
    assert_eq!(o.status.code(), Some(0));
    let out = String::from_utf8_lossy(&o.stdout);
    let pb = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/playbooks/plan.md"))
        .unwrap();
    assert_eq!(
        out,
        format!(
            "herdr-orchestra {} — lead playbook: plan\n\n{pb}",
            env!("CARGO_PKG_VERSION")
        )
    );
}

#[test]
fn the_run_playbook_prints_whole() {
    let o = orch_clean(&["lead", "run", "--ignored", "extra"], &[]);
    assert_eq!(o.status.code(), Some(0));
    let out = String::from_utf8_lossy(&o.stdout);
    let pb = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/playbooks/run.md"))
        .unwrap();
    assert_eq!(
        out,
        format!(
            "herdr-orchestra {} — lead playbook: run\n\n{pb}",
            env!("CARGO_PKG_VERSION")
        )
    );
}

// AC12: the current pane and the rename command, through the real adapter.
#[test]
fn whoami_reports_the_pane_and_the_rename_command() {
    let state = fake_state(true);
    let fake = fake_herdr();
    let o = orch_clean(
        &["lead", "whoami"],
        &[
            ("HERDR", fake.to_str().unwrap()),
            ("FAKE_HERDR_DIR", state.path().to_str().unwrap()),
            ("HERDR_PANE_ID", "w1:p1"),
        ],
    );
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(
        out,
        "PANE w1:p1 agent_status=idle\n\
This pane is YOU (the lead). Name it:  herdr agent rename w1:p1 lead\n\
Write `Lead: lead (<your kind>)` in the plan header, and never prompt this pane.\n"
    );
}

// Without HERDR_PANE_ID the focused-pane note comes first.
#[test]
fn whoami_without_pane_env_notes_the_focused_pane() {
    let state = fake_state(true);
    let fake = fake_herdr();
    let o = orch_clean(
        &["lead", "whoami"],
        &[
            ("HERDR", fake.to_str().unwrap()),
            ("FAKE_HERDR_DIR", state.path().to_str().unwrap()),
        ],
    );
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.starts_with("NOTE: HERDR_PANE_ID is not set in this shell"), "{out}");
    assert!(out.contains("PANE w1:p1 agent_status=idle"), "{out}");
}

// Outside a herdr pane: Python's message and exit code 2.
#[test]
fn whoami_fails_like_python_outside_a_herdr_pane() {
    let state = fake_state(false);
    let fake = fake_herdr();
    let o = orch_clean(
        &["lead", "whoami"],
        &[
            ("HERDR", fake.to_str().unwrap()),
            ("FAKE_HERDR_DIR", state.path().to_str().unwrap()),
        ],
    );
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.starts_with(
            "ERROR: not inside a herdr pane (`herdr pane current` failed) — start this agent from a Herdr pane: "
        ),
        "{err}"
    );
}
