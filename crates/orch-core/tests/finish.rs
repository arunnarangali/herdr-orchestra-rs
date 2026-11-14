//! `orch_core::finish` against real temporary repositories: refusals before anything is
//! touched, cleanup, the push (a local bare repo as `origin`), the PR step (a fake `gh`),
//! `Commits: single`, reruns after a failure, and the legacy branches of older versions.

use orch_core::finish::{self, Options};
use orch_core::git;
use orch_core::refs::{self, task_ref};
use orch_core::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::store::{Store, TransitionRequest};
use orch_core::task::State;
use orch_core::worktree;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn g(dir: &Path, args: &[&str]) -> String {
    let mut a = vec!["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"];
    a.extend(args);
    let out = git::run(dir, &a).unwrap();
    assert!(out.success(), "git {args:?}: {}", out.text());
    out.out().to_string()
}

fn bare(base: &Path) -> PathBuf {
    let p = base.join("origin.git");
    g(base, &["init", "-q", "--bare", p.to_str().unwrap()]);
    p
}

/// A `gh` script that logs its arguments to `log` and answers with `reply` (exiting `code`).
fn fake_gh(base: &Path, log: &Path, reply: &str, code: i32) -> PathBuf {
    let gh = base.join("gh");
    std::fs::write(
        &gh,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> {}\necho '{}'\nexit {}\n",
            log.display(),
            reply,
            code
        ),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&gh).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&gh, perms).unwrap();
    gh
}

fn task(id: &str) -> PlanTask {
    PlanTask {
        id: id.into(),
        title: format!("task {id}"),
        deps: vec![],
        enabled: true,
        open_questions: false,
        tier: None,
        needs: vec![],
        model_why: String::new(),
        prefer: "any".into(),
        review: false,
        files: vec![],
        parallel: true,
    }
}

fn agent(name: &str) -> RunAgent {
    RunAgent {
        role: AgentRole::Worker,
        name: name.into(),
        kind: "claude".into(),
        model: None,
        tier: None,
        detect: true,
        pane: None,
    }
}

struct Fx {
    _t: tempfile::TempDir,
    root: PathBuf,
}

impl Fx {
    /// A repo on `main` and a synced run (branch, two tasks M1 and M2, coders a and b).
    fn new(base: Option<&str>, branch: &str, commits: &str, check: &str) -> (Fx, Store) {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir(&root).unwrap();
        g(&root, &["init", "-q", "-b", "main"]);
        g(&root, &["config", "user.name", "t"]);
        g(&root, &["config", "user.email", "t@t"]);
        std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        g(&root, &["add", "-A"]);
        g(&root, &["commit", "-q", "-m", "init"]);
        let mut store = Store::open_in(&root.join(".orchestrate")).unwrap();
        store
            .apply_sync(&SyncInput {
                settings: RunSettings {
                    branch: branch.into(),
                    base: base.map(|b| b.into()),
                    title: "Demo".into(),
                    commits: commits.into(),
                    isolation: "worktree".into(),
                    max_parallel: 2,
                    setup_inputs: vec![],
                    scope: "warn".into(),
                    risk_hold: true,
                    budget_s: None,
                    check_command: check.into(),
                    setup_command: String::new(),
                    lead_mode: "off".into(),
                    reviewer: None,
                },
                agents: vec![agent("a"), agent("b")],
                tasks: vec![task("M1"), task("M2")],
            })
            .unwrap();
        (Fx { _t: t, root }, store)
    }

    /// The common fixture: `Base: main`, branch `feature/demo`, passing check.
    fn demo(commits: &str, check: &str) -> (Fx, Store) {
        Fx::new(Some("main"), "feature/demo", commits, check)
    }

    /// `orch start`: the main tree on the feature branch, one worktree per coder.
    fn started(&self, store: &mut Store) {
        worktree::start(store, &self.root).unwrap();
    }

