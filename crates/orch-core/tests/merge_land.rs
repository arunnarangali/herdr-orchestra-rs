//! Landing milestones on a real repository (§13–§16): squash, check before commit, conflicts,
//! risk hold, and a crash injected at every step of the merge with the journal repairing it.

use orch_core::conflict;
use orch_core::git;
use orch_core::journal::Step;
use orch_core::manager::{CommitOutcome, GitWork};
use orch_core::merge::{self, Fault, FaultPoint, Landed, MergeLock};
use orch_core::refs::{self, task_ref};
use orch_core::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::store::{Counter, Store, TransitionRequest};
use orch_core::task::State;
use orch_core::worktree;
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
    store: Store,
}

const CHECK_BOTH: &str = "if [ -e both_a.txt ] && [ -e both_b.txt ]; then echo CHECK-SAW-BOTH; exit 1; fi";

impl Fx {
    fn new(isolation: &str, check: &str, risk_hold: bool) -> Fx {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir(&root).unwrap();
        g(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "a\nb\nc\n").unwrap();
        std::fs::write(root.join("doomed.txt"), "x\n").unwrap();
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
            title: format!("title of {id}"),
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
                    risk_hold,
                    budget_s: None,
                    check_command: check.into(),
                    setup_command: "none".into(),
                    lead_mode: "off".into(),
                    reviewer: None,
                },
                agents: vec![agent("a"), agent("b")],
                tasks: vec![task("M1"), task("M2")],
            })
            .unwrap();
        worktree::start(&mut store, &root).unwrap();
        Fx { _t: t, root, store }
    }

    fn wt(&self, w: &str) -> PathBuf {
        worktree::worktree_path(&self.root, w)
    }

    /// The coder `worker` takes `task`, writes `files`, and the task is ready to land.
    fn work(&mut self, task: &str, worker: &str, files: &[(&str, &str)]) {
        GitWork::new(self.root.clone()).prepare(task, worker).unwrap();
        self.store.claim(task, worker, None).unwrap();
        for (f, body) in files {
            std::fs::write(self.wt(worker).join(f), body).unwrap();
        }
        self.to(task, State::Verifying);
    }

    fn to(&mut self, task: &str, s: State) {
        self.store.transition(task, &TransitionRequest::auto(s, None)).unwrap();
    }

    fn land(&mut self, task: &str) -> merge::Landing {
        merge::land(&mut self.store, &self.root, task, &Fault::none()).unwrap()
    }

    fn tip(&self) -> String {
        g(&self.root, &["rev-parse", "feature/demo"])
    }

    fn commits(&self) -> usize {
        g(&self.root, &["rev-list", "--count", "main..feature/demo"]).parse().unwrap()
    }

    fn status(&self, task: &str) -> State {
        self.store.get_task(task).unwrap().status
    }

    fn clean(&self) -> bool {
        g(&self.root, &["status", "--porcelain"]).is_empty()
    }

    fn has_ref(&self, task: &str) -> bool {
        refs::read(&self.root, &task_ref("feature/demo", task)).unwrap().is_some()
    }
}

#[test]
fn a_milestone_lands_as_one_commit_with_the_coders_message_and_a_trailer() {
    let mut fx = Fx::new("worktree", "true", true);
    std::fs::create_dir_all(fx.root.join(".orch-task/M1")).unwrap();
    std::fs::write(
        fx.root.join(".orch-task/M1/result-1.json"),
        r#"{"status":"done","commit":"feat(x): add the x thing"}"#,
    )
    .unwrap();
    fx.work("M1", "a", &[("new.txt", "n\n")]);
    let l = fx.land("M1");
    assert_eq!(l.outcome, Landed::Merged { empty: false, shared: false });
    assert_eq!(fx.commits(), 1);
    assert_eq!(
        g(&fx.root, &["log", "-1", "--format=%B", "feature/demo"]),
        "feat(x): add the x thing\n\nMilestone: M1"
    );
    assert_eq!(g(&fx.root, &["show", "feature/demo:new.txt"]), "n");
    assert!(fx.clean());
    assert!(!fx.has_ref("M1"), "the saved work is dropped once landed");
    assert_eq!(fx.status("M1"), State::Merged);
    assert!(fx.store.journal().unwrap().is_none());
    // no wip/orch commit anywhere on the branch, and no branch but the feature branch and main
    assert!(!g(&fx.root, &["log", "--format=%s", "feature/demo"]).contains("wip"));
    assert_eq!(g(&fx.root, &["branch", "--format=%(refname:short)"]).lines().count(), 2);
}

