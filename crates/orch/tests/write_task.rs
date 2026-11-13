//! `orch write-task`: the coder contract, compared byte-for-byte with the output of real
//! Python v4.10.1 (`expected_*_task.{md,json}` in `write_task_fixtures/`, generated with
//! `/tmp/opencode/golden/gen.py`). The store mirrors what Python's `state.json` held when
//! the goldens were made: a synced run (M2's run storage) with hand-set counters, like the
//! generator's state.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use orch_core::run_store::{PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::store::Store;

/// The fixture text (plan.md, report-1.md, the expected files).
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/write_task_fixtures");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(FIXTURES).join(name)).unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The orch binary run inside `repo`.
fn orch_in(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_orch"))
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap()
}

/// The orch binary with an isolated HOME and herdr, as `orch sync` tests run it.
fn orch_sandboxed(repo: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_orch"))
        .args(args)
        .current_dir(repo)
        .env("HOME", home)
        .env("HERDR", home.join("no-herdr"))
        .env_remove("ORCH_AGENTS_FILE")
        .env_remove("ORCH_MODELS_FILE")
        .env_remove("ORCH_MAX_TASKS")
        .env_remove("ORCH_LOG")
        .output()
        .unwrap()
}

fn err_of(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn out_of(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// A repo like the golden generator's: plan.md, M1's report, and a synced run whose task
/// rows mirror the generator's `state.json` (hand-set files, tier, needs, counters).
struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

/// Store the run the way `orch sync` does, with the generator's values.
fn repo() -> Repo {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
    std::fs::write(root.join(".orchestrate/plan.md"), fixture("plan.md")).unwrap();
    std::fs::create_dir_all(root.join(".orch-task/M1")).unwrap();
    std::fs::write(root.join(".orch-task/M1/report-1.md"), fixture("report-1.md")).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
    sync_repo(&root);
    Repo { _dir: dir, root }
}

/// Apply a sync like Python's golden state held: two milestones, preset files.
fn sync_repo(root: &Path) {
    let mut store = Store::open(&root.join(".orchestrate/state.db")).unwrap();
    store
        .apply_sync(&SyncInput {
            settings: RunSettings {
                branch: "feature/sample".into(),
                base: None,
                title: "Sample Feature".into(),
                commits: String::new(),
                isolation: "shared".into(),
                max_parallel: 1,
                setup_inputs: vec![],
                scope: "warn".into(),
                risk_hold: false,
                budget_s: None,
                check_command: "cargo test".into(),
                setup_command: String::new(),
                lead_mode: String::new(),
                reviewer: None,
            },
            agents: Vec::<RunAgent>::new(),
            tasks: vec![
                PlanTask {
                    id: "M1".into(),
                    title: "Foundation".into(),
                    deps: vec![],
                    enabled: true,
                    open_questions: false,
                    tier: Some("small".into()),
                    needs: vec!["coding".into()],
                    model_why: "mechanical work on a known pattern".into(),
                    prefer: "any".into(),
                    review: false,
                    // The generator's hand-set state: "src/b" without the slash.
                    files: vec!["src/a.rs".into(), "src/b".into()],
                    parallel: false,
                },
                PlanTask {
                    id: "M2".into(),
                    title: "Second".into(),
                    deps: vec!["M1".into()],
                    enabled: true,
                    open_questions: false,
                    tier: Some("medium".into()),
                    needs: vec!["coding".into(), "tool-use".into()],
                    model_why: "normal coding with tool use".into(),
                    prefer: "any".into(),
                    review: true,
                    files: vec!["src/c.rs".into()],
                    parallel: false,
                },
            ],
        })
        .unwrap();
    // Python's state: M1 attempt 1, M2 attempt 2 + lost 1, both last coded by claude-1.
    store.bump("M1", orch_core::store::Counter::Attempt).unwrap();
    store.bump("M2", orch_core::store::Counter::Attempt).unwrap();
    store.bump("M2", orch_core::store::Counter::Attempt).unwrap();
    store.bump("M2", orch_core::store::Counter::Lost).unwrap();
    store.assign_worker("M1", Some("claude-1"), None).unwrap();
    store.assign_worker("M2", Some("claude-1"), None).unwrap();
}

// AC1/AC8: the JSON for a sample plan is Python's, byte for byte.
#[test]
fn the_task_json_is_pythons_byte_for_byte() {
    let r = repo();
    let o = orch_in(&r.root, &["write-task", "M1"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));

    let ours = std::fs::read_to_string(r.root.join(".orch-task/M1/task.json")).unwrap();
    let theirs = fixture("expected_M1_task.json");
    assert_eq!(ours, theirs, "{ours}");
}

// AC9: the task.md sections, byte for byte.
#[test]
fn the_task_md_is_pythons_byte_for_byte() {
    let r = repo();
    let o = orch_in(&r.root, &["write-task", "M1"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));

    let ours = std::fs::read_to_string(r.root.join(".orch-task/M1/task.md")).unwrap();
    assert_eq!(ours, fixture("expected_M1_task.md"));
}

// AC9: `WROTE <path>` for both files, task.md first.
#[test]
fn wrote_lines_name_both_files() {
    let r = repo();
    let o = orch_in(&r.root, &["write-task", "M1"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    assert_eq!(
        out_of(&o),
        format!(
            "WROTE {}/.orch-task/M1/task.md\nWROTE {}/.orch-task/M1/task.json\n",
            r.root.display(),
            r.root.display()
        )
    );
}

// The full M2 shape: global instructions, the dependency's report, AC ids, earlier work.
#[test]
fn a_milestone_with_a_dependency_and_parked_work_matches_python() {
    let r = repo();
    // Python's golden repo had Mn's parked work on its hidden ref, no merged dep commit.
    git(&r.root, &["update-ref", "refs/orch/feature-sample/M2", "HEAD"]);
    let o = orch_in(&r.root, &["write-task", "M2"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));

    let ours_md = std::fs::read_to_string(r.root.join(".orch-task/M2/task.md")).unwrap();
    assert_eq!(ours_md, fixture("expected_M2_task.md"));

    let ours_json = std::fs::read_to_string(r.root.join(".orch-task/M2/task.json")).unwrap();
    let theirs_json = fixture("expected_M2_task.json");
    assert_eq!(ours_json, theirs_json, "{ours_json}");
}

// AC5's landed-dependency commit: `git log --grep="^Milestone: M1$" <branch>`.
#[test]
fn a_merged_dependency_commit_lands_in_depends_on() {
    let r = repo();
    git(&r.root, &["update-ref", "refs/orch/feature-sample/M2", "HEAD"]);
    git(&r.root, &["checkout", "-q", "-b", "feature/sample"]);
    git(&r.root, &["commit", "-q", "--allow-empty", "-m", "feat: m1 lands", "-m", "Milestone: M1"]);
    let sha = git_out(&r.root, &["log", "-1", "--format=%H"]);

    let o = orch_in(&r.root, &["write-task", "M2"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));

    let ours = std::fs::read_to_string(r.root.join(".orch-task/M2/task.json")).unwrap();
    let golden = fixture("expected_M2_task.json");
    let expected = golden.replace(
        r#""commit": null"#,
        &format!(r#""commit": "{}""#, sha.trim()),
    );
    assert_eq!(ours, expected, "{ours}");
}

// The stored attempt (+1), lost attempts and the last worker reach the contract.
#[test]
fn the_store_counters_reach_the_contract() {
    let r = repo();
    git(&r.root, &["update-ref", "refs/orch/feature-sample/M2", "HEAD"]);
    let o = orch_in(&r.root, &["write-task", "M2"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    let ours = std::fs::read_to_string(r.root.join(".orch-task/M2/task.json")).unwrap();
    assert!(ours.contains(r#""attempt": 3"#), "{ours}");
    assert!(ours.contains(r#""lost_attempts": 1"#), "{ours}");
    assert!(ours.contains(r#""by": "claude-1""#), "{ours}");
}

// AC1: Python's `load()` first — without a synced run there is no contract, with the same
// message and exit code as `orch status` / `orch set`.
#[test]
fn write_task_without_a_synced_run_fails_like_status_and_set() {
    // No state.db at all: sync never ran (or failed before storing anything).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
    std::fs::write(root.join(".orchestrate/plan.md"), fixture("plan.md")).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let o = orch_in(&root, &["write-task", "M1"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    assert!(!root.join(".orch-task/M1").exists());
    assert_eq!(
        err_of(&o),
        "ERROR: no run in .orchestrate/state.db — run `orch sync` after writing plan.md\n"
    );

    // A state.db without a run row: a sync that died before storing anything.
    drop(Store::open(&root.join(".orchestrate/state.db")).unwrap());
    let o = orch_in(&root, &["write-task", "M1"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(
        err_of(&o),
        "ERROR: no run in .orchestrate/state.db — run `orch sync` after writing plan.md\n"
    );
}

// AC1: after a failed sync of a plan with a dependency cycle, write-task still refuses.
#[test]
fn after_a_failed_sync_of_a_cyclic_plan_write_task_fails() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let (root, home) = (base.join("repo"), base.join("home"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
    std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
    let cyclic = fixture("plan.md").replace(
        "Depends on: none\nWhy separate: the base others build on",
        "Depends on: M2 (loop)\nWhy separate: the base others build on",
    );
    std::fs::write(root.join(".orchestrate/plan.md"), cyclic).unwrap();

    let o = orch_sandboxed(&root, &home, &["sync"]);
    assert_eq!(o.status.code(), Some(2), "{}{}", out_of(&o), err_of(&o));
    // `orch sync` reports its validation errors on stdout.
    assert!(
        out_of(&o).contains("ERROR: dependency cycle: M1 -> M2 -> M1"),
        "{}{}",
        out_of(&o),
        err_of(&o)
    );
    // `orch sync` creates the state.db before it validates, so a failed sync leaves a db
    // with no run in it — the state write-task must refuse.
    assert!(root.join(".orchestrate/state.db").exists());

    let o = orch_sandboxed(&root, &home, &["write-task", "M1"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    assert!(!root.join(".orch-task/M1").exists());
    assert_eq!(
        err_of(&o),
        "ERROR: no run in .orchestrate/state.db — run `orch sync` after writing plan.md\n"
    );
}

// AC10: Python's message and exit code for an id the state does not know.
#[test]
fn an_unknown_milestone_fails_like_python() {
    let r = repo();
    let o = orch_in(&r.root, &["write-task", "M9"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    assert_eq!(err_of(&o), "ERROR: unknown task 'M9' — known: M1, M2\n");
}

// A task the state knows but the plan lost reads Python's other message.
#[test]
fn a_state_task_missing_from_the_plan_fails_like_python() {
    let r = repo();
    let mut store = orch_core::store::Store::open(&r.root.join(".orchestrate/state.db")).unwrap();
    store
        .add_task(&orch_core::store::NewTask {
            id: "M9".into(),
            title: "Extra".into(),
            deps: vec![],
        })
        .unwrap();
    let o = orch_in(&r.root, &["write-task", "M9"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err_of(&o), "ERROR: M9 not found in plan.md\n");
}

// AC10: no arguments, or a repo whose plan vanished after the sync.
#[test]
fn usage_and_missing_plan_fail_like_python() {
    let r = repo();
    let o = orch_in(&r.root, &["write-task"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err_of(&o), "ERROR: usage: orch write-task <Mn>\n");

    std::fs::remove_file(r.root.join(".orchestrate/plan.md")).unwrap();
    let o = orch_in(&r.root, &["write-task", "M1"]);
    // Python would traceback here (plan_sections reads the plan); this build says why.
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err_of(&o), "ERROR: missing .orchestrate/plan.md\n");
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}