    /// Land every milestone in the store (what M2's merge does to the state).
    fn all_merged(store: &mut Store) {
        for id in ["M1", "M2"] {
            store
                .transition(id, &TransitionRequest::manual(State::Merged, Some("test"), true))
                .unwrap();
        }
    }
}

fn commit_on(dir: &Path, name: &str) -> String {
    std::fs::write(dir.join(name), format!("{name}\n")).unwrap();
    g(dir, &["add", "-A"]);
    g(dir, &["commit", "-q", "-m", name]);
    g(dir, &["rev-parse", "HEAD"])
}

fn opts(no_push: bool, gh: Option<&Path>) -> Options {
    Options {
        no_push,
        gh: gh.map(|p| p.to_path_buf()),
    }
}

/// A GitHub-shaped remote (`remote get-url` answers with the GitHub URL) whose pushes really
/// go to a local bare repo through `remote.origin.pushurl`.
fn github_shaped_origin(root: &Path, origin: &Path) {
    g(root, &["remote", "add", "origin", "https://github.com/acme/demo.git"]);
    g(root, &["config", "remote.origin.pushurl", origin.to_str().unwrap()]);
}

#[test]
fn refuses_until_every_milestone_landed() {
    let (fx, mut store) = Fx::demo("per-milestone", "true");
    let e = finish::run(&mut store, &fx.root, &opts(true, None)).unwrap_err();
    assert_eq!(
        e.message,
        "not every milestone has landed: M1 (pending), M2 (pending)"
    );
    assert_eq!(e.lines, Vec::<String>::new());
}

#[test]
fn refuses_a_dirty_tree_or_the_wrong_branch() {
    let (fx, mut store) = Fx::demo("per-milestone", "true");
    Fx::all_merged(&mut store);
    std::fs::write(fx.root.join("a.txt"), "dirty\n").unwrap();
    let e = finish::run(&mut store, &fx.root, &opts(true, None)).unwrap_err();
    assert_eq!(e.message, "main tree must be clean and on feature/demo");
    g(&fx.root, &["checkout", "-q", "--", "a.txt"]);
    let e = finish::run(&mut store, &fx.root, &opts(true, None)).unwrap_err();
    assert_eq!(e.message, "main tree must be clean and on feature/demo");
}

#[test]
fn a_failing_check_stops_before_anything_is_pushed_or_deleted() {
    let (fx, mut store) = Fx::demo("per-milestone", "echo boom; exit 6");
    fx.started(&mut store);
    refs::write(&fx.root, &task_ref("feature/demo", "M1"), "HEAD").unwrap();
    Fx::all_merged(&mut store);
    let e = finish::run(&mut store, &fx.root, &opts(true, None)).unwrap_err();
    assert!(
        e.message.starts_with("the check fails on feature/demo — fix it before finishing:"),
        "{e:?}"
    );
    assert!(e.message.contains("boom"), "{e:?}");
    // the worktree and the hidden ref are still there
    assert!(worktree::worktree_path(&fx.root, "a").exists());
    assert!(refs::read(&fx.root, &task_ref("feature/demo", "M1")).unwrap().is_some());
}