#[test]
fn the_message_falls_back_to_the_report_commit_line_then_the_title() {
    let mut fx = Fx::new("worktree", "true", true);
    std::fs::create_dir_all(fx.root.join(".orch-task/M1")).unwrap();
    std::fs::write(fx.root.join(".orch-task/M1/report-1.md"), "# r\nsome\nCOMMIT: `fix(y): first`\nCOMMIT: fix(y): last one\n").unwrap();
    fx.work("M1", "a", &[("one.txt", "1\n")]);
    fx.land("M1");
    assert_eq!(g(&fx.root, &["log", "-1", "--format=%B", "feature/demo"]), "fix(y): last one\n\nMilestone: M1");
    fx.work("M2", "b", &[("two.txt", "2\n")]);
    fx.land("M2");
    assert_eq!(g(&fx.root, &["log", "-1", "--format=%B", "feature/demo"]), "title of M2\n\nMilestone: M2");
}

#[test]
fn two_milestones_land_in_a_row_one_commit_each() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[("one.txt", "1\n")]);
    fx.work("M2", "b", &[("two.txt", "2\n")]);
    fx.land("M1");
    // M2's saved work is based on the old tip: the squash still goes on top of M1's commit
    let l = fx.land("M2");
    assert_eq!(l.outcome, Landed::Merged { empty: false, shared: false });
    assert_eq!(fx.commits(), 2);
    assert_eq!(g(&fx.root, &["show", "feature/demo:one.txt"]), "1");
    assert_eq!(g(&fx.root, &["show", "feature/demo:two.txt"]), "2");
    assert!(g(&fx.root, &["log", "--format=%s", "feature/demo"]).contains("title of M2"));
    assert!(fx.clean());
}

#[test]
fn a_milestone_that_changes_nothing_lands_without_a_commit() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[]);
    let before = fx.tip();
    assert_eq!(fx.land("M1").outcome, Landed::Merged { empty: true, shared: false });
    assert_eq!(fx.tip(), before);
    assert_eq!(fx.status("M1"), State::Merged);
}

#[test]
fn passes_alone_but_fails_merged_commits_nothing() {
    let mut fx = Fx::new("worktree", CHECK_BOTH, true);
    fx.work("M1", "a", &[("both_a.txt", "a\n")]);
    fx.work("M2", "b", &[("both_b.txt", "b\n")]);
    fx.land("M1");
    let tip = fx.tip();
    let l = fx.land("M2");
    let Landed::BranchFails { tail } = &l.outcome else { panic!("{:?}", l.outcome) };
    assert!(tail.contains("CHECK-SAW-BOTH"), "{tail}");
    assert_eq!(fx.tip(), tip, "nothing was committed");
    assert!(fx.clean(), "the main tree is clean again");
    assert!(fx.store.journal().unwrap().is_none());
    assert_eq!(fx.status("M2"), State::Verifying);
    assert!(fx.has_ref("M2"), "its work is kept for the fix");
    let issue = fx.store.issue("M2").unwrap().unwrap();
    assert!(issue.detail.contains("CHECK-SAW-BOTH"));
    assert!(matches!(l.outcome.outcome("M2"), CommitOutcome::BranchFails { line } if line.contains("CHECK-SAW-BOTH")));
}

#[test]
fn a_hung_check_times_out_and_commits_nothing() {
    let (ok, tail) = merge::run_check("sleep 30", Path::new("."), std::time::Duration::from_millis(150));
    assert!(!ok);
    assert!(tail.contains("timed out"), "{tail}");
    let (ok, tail) = merge::run_check("for i in $(seq 1 100); do echo line$i; done; exit 2", Path::new("."), std::time::Duration::from_secs(30));
    assert!(!ok);
    assert_eq!(tail.lines().count(), 60);
    assert!(tail.ends_with("line100"));
}

