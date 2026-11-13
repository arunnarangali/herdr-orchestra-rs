//! `orch sync`, `orch status` and `orch set` against the real binary in a temporary repo,
//! with a temporary HOME and a herdr that does not exist.

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

## [x] M2: Two
Depends on: M1 (needs it)
Why separate: second
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
    fn new(plan: &str) -> Sandbox {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let (root, home) = (base.join("repo"), base.join("home"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
        std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
        std::fs::write(root.join(".orchestrate/plan.md"), plan).unwrap();
        Sandbox { _t: t, root, home }
    }

    fn orch(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_orch"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("HERDR", self.home.join("no-herdr"))
            .env_remove("ORCH_AGENTS_FILE")
            .env_remove("ORCH_MODELS_FILE")
            .env_remove("ORCH_MAX_TASKS")
            .env_remove("ORCH_LOG")
            .output()
            .unwrap()
    }
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn sync_status_and_set_round_trip() {
    let sb = Sandbox::new(PLAN);
    let o = sb.orch(&["sync"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    let text = out(&o);
    assert!(text.contains("WARN: M1 has no `Why separate:`"), "{text}");
    assert!(
        text.contains("WORKER claude-1 kind=claude herdr=NOT FOUND"),
        "{text}"
    );
    assert!(
        text.trim_end().ends_with(
            "OK: 2 tasks, isolation=worktree, max_parallel=2, workers=['claude-1'], reviewer=None"
        ),
        "{text}"
    );

    let s = sb.orch(&["status"]);
    assert_eq!(s.status.code(), Some(0));
    let text = out(&s);
    assert!(text.starts_with("branch=feature/demo base=None isolation=worktree max_parallel=2 reviewer=None (busy: None)\n"), "{text}");
    assert!(
        text.contains("  worker claude-1     claude   task=-     model=sonnet (medium) \n"),
        "{text}"
    );
    assert!(
        text.contains("  M1   pending         deps=-          worker=-          tier=medium One\n"),
        "{text}"
    );
    assert!(
        text.contains("  M2   pending         deps=M1         worker=-          tier=medium Two\n"),
        "{text}"
    );
    assert!(
        !text.contains("STUCK") && !text.contains("ALL_DONE"),
        "{text}"
    );

    // set: pending/paused/failed/skipped only, through the §11 table.
    let o = sb.orch(&["set", "M1", "paused", "waiting", "for", "the", "developer"]);
    assert_eq!(out(&o), "OK\n");
    let text = out(&sb.orch(&["status"]));
    assert!(text.contains("M1   paused          deps=-          worker=-          tier=medium One  [waiting for the developer]\n"), "{text}");
    assert!(
        text.contains("STUCK — nothing running and nothing ready;"),
        "{text}"
    );

    let o = sb.orch(&["set", "M1", "merged"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(
        err(&o),
        "ERROR: `orch set` only moves a task to failed, paused, pending, skipped; setting 'merged' by hand skips the steps that make it true — add --force if you really mean it\n"
    );
    let o = sb.orch(&["set", "M1", "wat"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        err(&o).starts_with(
            "ERROR: unknown status 'wat' — one of awaiting-review, coding, committed,"
        ),
        "{}",
        err(&o)
    );
    let o = sb.orch(&["set", "M9", "pending"]);
    assert_eq!(err(&o), "ERROR: unknown task 'M9' — known: M1, M2\n");
    let o = sb.orch(&["set", "M1"]);
    assert_eq!(
        err(&o),
        "ERROR: usage: orch set <Mn> <status> [reason] [--force]\n"
    );

    // paused -> skipped is allowed; skipped -> failed is not (the table), unless forced.
    assert_eq!(out(&sb.orch(&["set", "M1", "skipped"])), "OK\n");
    let o = sb.orch(&["set", "M1", "failed"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(
        err(&o),
        "ERROR: M1: skipped -> failed is not an allowed transition\n"
    );
    assert_eq!(
        out(&sb.orch(&["set", "M1", "merged", "--force", "done", "by", "hand"])),
        "OK\n"
    );
    let o = sb.orch(&["set", "M1", "pending"]);
    assert_eq!(
        err(&o),
        "ERROR: M1: merged -> pending is not an allowed transition (merged is final)\n"
    );
    // The forced move is in the history with its reason.
    let text = out(&sb.orch(&["status"]));
    assert!(text.contains("[manual override: done by hand]"), "{text}");
}

#[test]
fn status_and_set_without_a_run_say_so() {
    let sb = Sandbox::new(PLAN);
    for args in [&["status"][..], &["set", "M1", "pending"][..]] {
        let o = sb.orch(args);
        assert_eq!(o.status.code(), Some(2));
        assert_eq!(
            err(&o),
            "ERROR: no run in .orchestrate/state.db — run `orch sync` after writing plan.md\n"
        );
    }
    assert!(!sb.root.join(".orchestrate/state.db").exists());
}

#[test]
fn sync_errors_exit_2_and_a_missing_plan_is_reported() {
    let sb = Sandbox::new(&PLAN.replace(
        "Branch: feature/demo",
        "Branch: feature/demo\nIsolation: docker",
    ));
    let o = sb.orch(&["sync"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(
        out(&o),
        "ERROR: Isolation must be worktree or shared, got 'docker'\n"
    );
    std::fs::remove_file(sb.root.join(".orchestrate/plan.md")).unwrap();
    let o = sb.orch(&["sync"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err(&o), "ERROR: missing .orchestrate/plan.md\n");
}

#[test]
fn all_done_when_every_milestone_landed_or_is_skipped() {
    let sb = Sandbox::new(PLAN);
    sb.orch(&["sync"]);
    sb.orch(&["set", "M1", "skipped"]);
    sb.orch(&["set", "M2", "merged", "--force"]);
    let text = out(&sb.orch(&["status"]));
    assert!(
        text.contains("ALL_DONE — run `orch finish` to push feature/demo and open a PR into None"),
        "{text}"
    );
}

#[test]
fn archived_plans_validate() {
    // The Phase 2-4 plans (copied as fixtures by the plan-model milestone) sync cleanly.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../orch-core/tests/fixtures/plans");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let sb = Sandbox::new(&std::fs::read_to_string(&path).unwrap());
        let o = sb.orch(&["sync"]);
        assert_eq!(
            o.status.code(),
            Some(0),
            "{}: {}{}",
            path.display(),
            out(&o),
            err(&o)
        );
        assert!(out(&o).contains("\nOK: "), "{}", out(&o));
    }
}
