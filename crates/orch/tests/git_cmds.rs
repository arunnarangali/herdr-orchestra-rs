//! `orch start`, `prepare`, `setup-done`, `park` and `cleanup` through the real binary in a
//! real temporary repo: output lines, exit codes and what ends up in git.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLAN: &str = "\
# Plan: Demo
Branch: feature/demo
Base: main
Check command: true
Workers: claude-1 (claude: sonnet), claude-2 (claude: sonnet)
Reviewer: none
Setup command: SETUP
Isolation: ISOLATION

## [x] M1: One
Depends on: none
Tier: medium
Model why: normal work
Goal:
- g
Acceptance criteria:
- works

## [x] M2: Two
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
    fn new(isolation: &str, setup: &str) -> Sandbox {
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
        let plan = PLAN.replace("SETUP", setup).replace("ISOLATION", isolation);
        std::fs::write(root.join(".orchestrate/plan.md"), plan).unwrap();
        let sb = Sandbox { _t: t, root, home };
        let o = sb.orch(&["sync"]);
        assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
        sb
    }

    fn orch(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_orch"))
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

#[test]
fn start_creates_the_branch_and_worktrees_and_is_safe_to_rerun() {
    let sb = Sandbox::new("worktree", "none");
    let o = sb.ok(&["start"]);
    assert!(o.contains(&format!("CREATED worktree {}", sb.wt("claude-1").display())), "{o}");
    assert!(o.contains("BRANCH feature/demo (base main)"), "{o}");
    assert!(o.contains(&format!("WORKER claude-1 kind=claude dir={}", sb.wt("claude-1").display())), "{o}");
    assert_eq!(git(&sb.root, &["rev-parse", "--abbrev-ref", "HEAD"]), "feature/demo");
    assert_eq!(git(&sb.root, &["status", "--porcelain"]), "");
    assert_eq!(git(&sb.root, &["branch", "--format=%(refname:short)"]).lines().count(), 2);
    let again = sb.ok(&["start"]);
    assert!(!again.contains("CREATED"), "{again}");
    assert!(again.contains("BRANCH feature/demo (base main)"), "{again}");
}

#[test]
fn start_refuses_a_dirty_tree_with_the_v4_10_1_message() {
    let sb = Sandbox::new("worktree", "none");
    std::fs::write(sb.root.join("a.txt"), "dirty\n").unwrap();
    let o = sb.orch(&["start"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err(&o).trim(), "ERROR: main working tree has uncommitted changes — commit or stash first");
}

#[test]
fn park_saves_dirty_work_even_when_the_milestone_is_already_pending() {
    let sb = Sandbox::new("worktree", "none");
    sb.ok(&["start"]);
    let o = sb.ok(&["prepare", "M1", "claude-1"]);
    assert!(o.contains(&format!("DIR {}", sb.wt("claude-1").display())), "{o}");
    assert!(o.contains(&format!("TASKDIR {}/.orch-task/M1", sb.wt("claude-1").display())), "{o}");
    std::fs::write(sb.wt("claude-1").join("work.txt"), "w\n").unwrap();
    assert_eq!(sb.ok(&["park", "M1", "pending", "agent", "stopped"]).trim(), "PARKED M1 as pending: agent stopped");
    assert_eq!(git(&sb.wt("claude-1"), &["status", "--porcelain"]), "");
    let r = "refs/orch/feature-demo/M1";
    assert_eq!(git(&sb.root, &["show", &format!("{r}:work.txt")]), "w");
    // parked again while already pending, with new files left behind by the coder: saved too
    std::fs::write(sb.wt("claude-1").join("late.txt"), "l\n").unwrap();
    sb.ok(&["park", "M1", "pending", "again"]);
    assert_eq!(git(&sb.wt("claude-1"), &["status", "--porcelain"]), "");
    assert_eq!(git(&sb.root, &["show", &format!("{r}:late.txt")]), "l");
    assert_eq!(git(&sb.root, &["show", &format!("{r}:work.txt")]), "w");
    // hidden: not a branch
    assert!(!git(&sb.root, &["branch", "--list"]).contains("orch"));
}

#[test]
fn park_that_cannot_save_changes_nothing_and_says_so() {
    let sb = Sandbox::new("worktree", "none");
    sb.ok(&["start"]);
    sb.ok(&["prepare", "M1", "claude-1"]);
    std::fs::write(sb.wt("claude-1").join("precious.txt"), "p\n").unwrap();
    let gitdir = git(&sb.wt("claude-1"), &["rev-parse", "--absolute-git-dir"]);
    std::fs::write(Path::new(&gitdir).join("index.lock"), "").unwrap();
    let o = sb.orch(&["park", "M1", "paused", "why"]);
    assert_eq!(o.status.code(), Some(2), "{}{}", out(&o), err(&o));
    assert!(err(&o).contains("M1 was NOT parked"), "{}", err(&o));
    assert!(err(&o).contains("still there"), "{}", err(&o));
    assert!(sb.wt("claude-1").join("precious.txt").exists());
    let status = sb.ok(&["status"]);
    assert!(status.contains("M1") && status.contains("coding"), "{status}");
}

#[test]
fn park_validates_its_arguments_like_python() {
    let sb = Sandbox::new("worktree", "none");
    let o = sb.orch(&["park", "M1"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("usage: orch park <Mn> paused|failed|pending [--lost] [reason]"));
    let o = sb.orch(&["park", "M1", "merged"]);
    assert!(err(&o).contains("status must be paused, failed or pending"));
    let o = sb.orch(&["park", "M9", "paused"]);
    assert!(err(&o).contains("unknown task 'M9' — known: M1, M2"), "{}", err(&o));
}

#[test]
fn prepare_refuses_work_that_belongs_to_another_milestone() {
    let sb = Sandbox::new("worktree", "none");
    sb.ok(&["start"]);
    sb.ok(&["prepare", "M1", "claude-1"]);
    std::fs::write(sb.wt("claude-1").join("m1.txt"), "x\n").unwrap();
    let o = sb.orch(&["prepare", "M2", "claude-1"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err(&o).trim(), "ERROR: claude-1 is busy with M1");
    assert!(sb.wt("claude-1").join("m1.txt").exists());
    // freed without parking (the coder's files are still there): the refusal names the owner
    sb.ok(&["set", "M1", "paused"]);
    let o = sb.orch(&["prepare", "M2", "claude-1"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("belong to M1"), "{}", err(&o));
    assert!(sb.wt("claude-1").join("m1.txt").exists());
}

#[test]
fn prepare_validates_status_and_worker() {
    let sb = Sandbox::new("worktree", "none");
    sb.ok(&["start"]);
    let o = sb.orch(&["prepare", "M1", "nobody"]);
    assert_eq!(err(&o).trim(), "ERROR: unknown worker nobody");
    let o = sb.orch(&["prepare", "M1"]);
    assert!(err(&o).contains("usage: orch prepare <Mn> <worker>"));
    sb.ok(&["prepare", "M1", "claude-1"]);
    let o = sb.orch(&["prepare", "M1", "claude-2"]);
    assert_eq!(err(&o).trim(), "ERROR: M1 is coding, expected pending/paused/needs-fix/conflict");
}

#[test]
fn setup_is_needed_first_and_again_only_when_dependency_files_change() {
    let sb = Sandbox::new("worktree", "echo setup");
    sb.ok(&["start"]);
    let cmd = format!("SETUP_NEEDED run `echo setup` in {}, then `orch setup-done claude-1`", sb.wt("claude-1").display());
    let o = sb.ok(&["prepare", "M1", "claude-1"]);
    assert!(o.contains(&cmd) && !o.contains("changed since"), "{o}");
    assert_eq!(sb.ok(&["setup-done", "claude-1"]).trim(), "OK");
    sb.ok(&["park", "M1", "pending", "x"]);
    let o = sb.ok(&["prepare", "M1", "claude-1"]);
    assert!(!o.contains("SETUP_NEEDED"), "unchanged dependency files: {o}");
    sb.ok(&["park", "M1", "pending", "x"]);
    // a dependency change lands on the feature branch
    std::fs::write(sb.root.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
    git(&sb.root, &["commit", "-q", "-am", "dep"]);
    let o = sb.ok(&["prepare", "M1", "claude-1"]);
    assert!(o.contains("(dependency files changed since its last setup)"), "{o}");
    // the main folder's own fingerprint was recorded by `start`, the other coder has its own
    sb.ok(&["setup-done", "claude-1"]);
    let o = sb.ok(&["prepare", "M2", "claude-2"]);
    assert!(o.contains("SETUP_NEEDED") && !o.contains("changed since"), "{o}");
}

#[test]
fn cleanup_removes_clean_worktrees_and_keeps_unsaved_ones() {
    let sb = Sandbox::new("worktree", "none");
    sb.ok(&["start"]);
    std::fs::write(sb.wt("claude-2").join("unsaved.txt"), "u\n").unwrap();
    let o = sb.ok(&["cleanup"]);
    assert!(o.contains(&format!("REMOVED {}", sb.wt("claude-1").display())), "{o}");
    assert!(o.contains(&format!("SKIP {}: has uncommitted changes", sb.wt("claude-2").display())), "{o}");
    assert!(!sb.wt("claude-1").exists());
    assert!(sb.wt("claude-2").join("unsaved.txt").exists());
}

#[test]
fn shared_mode_start_cleanup_and_park() {
    let sb = Sandbox::new("shared", "none");
    let o = sb.ok(&["start"]);
    assert!(!o.contains("CREATED"), "{o}");
    assert!(!sb.wt("claude-1").exists());
    assert_eq!(sb.ok(&["cleanup"]).trim(), "nothing to clean (shared mode)");
    sb.ok(&["prepare", "M1", "claude-1"]);
    std::fs::write(sb.root.join("a.txt"), "edited\n").unwrap();
    sb.ok(&["park", "M1", "paused", "stop"]);
    assert_eq!(git(&sb.root, &["status", "--porcelain"]), "");
    assert_eq!(git(&sb.root, &["show", "refs/orch/feature-demo/M1:a.txt"]), "edited");
    sb.ok(&["prepare", "M1", "claude-1"]);
    assert_eq!(std::fs::read_to_string(sb.root.join("a.txt")).unwrap(), "edited\n");
}