#[test]
fn a_conflict_leaves_the_tree_clean_stores_the_files_and_the_job_text() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[("a.txt", "a\nONE\nc\n")]);
    fx.work("M2", "b", &[("a.txt", "a\nTWO\nc\n")]);
    fx.land("M1");
    let tip = fx.tip();
    let l = fx.land("M2");
    assert_eq!(l.outcome, Landed::Conflict { files: vec!["a.txt".into()] });
    assert_eq!(fx.tip(), tip);
    assert!(fx.clean());
    assert!(fx.store.journal().unwrap().is_none());
    assert!(fx.has_ref("M2"));
    let issue = fx.store.issue("M2").unwrap().unwrap();
    assert_eq!(issue.files, ["a.txt"]);
    assert!(!issue.rereview);
    let body = conflict::job_body("M2", "title of M2", "feature/demo", "true", &issue.files);
    assert!(body.contains("conflict with yours in: a.txt") && body.contains("git merge --no-commit feature/demo"));
    assert!(matches!(l.outcome.outcome("M2"), CommitOutcome::Conflict { line } if line.contains("a.txt")));
}

#[test]
fn a_conflict_after_two_resolution_rounds_is_unresolved() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[("a.txt", "a\nONE\nc\n")]);
    fx.work("M2", "b", &[("a.txt", "a\nTWO\nc\n")]);
    fx.land("M1");
    fx.store.bump("M2", Counter::Conflict).unwrap();
    fx.store.bump("M2", Counter::Conflict).unwrap();
    let l = fx.land("M2");
    assert_eq!(l.outcome, Landed::Unresolved { files: vec!["a.txt".into()] });
    assert!(fx.clean());
    assert!(matches!(l.outcome.outcome("M2"), CommitOutcome::Conflict { line } if line.contains("unresolved after 2")));
}

#[test]
fn an_approved_milestone_that_conflicts_is_marked_for_re_review_with_the_resolution_patch() {
    let mut fx = Fx::new("worktree", "true", true);
    rusqlite::Connection::open(fx.store.path())
        .unwrap()
        .execute("UPDATE task_plan SET review=1 WHERE task_id='M2'", [])
        .unwrap();
    fx.work("M1", "a", &[("a.txt", "a\nONE\nc\n")]);
    fx.work("M2", "b", &[("a.txt", "a\nTWO\nc\n")]);
    // M2 went through the reviewer and was approved
    fx.to("M2", State::Reviewing);
    fx.to("M2", State::Reviewed);
    fx.land("M1");
    let l = fx.land("M2");
    assert!(matches!(l.outcome, Landed::Conflict { .. }));
    let issue = fx.store.issue("M2").unwrap().unwrap();
    assert!(issue.rereview);
    let before = refs::read(&fx.root, &task_ref("feature/demo", "M2")).unwrap().unwrap();
    assert_eq!(issue.resolution_base.as_deref(), Some(before.as_str()));
    // the coder resolves in its folder: the saved work moves on
    std::fs::write(fx.wt("b").join("a.txt"), "a\nONE\nTWO\nc\n").unwrap();
    worktree::save_task_work(&fx.store, &fx.root, "M2", "resolved").unwrap();
    let patches = conflict::write_review_patches(&fx.store, &fx.root, "feature/demo", "M2").unwrap();
    assert!(patches.diff && patches.resolution);
    let dir = fx.root.join(".orchestrate/review/M2");
    let resolution = std::fs::read_to_string(dir.join("resolution.patch")).unwrap();
    assert!(resolution.contains("+ONE") && resolution.contains("a.txt"), "{resolution}");
    assert!(std::fs::read_to_string(dir.join("diff.patch")).unwrap().contains("+TWO"));
    fx.store.clear_rereview("M2").unwrap();
    assert!(!conflict::write_review_patches(&fx.store, &fx.root, "feature/demo", "M2").unwrap().resolution);
}

