//! The run's git operations against real temporary repositories (§13): `start`, `prepare`,
//! park (save), `cleanup` and the event manager's `GitWork`, with two worktrees at once, dirty
//! files, a missing hidden ref and a setup change between two `prepare` calls.

use orch_core::git;
use orch_core::manager::GitWork;
use orch_core::refs::{self, RefState, Saved, task_ref};
use orch_core::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::store::{Store, TransitionRequest};
use orch_core::task::State;
use orch_core::worktree::{self, CleanupLine, Removal};
use std::path::{Path, PathBuf};

fn g(dir: &Path, args: &[&str]) -> String {
    let mut a = vec!["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"];
    a.extend(args);
    let out = git::run(dir, &a).unwrap();
    assert!(out.success(), "git {args:?}: {}", out.text());
    out.out().to_string()
}

struct Fx {
    _t: tempfile::TempDir,
    root: PathBuf,
}

impl Fx {
    /// A repo on `main` and a synced run `feature/demo` with coders a and b, tasks M1 and M2.
    fn new(isolation: &str, setup: &str) -> (Fx, Store) {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir(&root).unwrap();
        g(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        g(&root, &["add", "-A"]);
        g(&root, &["commit", "-q", "-m", "init"]);
        let mut store = Store::open_in(&root.join(".orchestrate")).unwrap();
        let agent = |n: &str| RunAgent {
            role: AgentRole::Worker,
            name: n.into(),
            kind: "claude".into(),
            model: None,
            tier: None,
            detect: true,
            pane: None,
        };
        let task = |id: &str| PlanTask {
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
        };
        store
            .apply_sync(&SyncInput {
                settings: RunSettings {
                    branch: "feature/demo".into(),
                    base: Some("main".into()),
                    title: "Demo".into(),
                    commits: "per-milestone".into(),
                    isolation: isolation.into(),
                    max_parallel: 2,
                    setup_inputs: vec![],
                    scope: "warn".into(),
                    risk_hold: true,
                    budget_s: None,
                    check_command: "true".into(),
                    setup_command: setup.into(),
                    lead_mode: "off".into(),
                    reviewer: None,
                },
                agents: vec![agent("a"), agent("b")],
                tasks: vec![task("M1"), task("M2")],
            })
            .unwrap();
        (Fx { _t: t, root }, store)
    }

    fn wt(&self, w: &str) -> PathBuf {
        worktree::worktree_path(&self.root, w)
    }

    fn work(&self) -> GitWork {
        GitWork::new(self.root.clone())
    }
}

fn claim(store: &mut Store, task: &str, worker: &str) {
    store.claim(task, worker, None).unwrap();
}

fn tref(task: &str) -> String {
    task_ref("feature/demo", task)
}

#[test]
fn start_makes_the_branch_and_one_detached_worktree_per_coder_and_is_safe_twice() {
    let (fx, mut store) = Fx::new("worktree", "none");
    let r = worktree::start(&mut store, &fx.root).unwrap();
    assert_eq!((r.branch.as_str(), r.base.as_str()), ("feature/demo", "main"));
    assert_eq!(r.created, vec![fx.wt("a"), fx.wt("b")]);
    assert_eq!(git::current_branch(&fx.root).unwrap(), "feature/demo");
    for w in ["a", "b"] {
        assert!(fx.wt(w).join("a.txt").exists());
        // detached: no per-worker branch
        assert_eq!(git::current_branch(&fx.wt(w)).unwrap(), "HEAD");
    }
    assert_eq!(
        g(&fx.root, &["branch", "--format=%(refname:short)"]).lines().collect::<Vec<_>>(),
        ["feature/demo", "main"]
    );
    // ignored through info/exclude only: status stays clean with the folders present
    std::fs::create_dir_all(fx.root.join(".orch-task/M1")).unwrap();
    std::fs::write(fx.root.join(".orch-task/M1/task.md"), "x").unwrap();
    assert_eq!(g(&fx.root, &["status", "--porcelain"]), "");
    assert!(!fx.root.join(".gitignore").exists());
    // twice: nothing new, nothing broken
    let again = worktree::start(&mut store, &fx.root).unwrap();
    assert!(again.created.is_empty());
    assert_eq!(again.workers.len(), 2);
    let exclude = std::fs::read_to_string(fx.root.join(".git/info/exclude")).unwrap();
    assert_eq!(exclude.matches(".orchestrate/").count(), 1);
    assert_eq!(store.run_settings().unwrap().unwrap().base.as_deref(), Some("main"));
}

#[test]
fn start_refuses_a_dirty_main_tree_and_switches_to_an_existing_branch() {
    let (fx, mut store) = Fx::new("worktree", "none");
    std::fs::write(fx.root.join("a.txt"), "dirty\n").unwrap();
    let e = worktree::start(&mut store, &fx.root).unwrap_err();
    assert_eq!(e.to_string(), "main working tree has uncommitted changes — commit or stash first");
    assert_eq!(g(&fx.root, &["branch", "--format=%(refname:short)"]), "main");
    assert!(!fx.wt("a").exists());
    g(&fx.root, &["checkout", "-q", "--", "a.txt"]);
    g(&fx.root, &["branch", "feature/demo"]);
    worktree::start(&mut store, &fx.root).unwrap();
    assert_eq!(git::current_branch(&fx.root).unwrap(), "feature/demo");
}

#[test]
fn start_without_a_base_records_the_current_branch() {
    let (fx, mut store) = Fx::new("worktree", "none");
    g(&fx.root, &["switch", "-q", "-c", "develop"]);
    // a plan with no `Base:`
    rusqlite::Connection::open(store.path())
        .unwrap()
        .execute("UPDATE run SET base=NULL", [])
        .unwrap();
    let r = worktree::start(&mut store, &fx.root).unwrap();
    assert_eq!(r.base, "develop");
    assert_eq!(store.run_settings().unwrap().unwrap().base.as_deref(), Some("develop"));
}

#[test]
fn two_coders_work_two_milestones_at_once() {
    let (fx, mut store) = Fx::new("worktree", "none");
    worktree::start(&mut store, &fx.root).unwrap();
    let w = fx.work();
    w.prepare("M1", "a").unwrap();
    claim(&mut store, "M1", "a");
    w.prepare("M2", "b").unwrap();
    claim(&mut store, "M2", "b");
    std::fs::write(fx.wt("a").join("one.txt"), "1\n").unwrap();
    std::fs::write(fx.wt("b").join("two.txt"), "2\n").unwrap();
    // saving M1 touches neither M2's folder nor M2's ref
    w.save_work("M1").unwrap();
    assert!(worktree::is_clean(&fx.wt("a")).unwrap());
    assert!(!worktree::is_clean(&fx.wt("b")).unwrap());
    assert_eq!(refs::read_state(&fx.root, &tref("M2")).unwrap(), RefState::Missing);
    w.save_work("M2").unwrap();
    let m1 = refs::read(&fx.root, &tref("M1")).unwrap().unwrap();
    let m2 = refs::read(&fx.root, &tref("M2")).unwrap().unwrap();
    assert_eq!(g(&fx.root, &["show", &format!("{m1}:one.txt")]), "1");
    assert_eq!(g(&fx.root, &["show", &format!("{m2}:two.txt")]), "2");
    assert!(git::run(&fx.root, &["cat-file", "-e", &format!("{m1}:two.txt")]).unwrap().status != 0);
    // and the feature branch still has only the start commit's history
    assert_eq!(g(&fx.root, &["rev-list", "--count", "feature/demo"]), "1");
}

#[test]
fn prepare_starts_from_saved_work_else_the_branch_tip() {
    let (fx, mut store) = Fx::new("worktree", "none");
    worktree::start(&mut store, &fx.root).unwrap();
    let w = fx.work();
    // no saved work: the feature branch tip, even after the branch moved on
    std::fs::write(fx.root.join("landed.txt"), "l\n").unwrap();
    g(&fx.root, &["add", "-A"]);
    g(&fx.root, &["commit", "-q", "-m", "landed"]);
    w.prepare("M1", "a").unwrap();
    assert!(fx.wt("a").join("landed.txt").exists());
    claim(&mut store, "M1", "a");
    std::fs::write(fx.wt("a").join("wip.txt"), "w\n").unwrap();
    w.save_work("M1").unwrap();
    store.release_claim("M1").unwrap();
    store.transition("M1", &TransitionRequest::auto(State::Pending, Some("parked"))).unwrap();
    // another coder picks M1 up from its saved work
    w.prepare("M1", "b").unwrap();
    assert_eq!(std::fs::read_to_string(fx.wt("b").join("wip.txt")).unwrap(), "w\n");
    // a milestone with no saved work starts at the tip
    w.prepare("M2", "a").unwrap();
    assert!(!fx.wt("a").join("wip.txt").exists());
}

#[test]
fn a_missing_or_broken_hidden_ref_falls_back_to_the_branch_with_a_note() {
    let (fx, mut store) = Fx::new("worktree", "none");
    worktree::start(&mut store, &fx.root).unwrap();
    // missing: plain start from the tip
    worktree::prepare_workdir(&mut store, &fx.root, "M1", "a").unwrap();
    // broken: the ref names a commit that does not exist
    let path = fx.root.join(".git").join(tref("M1"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "1111111111111111111111111111111111111111\n").unwrap();
    let p = worktree::prepare_workdir(&mut store, &fx.root, "M1", "a").unwrap();
    assert!(p.notes.iter().any(|n| n.contains("is gone")), "{:?}", p.notes);
    assert_eq!(git::check(&p.dir, &["rev-parse", "HEAD"]).unwrap(), git::check(&fx.root, &["rev-parse", "feature/demo"]).unwrap());
}

#[test]
fn prepare_refuses_dirty_work_of_another_milestone_and_names_it() {
    let (fx, mut store) = Fx::new("worktree", "none");
    worktree::start(&mut store, &fx.root).unwrap();
    let w = fx.work();
    w.prepare("M1", "a").unwrap();
    claim(&mut store, "M1", "a");
    std::fs::write(fx.wt("a").join("mine.txt"), "m1's\n").unwrap();
    // M2 is prepared on a's folder while M1 still holds it
    let e = w.prepare("M2", "a").unwrap_err();
    assert!(e.contains("belong to M1"), "{e}");
    assert_eq!(std::fs::read_to_string(fx.wt("a").join("mine.txt")).unwrap(), "m1's\n");
    // M1 parked: its work is saved, then M2 may use the folder
    w.save_work("M1").unwrap();
    store.release_claim("M1").unwrap();
    store.transition("M1", &TransitionRequest::auto(State::Paused, Some("x"))).unwrap();
    w.prepare("M2", "a").unwrap();
    assert!(!fx.wt("a").join("mine.txt").exists());
    assert!(refs::read(&fx.root, &tref("M1")).unwrap().is_some());
}

#[test]
fn dirty_files_of_the_same_milestone_are_saved_not_refused_or_wiped() {
    // the restart case from docs/bug-report-restart-recovery.md: the milestone was parked, the
    // coder kept working, then the milestone is dispatched again
    let (fx, mut store) = Fx::new("worktree", "none");
    worktree::start(&mut store, &fx.root).unwrap();
    let w = fx.work();
    w.prepare("M1", "a").unwrap();
    claim(&mut store, "M1", "a");
    std::fs::write(fx.wt("a").join("first.txt"), "1\n").unwrap();
    w.save_work("M1").unwrap();
    store.release_claim("M1").unwrap();
    store.transition("M1", &TransitionRequest::auto(State::Pending, Some("parked"))).unwrap();
    std::fs::write(fx.wt("a").join("late.txt"), "late\n").unwrap();
    // parking an already-pending milestone still saves (nothing was lost)
    assert!(matches!(
        worktree::save_task_work(&store, &fx.root, "M1", "again").unwrap(),
        Saved::Saved(_)
    ));
    assert!(worktree::is_clean(&fx.wt("a")).unwrap());
    // and a later dirty tree is saved by prepare itself
    std::fs::write(fx.wt("a").join("later.txt"), "later\n").unwrap();
    let p = worktree::prepare_workdir(&mut store, &fx.root, "M1", "a").unwrap();
    assert!(p.notes.iter().any(|n| n.starts_with("SAVED M1")), "{:?}", p.notes);
    for f in ["first.txt", "late.txt", "later.txt"] {
        assert!(fx.wt("a").join(f).exists(), "{f} must survive");
    }
}

#[test]
fn dirty_work_nobody_owns_is_refused() {
    let (fx, mut store) = Fx::new("worktree", "none");
    worktree::start(&mut store, &fx.root).unwrap();
    std::fs::write(fx.wt("a").join("stray.txt"), "s\n").unwrap();
    let e = worktree::prepare_workdir(&mut store, &fx.root, "M1", "a").unwrap_err();
    assert!(e.to_string().contains("no milestone owns"), "{e}");
    assert!(fx.wt("a").join("stray.txt").exists());
}

#[test]
fn setup_runs_first_then_only_when_the_dependency_files_change() {
    let (fx, mut store) = Fx::new("worktree", "echo ran >> \"$PWD/../setup-ran.log\"");
    worktree::start(&mut store, &fx.root).unwrap();
    let log = fx.wt("a").parent().unwrap().join("setup-ran.log");
    let runs = || std::fs::read_to_string(&log).map(|s| s.lines().count()).unwrap_or(0);
    let w = fx.work();
    w.prepare("M1", "a").unwrap();
    assert_eq!(runs(), 1, "first prepare in a fresh worktree sets it up");
    w.prepare("M1", "a").unwrap();
    assert_eq!(runs(), 1, "unchanged dependency files: no second run");
    // source changes landing on the branch do not count
    std::fs::write(fx.root.join("a.txt"), "changed\n").unwrap();
    g(&fx.root, &["commit", "-q", "-am", "src"]);
    w.prepare("M1", "a").unwrap();
    assert_eq!(runs(), 1);
    // a dependency change landing on the branch does, and only in the folders that missed it
    std::fs::write(fx.root.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
    g(&fx.root, &["commit", "-q", "-am", "dep"]);
    w.prepare("M1", "a").unwrap();
    assert_eq!(runs(), 2);
    w.prepare("M1", "a").unwrap();
    assert_eq!(runs(), 2);
    // the other coder's worktree is its own folder: it has its own first run
    w.prepare("M2", "b").unwrap();
    assert_eq!(runs(), 3);
    // the fingerprint is in the database: a fresh Store (a restart) sees it
    drop(store);
    let store = Store::open_in(&fx.root.join(".orchestrate")).unwrap();
    assert!(store.setup_fingerprint(&fx.wt("a")).unwrap().is_some());
    w.prepare("M1", "a").unwrap();
    assert_eq!(runs(), 3);
}

#[test]
fn a_failing_setup_makes_prepare_fail_and_is_retried() {
    let (fx, mut store) = Fx::new("worktree", "exit 1");
    worktree::start(&mut store, &fx.root).unwrap();
    let e = fx.work().prepare("M1", "a").unwrap_err();
    assert!(e.contains("setup command failed"), "{e}");
    assert_eq!(store.setup_fingerprint(&fx.wt("a")).unwrap(), None);
}

#[test]
fn cleanup_removes_clean_worktrees_and_keeps_dirty_ones() {
    let (fx, mut store) = Fx::new("worktree", "none");
    worktree::start(&mut store, &fx.root).unwrap();
    std::fs::write(fx.wt("b").join("unsaved.txt"), "u\n").unwrap();
    let lines = worktree::cleanup(&mut store, &fx.root).unwrap().unwrap();
    assert_eq!(lines, vec![CleanupLine::Removed(fx.wt("a")), CleanupLine::Skipped(fx.wt("b"))]);
    assert_eq!(lines[1].to_string(), format!("SKIP {}: has uncommitted changes", fx.wt("b").display()));
    assert!(!fx.wt("a").exists());
    assert_eq!(std::fs::read_to_string(fx.wt("b").join("unsaved.txt")).unwrap(), "u\n");
    assert_eq!(worktree::remove(&fx.root, "a").unwrap(), Removal::Absent);
}

#[test]
fn shared_mode_parks_the_main_folder_into_the_ref_and_prepare_restores_it() {
    let (fx, mut store) = Fx::new("shared", "none");
    worktree::start(&mut store, &fx.root).unwrap();
    assert!(!fx.wt("a").exists(), "no worktrees in shared mode");
    let w = fx.work();
    w.prepare("M1", "a").unwrap();
    claim(&mut store, "M1", "a");
    std::fs::write(fx.root.join("a.txt"), "edited\n").unwrap();
    std::fs::write(fx.root.join("new.txt"), "n\n").unwrap();
    w.save_work("M1").unwrap();
    assert_eq!(g(&fx.root, &["status", "--porcelain"]), "");
    assert!(refs::read(&fx.root, &tref("M1")).unwrap().is_some());
    store.release_claim("M1").unwrap();
    store.transition("M1", &TransitionRequest::auto(State::Pending, Some("parked"))).unwrap();
    // someone else's uncommitted edits in the main folder block restoring: refused, nothing wiped
    std::fs::write(fx.root.join("a.txt"), "other\n").unwrap();
    let e = w.prepare("M1", "a").unwrap_err();
    assert!(e.contains("can't be restored"), "{e}");
    assert_eq!(std::fs::read_to_string(fx.root.join("a.txt")).unwrap(), "other\n");
    g(&fx.root, &["checkout", "-q", "--", "a.txt"]);
    w.prepare("M1", "a").unwrap();
    assert_eq!(std::fs::read_to_string(fx.root.join("a.txt")).unwrap(), "edited\n");
    assert_eq!(std::fs::read_to_string(fx.root.join("new.txt")).unwrap(), "n\n");
    // a milestone no one owns may not start on a dirty main folder
    let e = w.prepare("M2", "a").unwrap_err();
    assert!(e.contains("no milestone owns"), "{e}");
}
