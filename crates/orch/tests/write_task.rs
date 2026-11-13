//! `orch write-task`: the coder contract, compared byte-for-byte with the output of real
//! Python v4.10.1 (`expected_*_task.{md,json}` in `write_task_fixtures/`, generated with
//! `/tmp/opencode/golden/gen.py`). The store rows mirror what Python's `state.json` held
//! when the goldens were made.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

fn err_of(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn out_of(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// A repo like the golden generator's: plan.md, M1's report, the state rows.
/// Returns the repo (kept alive by the caller) and its root path.
struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

fn repo(store_rows: bool) -> Repo {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
    std::fs::write(root.join(".orchestrate/plan.md"), fixture("plan.md")).unwrap();
    std::fs::create_dir_all(root.join(".orch-task/M1")).unwrap();
    std::fs::write(root.join(".orch-task/M1/report-1.md"), fixture("report-1.md")).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
    if store_rows {
        let mut store = orch_core::store::Store::open(&root.join(".orchestrate/state.db")).unwrap();
        store
            .add_task(&orch_core::store::NewTask {
                id: "M1".into(),
                title: "Foundation".into(),
                deps: vec![],
            })
            .unwrap();
        store
            .add_task(&orch_core::store::NewTask {
                id: "M2".into(),
                title: "Second".into(),
                deps: vec!["M1".into()],
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
    Repo { _dir: dir, root }
}

// AC8: the JSON for a sample plan is Python's, byte for byte.
#[test]
fn the_task_json_is_pythons_byte_for_byte() {
    let r = repo(true);
    let o = orch_in(&r.root, &["write-task", "M1"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));

    let ours = std::fs::read_to_string(r.root.join(".orch-task/M1/task.json")).unwrap();
    let theirs = fixture("expected_M1_task.json");
    // One documented difference: Python's `files_allowed` comes from its state's tasks
    // table, which `orch sync` fills from the same plan text — keeping `src/b/`'s slash
    // (the production archive shows trailing slashes). The golden's state was preset by
    // hand ("src/b"), which no live parse of this plan can produce, so the compare
    // normalizes the slash away; everything else must be byte-exact.
    assert_eq!(ours.replace("src/b/", "src/b"), theirs, "{ours}");
}

// AC9: the task.md sections, byte for byte.
#[test]
fn the_task_md_is_pythons_byte_for_byte() {
    let r = repo(true);
    let o = orch_in(&r.root, &["write-task", "M1"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));

    let ours = std::fs::read_to_string(r.root.join(".orch-task/M1/task.md")).unwrap();
    assert_eq!(ours, fixture("expected_M1_task.md"));
}

// AC9: `WROTE <path>` for both files, task.md first.
#[test]
fn wrote_lines_name_both_files() {
    let r = repo(true);
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
    let r = repo(true);
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
    let r = repo(true);
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
    let r = repo(true);
    git(&r.root, &["update-ref", "refs/orch/feature-sample/M2", "HEAD"]);
    let o = orch_in(&r.root, &["write-task", "M2"]);
    assert_eq!(o.status.code(), Some(0), "{}", err_of(&o));
    let ours = std::fs::read_to_string(r.root.join(".orch-task/M2/task.json")).unwrap();
    assert!(ours.contains(r#""attempt": 3"#), "{ours}");
    assert!(ours.contains(r#""lost_attempts": 1"#), "{ours}");
    assert!(ours.contains(r#""by": "claude-1""#), "{ours}");
}

// AC10: Python's message and exit code for an id the state does not know.
#[test]
fn an_unknown_milestone_fails_like_python() {
    let r = repo(true);
    let o = orch_in(&r.root, &["write-task", "M9"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    assert_eq!(err_of(&o), "ERROR: unknown task 'M9' — known: M1, M2\n");
}

// A task the state knows but the plan lost reads Python's other message.
#[test]
fn a_state_task_missing_from_the_plan_fails_like_python() {
    let r = repo(true);
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

// Without a store, Python's known list is what the plan has.
#[test]
fn without_a_store_the_plan_lists_the_known_tasks() {
    let r = repo(false);
    let o = orch_in(&r.root, &["write-task", "M7"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err_of(&o), "ERROR: unknown task 'M7' — known: M1, M2\n");
}

// AC10: no arguments, or a repo without a plan.
#[test]
fn usage_and_missing_plan_fail_like_python() {
    let r = repo(true);
    let o = orch_in(&r.root, &["write-task"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err_of(&o), "ERROR: usage: orch write-task <Mn>\n");

    let empty = tempfile::tempdir().unwrap();
    git(empty.path(), &["init", "-q"]);
    let o = orch_in(empty.path(), &["write-task", "M1"]);
    // Python would traceback here (its load() reads state.json first); this build says why.
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err_of(&o), "ERROR: missing .orchestrate/plan.md\n");
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}
