//! `orch commit` and `orch merge` through the real binary in a real temporary repo: output lines,
//! exit codes, what ends up in git, and recovery of a merge killed halfway (`ORCH_FAULT`).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLAN: &str = "\
# Plan: Demo
Branch: feature/demo
Base: main
Check command: CHECK
Workers: claude-1 (claude: sonnet), claude-2 (claude: sonnet)
Reviewer: none
Setup command: SETUP
Isolation: ISOLATION

## [x] M1: title M1
Depends on: none
Tier: medium
Model why: normal work
Goal:
- g
Acceptance criteria:
- works

## [x] M2: title M2
Depends on: none
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

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

impl Sandbox {
    fn new(isolation: &str, check: &str) -> Sandbox {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let (root, home) = (base.join("repo"), base.join("home"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
        let plan = PLAN.replace("SETUP", "none").replace("CHECK", check).replace("ISOLATION", isolation);
        std::fs::write(root.join(".orchestrate/plan.md"), plan).unwrap();
        let sb = Sandbox { _t: t, root, home };
        let o = sb.orch(&["sync"]);
        assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
        sb
    }

    fn orch(&self, args: &[&str]) -> Output {
        self.orch_env(args, &[])
    }

    fn orch_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_orch"))
            .envs(env.iter().copied())
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("HERDR", self.home.join("no-herdr"))
            .env_remove("ORCH_AGENTS_FILE")
            .env_remove("ORCH_MODELS_FILE")
            .env_remove("ORCH_LOG")
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let o = self.orch(args);
        assert_eq!(o.status.code(), Some(0), "orch {args:?}: {}{}", out(&o), err(&o));
        out(&o)
    }

    fn wt(&self, w: &str) -> PathBuf {
        self.root.parent().unwrap().join("repo-wt").join(w)
    }
}


impl Sandbox {
    /// M1 (or `tid`) taken by `worker`, a file written, and the coder is done.
    fn coded(&self, tid: &str, worker: &str, file: &str, body: &str) {
        self.ok(&["prepare", tid, worker]);
        std::fs::write(self.wt(worker).join(file), body).unwrap();
        self.ok(&["set", tid, "verifying", "--force"]);
    }