#[test]
fn risky_changes_are_held_unless_risk_hold_is_off_or_the_milestone_was_approved() {
    let mut fx = Fx::new("worktree", "true", true);
    std::fs::create_dir_all(fx.wt("a")).unwrap();
    fx.work("M1", "a", &[("migrations_x.txt", "ok\n")]);
    std::fs::remove_file(fx.wt("a").join("doomed.txt")).unwrap();
    let tip = fx.tip();
    let Landed::Held { risks } = fx.land("M1").outcome else { panic!() };
    assert_eq!(risks, ["deletes doomed.txt"]);
    assert_eq!(fx.tip(), tip);
    assert!(fx.clean() && fx.store.journal().unwrap().is_none());
    // the developer approves it: held -> committed, then it lands
    fx.to("M1", State::Held);
    fx.to("M1", State::Committed);
    assert!(matches!(fx.land("M1").outcome, Landed::Merged { .. }));
    assert!(git::run(&fx.root, &["cat-file", "-e", "feature/demo:doomed.txt"]).unwrap().status != 0);
    // and with `Risk hold: off` it never stops
    let mut fx = Fx::new("worktree", "true", false);
    fx.work("M1", "a", &[]);
    std::fs::remove_file(fx.wt("a").join("doomed.txt")).unwrap();
    assert!(matches!(fx.land("M1").outcome, Landed::Merged { .. }));
}

#[test]
fn the_risk_rules() {
    let (_t, root) = {
        let fx = Fx::new("worktree", "true", true);
        (fx._t, fx.root)
    };
    let s = |hold| RunSettings {
        branch: "feature/demo".into(),
        base: None,
        title: String::new(),
        commits: "per-milestone".into(),
        isolation: "worktree".into(),
        max_parallel: 1,
        setup_inputs: vec![],
        scope: "warn".into(),
        risk_hold: hold,
        budget_s: None,
        check_command: String::new(),
        setup_command: "none".into(),
        lead_mode: "off".into(),
        reviewer: None,
    };
    g(&root, &["switch", "-q", "-c", "scratch"]);
    for (p, body) in [
        ("db/migrations/001.sql", "x\n"),
        (".github/workflows/ci.yml", "x\n"),
        (".env", "S=1\n"),
        (".env.example", "S=\n"),
        ("keys/server.pem", "x\n"),
        ("Cargo.lock", "x\n"),
        ("pkg/package.json", "{}\n"),
    ] {
        std::fs::create_dir_all(root.join(p).parent().unwrap()).unwrap();
        std::fs::write(root.join(p), body).unwrap();
    }
    g(&root, &["add", "-A"]);
    g(&root, &["commit", "-q", "-m", "risky"]);
    let likely = vec!["pkg/package.json".to_string()];
    let flags = merge::risk_flags(&root, &s(true), &likely, &["main", "scratch"], 1500).unwrap();
    for want in [
        "database migration: db/migrations/001.sql",
        "CI configuration: .github/workflows/ci.yml",
        "secrets or environment file: .env",
        "secrets or environment file: keys/server.pem",
        "dependency change not listed in Likely files: Cargo.lock",
    ] {
        assert!(flags.iter().any(|f| f == want), "{want} in {flags:?}");
    }
    assert!(!flags.iter().any(|f| f.contains(".env.example") || f.contains("pkg/package.json")), "{flags:?}");
    let big = merge::risk_flags(&root, &s(true), &likely, &["main", "scratch"], 3).unwrap();
    assert!(big.iter().any(|f| f.starts_with("large change: ") && f.ends_with("(limit ORCH_RISK_MAX_LINES=3)")), "{big:?}");
    assert!(merge::risk_flags(&root, &s(false), &likely, &["main", "scratch"], 3).unwrap().is_empty());
}

// ---------------------------------------------------------------- crash recovery

/// Run a landing with `point` injected, then check what the crash left.
fn crash(fx: &mut Fx, task: &str, point: FaultPoint) {
    let e = merge::land(&mut fx.store, &fx.root, task, &Fault::at(point)).unwrap_err();
    assert_eq!(e.exit_code(), merge::FAULT_EXIT, "{e}");
}