#[test]
fn cleans_up_pushes_and_reports_clean_with_upstream_set() {
    let t = tempfile::tempdir().unwrap();
    let origin = bare(t.path());
    let (fx, mut store) = Fx::demo("per-milestone", "true");
    fx.started(&mut store);
    commit_on(&fx.root, "m1.txt");
    commit_on(&fx.root, "m2.txt");
    refs::write(&fx.root, &task_ref("feature/demo", "M1"), "HEAD").unwrap();
    Fx::all_merged(&mut store);
    github_shaped_origin(&fx.root, &origin);
    let lines = finish::run(&mut store, &fx.root, &opts(false, None)).unwrap();
    for w in ["a", "b"] {
        assert!(
            lines.contains(&format!("REMOVED {}", worktree::worktree_path(&fx.root, w).display())),
            "{lines:?}"
        );
        assert!(!worktree::worktree_path(&fx.root, w).exists());
    }
    assert!(
        lines.contains(&"CLEAN feature/demo is the only branch this run made (1 hidden ref(s) removed)".to_string()),
        "{lines:?}"
    );
    assert!(lines.contains(&"PUSHED feature/demo to origin".to_string()), "{lines:?}");
    assert_eq!(
        lines.last().map(String::as_str),
        Some("OPEN A PR from feature/demo into main: https://github.com/acme/demo/compare/main...feature/demo?expand=1"),
        "{lines:?}"
    );
    assert!(refs::list(&fx.root, "feature/demo").unwrap().is_empty());
    // only the feature branch reached the remote, with upstream set
    assert_eq!(g(&origin, &["for-each-ref", "--format=%(refname)"]), "refs/heads/feature/demo");
    assert_eq!(
        g(&fx.root, &["rev-parse", "--abbrev-ref", "feature/demo@{upstream}"]),
        "origin/feature/demo"
    );
    // no other branch was touched or deleted
    assert!(git::branch_exists(&fx.root, "main").unwrap());
}