    fn tip(&self) -> String {
        git(&self.root, &["rev-parse", "feature/demo"])
    }
}

#[test]
fn commit_then_merge_lands_one_commit_after_the_check() {
    let sb = Sandbox::new("worktree", "true");
    sb.ok(&["start"]);
    sb.coded("M1", "claude-1", "new.txt", "n\n");
    let r = "refs/orch/feature-demo/M1";
    assert_eq!(sb.ok(&["commit", "M1"]).trim(), format!("COMMITTED M1 (saved in {r})"));
    assert_eq!(sb.ok(&["merge", "M1"]).trim(), "MERGED M1 into feature/demo");
    assert_eq!(git(&sb.root, &["log", "-1", "--format=%B", "feature/demo"]), "title M1\n\nMilestone: M1");
    assert_eq!(git(&sb.root, &["status", "--porcelain"]), "");
    assert!(sb.ok(&["status"]).contains("merged"));
    // a second merge is refused like Python
    let o = sb.orch(&["merge", "M1"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err(&o).trim(), "ERROR: M1 is merged, expected committed");
}

#[test]
fn merge_validates_like_python() {
    let sb = Sandbox::new("worktree", "true");
    sb.ok(&["start"]);
    let o = sb.orch(&["merge"]);
    assert!(err(&o).contains("usage: orch merge <Mn>"));
    let o = sb.orch(&["merge", "M1"]);
    assert_eq!(err(&o).trim(), "ERROR: M1 is pending, expected committed");
    let o = sb.orch(&["commit", "M1"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("M1 has no saved work (refs/orch/feature-demo/M1)"), "{}", err(&o));
}

#[test]
fn a_check_that_fails_on_the_branch_exits_6_and_commits_nothing() {
    let sb = Sandbox::new("worktree", "! [ -e a1.txt -a -e a2.txt ]");
    sb.ok(&["start"]);
    sb.coded("M1", "claude-1", "a1.txt", "1\n");
    sb.coded("M2", "claude-2", "a2.txt", "2\n");
    sb.ok(&["commit", "M1"]);
    sb.ok(&["commit", "M2"]);
    sb.ok(&["merge", "M1"]);
    let tip = sb.tip();
    let o = sb.orch(&["merge", "M2"]);
    assert_eq!(o.status.code(), Some(6), "{}{}", out(&o), err(&o));
    assert!(out(&o).starts_with("CHECK_FAILED M2 on feature/demo -> fix job queued for claude-2 (`orch send-job M2`); nothing was committed"), "{}", out(&o));
    assert_eq!(sb.tip(), tip);
    assert_eq!(git(&sb.root, &["status", "--porcelain"]), "");
    assert!(sb.ok(&["status"]).contains("needs-fix"));
}

#[test]
fn a_conflict_exits_4_and_leaves_the_tree_clean() {
    let sb = Sandbox::new("worktree", "true");
    sb.ok(&["start"]);
    sb.coded("M1", "claude-1", "a.txt", "one\n");
    sb.coded("M2", "claude-2", "a.txt", "two\n");
    sb.ok(&["commit", "M1"]);
    sb.ok(&["commit", "M2"]);
    sb.ok(&["merge", "M1"]);
    let o = sb.orch(&["merge", "M2"]);
    assert_eq!(o.status.code(), Some(4), "{}{}", out(&o), err(&o));
    assert_eq!(out(&o).trim(), "CONFLICT M2 files=a.txt -> conflict job queued for claude-2 (`orch send-job M2`)");
    assert_eq!(git(&sb.root, &["status", "--porcelain"]), "");
    assert!(sb.ok(&["status"]).contains("conflict"));
}

#[test]
fn risky_work_exits_7_and_approve_lets_it_land() {
    let sb = Sandbox::new("worktree", "true");
    sb.ok(&["start"]);
    sb.ok(&["prepare", "M1", "claude-1"]);
    std::fs::remove_file(sb.wt("claude-1").join("a.txt")).unwrap();
    sb.ok(&["set", "M1", "verifying", "--force"]);
    sb.ok(&["commit", "M1"]);
    let o = sb.orch(&["merge", "M1"]);
    assert_eq!(o.status.code(), Some(7), "{}{}", out(&o), err(&o));
    let t = out(&o);
    assert!(t.contains("HELD M1 for approval — deletes a.txt"), "{t}");
    assert!(t.contains("Look at it, then run `orch approve M1` (or `orch set M1 paused` to stop it)"), "{t}");
    assert!(git(&sb.root, &["show", "feature/demo:a.txt"]).contains('a'));
    assert!(sb.ok(&["approve", "M1"]).contains("APPROVED M1"));
    assert_eq!(sb.ok(&["merge", "M1"]).trim(), "MERGED M1 into feature/demo");
    assert!(git(&sb.root, &["ls-tree", "--name-only", "feature/demo"]).lines().all(|f| f != "a.txt"));
}

#[test]
fn a_merge_killed_after_the_commit_is_finished_by_the_next_start() {
    let sb = Sandbox::new("worktree", "true");
    sb.ok(&["start"]);
    sb.coded("M1", "claude-1", "new.txt", "n\n");
    sb.ok(&["commit", "M1"]);
    let o = sb.orch_env(&["merge", "M1"], &[("ORCH_FAULT", "after-commit")]);
    assert_eq!(o.status.code(), Some(99));
    assert_eq!(git(&sb.root, &["rev-list", "--count", "main..feature/demo"]), "1");
    assert!(sb.ok(&["status"]).contains("committed"), "the database does not know yet");
    let o = sb.ok(&["start"]);
    assert!(o.contains("RECOVERED M1: its commit had landed before the crash"), "{o}");
    assert!(sb.ok(&["status"]).contains("merged"));
    assert_eq!(git(&sb.root, &["rev-list", "--count", "main..feature/demo"]), "1");
    assert_eq!(git(&sb.root, &["status", "--porcelain"]), "");
}

#[test]
fn a_merge_killed_after_the_squash_is_undone_and_redone() {
    let sb = Sandbox::new("worktree", "true");
    sb.ok(&["start"]);
    sb.coded("M1", "claude-1", "new.txt", "n\n");
    sb.ok(&["commit", "M1"]);
    let tip = sb.tip();
    for point in ["after-squash", "after-check"] {
        let o = sb.orch_env(&["merge", "M1"], &[("ORCH_FAULT", point)]);
        assert_eq!(o.status.code(), Some(99), "{point}");
        assert_ne!(git(&sb.root, &["status", "--porcelain"]), "", "{point}: half-merged tree");
        // `orch start` would refuse a dirty tree; recovery runs first and undoes it
        let o = sb.ok(&["start"]);
        assert!(o.contains("RECOVERED M1: undid the half-done merge; it will be merged again from its saved work"), "{point}: {o}");
        assert_eq!(git(&sb.root, &["status", "--porcelain"]), "", "{point}");
        assert_eq!(sb.tip(), tip, "{point}");
    }
    // the next merge repairs on its own too
    let o = sb.orch_env(&["merge", "M1"], &[("ORCH_FAULT", "after-squash")]);
    assert_eq!(o.status.code(), Some(99));
    let o = sb.ok(&["merge", "M1"]);
    assert!(o.contains("RECOVERED M1") && o.contains("MERGED M1 into feature/demo"), "{o}");
    assert_eq!(git(&sb.root, &["rev-list", "--count", "main..feature/demo"]), "1");
}

#[test]
fn shared_mode_commit_lands_in_place_and_merge_has_no_step() {
    let sb = Sandbox::new("shared", "true");
    sb.ok(&["start"]);
    sb.ok(&["prepare", "M1", "claude-1"]);
    std::fs::write(sb.root.join("new.txt"), "n\n").unwrap();
    sb.ok(&["set", "M1", "verifying", "--force"]);
    assert_eq!(sb.ok(&["commit", "M1"]).trim(), "MERGED M1 (shared mode: committed on feature/demo)");
    assert_eq!(git(&sb.root, &["log", "-1", "--format=%B"]), "title M1\n\nMilestone: M1");
    let o = sb.orch(&["merge", "M1"]);
    assert_eq!(err(&o).trim(), "ERROR: shared mode has no merge step (orch commit already landed it)");
}