#[test]
fn a_crash_after_the_squash_is_undone_and_the_milestone_is_merged_again() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[("new.txt", "n\n")]);
    let tip = fx.tip();
    crash(&mut fx, "M1", FaultPoint::AfterSquash);
    assert!(!fx.clean(), "the half-done squash is in the tree");
    let j = fx.store.journal().unwrap().unwrap();
    assert_eq!((j.task.as_str(), j.step, j.head_before.as_str()), ("M1", Step::Squashed, tip.as_str()));
    assert_eq!(fx.tip(), tip);
    let r = merge::recover(&mut fx.store, &fx.root).unwrap().unwrap();
    assert_eq!(r, merge::Recovery::Undone("M1".into()));
    assert!(fx.clean());
    assert_eq!(fx.tip(), tip);
    assert!(fx.store.journal().unwrap().is_none());
    assert!(fx.has_ref("M1"));
    assert_eq!(fx.status("M1"), State::Verifying);
    assert_eq!(fx.land("M1").outcome, Landed::Merged { empty: false, shared: false });
    assert_eq!(fx.commits(), 1);
}

#[test]
fn a_crash_after_the_check_but_before_the_commit_is_undone() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[("new.txt", "n\n")]);
    let tip = fx.tip();
    crash(&mut fx, "M1", FaultPoint::AfterCheck);
    assert_eq!(fx.store.journal().unwrap().unwrap().step, Step::Checked);
    // the next landing request repairs first, then lands (no separate recover call needed)
    let l = fx.land("M1");
    assert_eq!(l.recovered, Some(merge::Recovery::Undone("M1".into())));
    assert_eq!(l.outcome, Landed::Merged { empty: false, shared: false });
    assert_eq!(fx.commits(), 1);
    assert_ne!(fx.tip(), tip);
    assert!(fx.clean());
}

#[test]
fn a_crash_after_the_commit_is_finished_not_redone() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[("new.txt", "n\n")]);
    crash(&mut fx, "M1", FaultPoint::AfterCommit);
    assert_eq!(fx.commits(), 1, "the commit landed");
    assert_eq!(fx.status("M1"), State::Verifying, "but the database does not know");
    assert!(fx.has_ref("M1"));
    let r = merge::recover(&mut fx.store, &fx.root).unwrap().unwrap();
    assert_eq!(r, merge::Recovery::Finished("M1".into()));
    assert_eq!(fx.status("M1"), State::Merged);
    assert!(!fx.has_ref("M1"));
    assert!(fx.store.journal().unwrap().is_none());
    assert_eq!(fx.commits(), 1, "not merged twice");
    assert!(fx.clean());
    // nothing left to repair
    assert!(merge::recover(&mut fx.store, &fx.root).unwrap().is_none());
}

#[test]
fn a_crash_just_before_the_journal_is_cleared_is_finished() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[("new.txt", "n\n")]);
    crash(&mut fx, "M1", FaultPoint::BeforeClear);
    let j = fx.store.journal().unwrap().unwrap();
    assert_eq!(j.step, Step::Committed);
    assert_eq!(j.commit.as_deref(), Some(fx.tip().as_str()));
    assert!(!fx.has_ref("M1"));
    // a land request for the same milestone just reports it merged
    let l = fx.land("M1");
    assert_eq!(l.recovered, Some(merge::Recovery::Finished("M1".into())));
    assert_eq!(l.outcome, Landed::Merged { empty: false, shared: false });
    assert_eq!(fx.status("M1"), State::Merged);
    assert_eq!(fx.commits(), 1);
}

#[test]
fn recovery_refuses_to_undo_when_the_saved_work_is_gone() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[("new.txt", "n\n")]);
    crash(&mut fx, "M1", FaultPoint::AfterSquash);
    // pretend the journaled source vanished
    rusqlite::Connection::open(fx.store.path())
        .unwrap()
        .execute("UPDATE merge_journal SET src='1111111111111111111111111111111111111111'", [])
        .unwrap();
    let e = merge::recover(&mut fx.store, &fx.root).unwrap_err();
    assert!(e.to_string().contains("is gone"), "{e}");
    assert!(!fx.clean(), "nothing was thrown away");
}

#[test]
fn a_journal_whose_branch_moved_on_its_own_is_cleared_without_touching_it() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[("new.txt", "n\n")]);
    crash(&mut fx, "M1", FaultPoint::AfterSquash);
    g(&fx.root, &["reset", "-q", "--hard", "HEAD"]);
    std::fs::write(fx.root.join("other.txt"), "o\n").unwrap();
    g(&fx.root, &["add", "-A"]);
    g(&fx.root, &["commit", "-q", "-m", "someone else"]);
    let tip = fx.tip();
    assert_eq!(merge::recover(&mut fx.store, &fx.root).unwrap(), Some(merge::Recovery::Cleared("M1".into())));
    assert_eq!(fx.tip(), tip);
    assert_eq!(fx.status("M1"), State::Verifying);
}