#[test]
fn no_push_stops_before_any_remote_call() {
    let t = tempfile::tempdir().unwrap();
    let origin = bare(t.path());
    let (fx, mut store) = Fx::demo("per-milestone", "true");
    fx.started(&mut store);
    commit_on(&fx.root, "m1.txt");
    refs::write(&fx.root, &task_ref("feature/demo", "M1"), "HEAD").unwrap();
    Fx::all_merged(&mut store);
    g(&fx.root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    let lines = finish::run(&mut store, &fx.root, &opts(true, None)).unwrap();
    assert!(
        lines.contains(&"NOT PUSHED (--no-push): push feature/demo and open a PR into main when ready".to_string()),
        "{lines:?}"
    );
    assert!(g(&origin, &["for-each-ref", "--format=%(refname)"]).is_empty());
}

#[test]
fn without_gh_a_non_github_remote_gets_a_plain_note() {
    let t = tempfile::tempdir().unwrap();
    let origin = bare(t.path());
    let (fx, mut store) = Fx::demo("per-milestone", "true");
    fx.started(&mut store);
    commit_on(&fx.root, "m1.txt");
    Fx::all_merged(&mut store);
    g(&fx.root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    let lines = finish::run(&mut store, &fx.root, &opts(false, None)).unwrap();
    assert!(lines.contains(&"PUSHED feature/demo to origin".to_string()), "{lines:?}");
    assert_eq!(
        lines.last().map(String::as_str),
        Some("OPEN A PR from feature/demo into main"),
        "{lines:?}"
    );
}

#[test]
fn with_gh_opens_the_pr_from_the_title_and_the_milestone_commits() {
    let t = tempfile::tempdir().unwrap();
    let origin = bare(t.path());
    let (fx, mut store) = Fx::demo("per-milestone", "true");
    let log = t.path().join("gh.log");
    let gh = fake_gh(t.path(), &log, "https://github.com/acme/demo/pull/1", 0);
    fx.started(&mut store);
    commit_on(&fx.root, "m1.txt");
    commit_on(&fx.root, "m2.txt");
    Fx::all_merged(&mut store);
    github_shaped_origin(&fx.root, &origin);
    let lines = finish::run(&mut store, &fx.root, &opts(false, Some(&gh))).unwrap();
    assert!(lines.contains(&"PUSHED feature/demo to origin".to_string()), "{lines:?}");
    assert_eq!(
        lines.last().map(String::as_str),
        Some("PR https://github.com/acme/demo/pull/1"),
        "{lines:?}"
    );
    let call = std::fs::read_to_string(&log).unwrap();
    for arg in ["--base", "main", "--head", "feature/demo", "--title", "Demo"] {
        assert!(call.contains(arg), "{arg} missing from {call}");
    }
    // the body is the title and the milestone commit lines, and nothing else
    let body = call.split("--body").nth(1).unwrap_or_default().trim();
    assert!(body.starts_with("Demo"), "{call}");
    assert!(body.contains("- m1.txt"), "{call}");
    assert!(body.contains("- m2.txt"), "{call}");
    assert!(!body.contains("Generated with"), "{call}");
    assert!(!body.contains("Built by"), "{call}");
}

#[test]
fn the_lead_summary_is_used_as_the_pr_body_when_it_exists() {
    let t = tempfile::tempdir().unwrap();
    let origin = bare(t.path());
    let (fx, mut store) = Fx::demo("per-milestone", "true");
    let log = t.path().join("gh.log");
    let gh = fake_gh(t.path(), &log, "https://github.com/acme/demo/pull/1", 0);
    fx.started(&mut store);
    commit_on(&fx.root, "m1.txt");
    Fx::all_merged(&mut store);
    github_shaped_origin(&fx.root, &origin);
    std::fs::write(fx.root.join(".orchestrate/summary.md"), "# Summary: Demo\n\nDone.\n").unwrap();
    let lines = finish::run(&mut store, &fx.root, &opts(false, Some(&gh))).unwrap();
    assert_eq!(
        lines.last().map(String::as_str),
        Some("PR https://github.com/acme/demo/pull/1"),
        "{lines:?}"
    );
    let call = std::fs::read_to_string(&log).unwrap();
    assert!(call.contains("--body-file"), "{call}");
    assert!(!call.contains("--body "), "{call}");
}

#[test]
fn a_failed_pr_step_is_reported_and_the_rerun_retries_it() {
    let t = tempfile::tempdir().unwrap();
    let origin = bare(t.path());
    let (fx, mut store) = Fx::demo("per-milestone", "true");
    let log = t.path().join("gh.log");
    let gh = fake_gh(t.path(), &log, "gh: could not create PR", 1);
    fx.started(&mut store);
    commit_on(&fx.root, "m1.txt");
    Fx::all_merged(&mut store);
    github_shaped_origin(&fx.root, &origin);
    let lines = finish::run(&mut store, &fx.root, &opts(false, Some(&gh))).unwrap();
    assert!(
        lines.contains(&"NOTE gh pr create failed: gh: could not create PR".to_string()),
        "{lines:?}"
    );
    assert_eq!(
        lines.last().map(String::as_str),
        Some("OPEN A PR from feature/demo into main: https://github.com/acme/demo/compare/main...feature/demo?expand=1"),
        "{lines:?}"
    );
    // the rerun is safe: nothing is removed twice, the push is idempotent, the PR is retried
    let log2 = t.path().join("gh2.log");
    let gh2 = fake_gh(t.path(), &log2, "https://github.com/acme/demo/pull/2", 0);
    let lines = finish::run(&mut store, &fx.root, &opts(false, Some(&gh2))).unwrap();
    assert!(
        lines.contains(&"CLEAN feature/demo is the only branch this run made (0 hidden ref(s) removed)".to_string()),
        "{lines:?}"
    );
    assert_eq!(
        lines.last().map(String::as_str),
        Some("PR https://github.com/acme/demo/pull/2"),
        "{lines:?}"
    );
    let call = std::fs::read_to_string(&log2).unwrap();
    assert!(call.contains("--head\nfeature/demo"), "{call}");
}

#[test]
fn a_failed_push_is_reported_and_the_rerun_pushes() {
    let t = tempfile::tempdir().unwrap();
    let (fx, mut store) = Fx::demo("per-milestone", "true");
    fx.started(&mut store);
    commit_on(&fx.root, "m1.txt");
    refs::write(&fx.root, &task_ref("feature/demo", "M1"), "HEAD").unwrap();
    Fx::all_merged(&mut store);
    // origin is not a git repository: the push fails
    let bad = t.path().join("not-a-repo");
    std::fs::create_dir(&bad).unwrap();
    g(&fx.root, &["remote", "add", "origin", bad.to_str().unwrap()]);
    let e = finish::run(&mut store, &fx.root, &opts(false, None)).unwrap_err();
    assert!(e.message.contains("git push"), "{e:?}");
    assert!(
        e.lines.contains(&"CLEAN feature/demo is the only branch this run made (1 hidden ref(s) removed)".to_string()),
        "{e:?}"
    );
    // the destructive work is not redone, and the push now goes through
    let origin = bare(t.path());
    g(&fx.root, &["remote", "set-url", "origin", origin.to_str().unwrap()]);
    let lines = finish::run(&mut store, &fx.root, &opts(false, None)).unwrap();
    assert!(
        lines.contains(&"CLEAN feature/demo is the only branch this run made (0 hidden ref(s) removed)".to_string()),
        "{lines:?}"
    );
    assert!(lines.contains(&"PUSHED feature/demo to origin".to_string()), "{lines:?}");
}

#[test]
fn commits_single_squashes_the_run_into_one_commit() {
    let (fx, mut store) = Fx::demo("single", "true");
    fx.started(&mut store);
    commit_on(&fx.root, "m1.txt");
    commit_on(&fx.root, "m2.txt");
    Fx::all_merged(&mut store);
    let lines = finish::run(&mut store, &fx.root, &opts(true, None)).unwrap();
    assert!(
        lines.contains(&"SQUASHED 2 commits into one (Commits: single)".to_string()),
        "{lines:?}"
    );
    assert_eq!(g(&fx.root, &["rev-list", "--count", "main..HEAD"]), "1");
    let msg = g(&fx.root, &["log", "-1", "--format=%B"]);
    assert!(msg.starts_with("Demo\n"), "{msg}");
    assert!(msg.contains("- m1.txt"), "{msg}");
    assert!(msg.contains("- m2.txt"), "{msg}");
    assert!(msg.contains("Milestones: M1, M2"), "{msg}");
    // the rerun squashes nothing again
    let lines = finish::run(&mut store, &fx.root, &opts(true, None)).unwrap();
    assert!(!lines.iter().any(|l| l.starts_with("SQUASHED")), "{lines:?}");
    assert_eq!(g(&fx.root, &["rev-list", "--count", "main..HEAD"]), "1");
}

#[test]
fn commits_single_pr_body_keeps_the_milestone_lines() {
    let t = tempfile::tempdir().unwrap();
    let origin = bare(t.path());
    let (fx, mut store) = Fx::demo("single", "true");
    let log = t.path().join("gh.log");
    let gh = fake_gh(t.path(), &log, "https://github.com/acme/demo/pull/1", 0);
    fx.started(&mut store);
    commit_on(&fx.root, "m1.txt");
    commit_on(&fx.root, "m2.txt");
    Fx::all_merged(&mut store);
    github_shaped_origin(&fx.root, &origin);
    finish::run(&mut store, &fx.root, &opts(false, Some(&gh))).unwrap();
    let call = std::fs::read_to_string(&log).unwrap();
    let body = call.split("--body").nth(1).unwrap_or_default().trim();
    assert!(body.contains("- m1.txt"), "{call}");
    assert!(body.contains("- m2.txt"), "{call}");
    assert!(!body.contains("- Demo"), "{call}");
}

#[test]
fn commits_per_milestone_leaves_history_alone() {
    let (fx, mut store) = Fx::demo("per-milestone", "true");
    fx.started(&mut store);
    commit_on(&fx.root, "m1.txt");
    commit_on(&fx.root, "m2.txt");
    Fx::all_merged(&mut store);
    let lines = finish::run(&mut store, &fx.root, &opts(true, None)).unwrap();
    assert!(!lines.iter().any(|l| l.starts_with("SQUASHED")), "{lines:?}");
    assert_eq!(g(&fx.root, &["rev-list", "--count", "main..HEAD"]), "2");
}

#[test]
fn legacy_branches_are_picked_up_at_finish() {
    let (fx, mut store) = Fx::new(Some("main"), "orch/demo", "per-milestone", "true");
    // a pre-4.5 task branch with real work
    g(&fx.root, &["switch", "-q", "-c", "orch/demo-M1"]);
    commit_on(&fx.root, "old-work.txt");
    g(&fx.root, &["switch", "-q", "main"]);
    fx.started(&mut store);
    commit_on(&fx.root, "m1.txt");
    Fx::all_merged(&mut store);
    let lines = finish::run(&mut store, &fx.root, &opts(true, None)).unwrap();
    assert!(
        lines.contains(&"PICKED UP legacy branch orch/demo-M1: its work is in refs/orch/orch-demo/M1 now, and the branch is gone".to_string()),
        "{lines:?}"
    );
    assert!(!git::branch_exists(&fx.root, "orch/demo-M1").unwrap());
    // the run's cleanup removes the ref it was carried into, with the rest
    assert!(refs::read(&fx.root, "refs/orch/orch-demo/M1").unwrap().is_none());
    assert!(
        lines.contains(&"CLEAN orch/demo is the only branch this run made (1 hidden ref(s) removed)".to_string()),
        "{lines:?}"
    );
}

#[test]
fn legacy_branches_are_kept_or_deleted_without_losing_anything() {
    let (fx, mut store) = Fx::new(Some("main"), "orch/demo", "per-milestone", "true");
    fx.started(&mut store);
    commit_on(&fx.root, "m1.txt");
    // a legacy branch whose tip the hidden ref already holds
    g(&fx.root, &["switch", "-q", "-c", "orch/demo-M2"]);
    let carried = commit_on(&fx.root, "carried.txt");
    g(&fx.root, &["switch", "-q", "orch/demo"]);
    refs::write(&fx.root, "refs/orch/orch-demo/M2", &carried).unwrap();
    // a legacy branch whose work the ref does not hold
    g(&fx.root, &["switch", "-q", "-c", "orch/demo-M1"]);
    commit_on(&fx.root, "branch-work.txt");
    g(&fx.root, &["switch", "-q", "orch/demo"]);
    refs::write(&fx.root, "refs/orch/orch-demo/M1", "main").unwrap();
    Fx::all_merged(&mut store);
    let lines = finish::run(&mut store, &fx.root, &opts(true, None)).unwrap();
    assert!(
        lines.contains(&"PICKED UP legacy branch orch/demo-M2: its work was already in refs/orch/orch-demo/M2, and the branch is gone".to_string()),
        "{lines:?}"
    );
    assert!(!git::branch_exists(&fx.root, "orch/demo-M2").unwrap());
    assert!(
        lines.iter().any(|l| l.starts_with(
            "KEPT legacy branch orch/demo-M1: refs/orch/orch-demo/M1 holds other saved work"
        )),
        "{lines:?}"
    );
    assert!(git::branch_exists(&fx.root, "orch/demo-M1").unwrap());
}

#[test]
fn a_run_without_a_recorded_base_falls_back_to_the_default_branch() {
    let t = tempfile::tempdir().unwrap();
    let origin = bare(t.path());
    let (fx, mut store) = Fx::new(None, "feature/demo", "per-milestone", "true");
    g(&fx.root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    g(&fx.root, &["push", "-q", "origin", "main"]);
    g(&fx.root, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
    worktree::ensure_excluded(&fx.root).unwrap();
    g(&fx.root, &["switch", "-q", "-c", "feature/demo"]);
    commit_on(&fx.root, "m1.txt");
    Fx::all_merged(&mut store);
    let lines = finish::run(&mut store, &fx.root, &opts(true, None)).unwrap();
    assert!(
        lines.contains(&"NOT PUSHED (--no-push): push feature/demo and open a PR into main when ready".to_string()),
        "{lines:?}"
    );
}