// ---------------------------------------------------------------- one merge at a time

#[test]
fn a_second_merge_while_one_runs_is_refused_with_a_clear_line() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[("new.txt", "n\n")]);
    let held = MergeLock::acquire(&fx.root).unwrap();
    let e = merge::land(&mut fx.store, &fx.root, "M1", &Fault::none()).unwrap_err();
    assert_eq!(e.exit_code(), 3);
    assert!(e.to_string().contains("another merge is in progress"), "{e}");
    // a running merge is not "recovered" by a second process
    assert!(merge::recover(&mut fx.store, &fx.root).unwrap().is_none());
    drop(held);
    assert!(matches!(fx.land("M1").outcome, Landed::Merged { .. }));
}

#[test]
fn only_one_journal_row_can_exist() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.store.journal_begin("M1", "abc", "def").unwrap();
    let e = fx.store.journal_begin("M2", "abc", "def").unwrap_err();
    assert!(e.to_string().contains("merge of M1 is already in progress"), "{e}");
}

#[test]
fn a_milestone_that_cannot_become_merged_is_refused_before_anything_changes() {
    let mut fx = Fx::new("worktree", "true", true);
    let tip = fx.tip();
    let e = merge::land(&mut fx.store, &fx.root, "M1", &Fault::none()).unwrap_err();
    assert!(e.to_string().contains("pending"), "{e}");
    assert_eq!(fx.tip(), tip);
    assert!(fx.store.journal().unwrap().is_none());
}

#[test]
fn a_dirty_main_tree_refuses_the_merge() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[("new.txt", "n\n")]);
    std::fs::write(fx.root.join("a.txt"), "dirty\n").unwrap();
    let e = merge::land(&mut fx.store, &fx.root, "M1", &Fault::none()).unwrap_err();
    assert_eq!(e.to_string(), "main tree must be clean and on feature/demo");
    assert_eq!(std::fs::read_to_string(fx.root.join("a.txt")).unwrap(), "dirty\n");
    assert!(fx.store.journal().unwrap().is_none());
}

// ---------------------------------------------------------------- Work::commit and shared mode

#[test]
fn work_commit_reports_merged_held_and_conflict_from_this_code() {
    let mut fx = Fx::new("worktree", "true", true);
    fx.work("M1", "a", &[("a.txt", "a\nONE\nc\n")]);
    fx.work("M2", "b", &[("a.txt", "a\nTWO\nc\n")]);
    let w = GitWork::new(fx.root.clone());
    assert_eq!(w.commit("M1").unwrap(), CommitOutcome::Merged);
    assert!(matches!(w.commit("M2").unwrap(), CommitOutcome::Conflict { line } if line.contains("a.txt")));
    assert!(w.commit("M9").is_err());
}

#[test]
fn shared_mode_commits_in_place_and_holds_risky_work_unstaged() {
    let mut fx = Fx::new("shared", "true", true);
    fx.store.claim("M1", "a", None).unwrap();
    fx.to("M1", State::Verifying);
    std::fs::remove_file(fx.root.join("doomed.txt")).unwrap();
    let Landed::Held { risks } = fx.land("M1").outcome else { panic!() };
    assert_eq!(risks, ["deletes doomed.txt"]);
    assert!(!fx.root.join("doomed.txt").exists(), "the work stays in the tree");
    assert_eq!(g(&fx.root, &["diff", "--cached", "--name-only"]), "", "but unstaged");
    fx.to("M1", State::Held);
    fx.to("M1", State::Committed);
    assert_eq!(fx.land("M1").outcome, Landed::Merged { empty: false, shared: true });
    assert_eq!(fx.commits(), 1);
    assert_eq!(g(&fx.root, &["log", "-1", "--format=%B", "feature/demo"]), "title of M1\n\nMilestone: M1");
    assert!(fx.clean());
}
