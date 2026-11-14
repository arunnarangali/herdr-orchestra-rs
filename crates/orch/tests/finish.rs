//! `orch finish [--no-push]` and `orch start`'s legacy pickup through the real binary in a
//! real temporary repo: output lines, exit codes and what ends up in git (a local bare repo
//! as `origin`, a fake `gh`; no test reaches a real remote).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLAN: &str = "\
# Plan: Demo
Branch: BRANCH
Base: main
Check command: CHECK
Commits: COMMITS
Workers: claude-1 (claude: sonnet), claude-2 (claude: sonnet)
Reviewer: none
Isolation: worktree

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
    /// A repo on `main` with a synced plan; `extra` is put first on the child's PATH.
    fn new(branch: &str, check: &str, commits: &str) -> Sandbox {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let (root, home) = (base.join("repo"), base.join("home"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "t"]);
        git(&root, &["config", "user.email", "t@t"]);
        std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
        let plan = PLAN
            .replace("BRANCH", branch)
            .replace("CHECK", check)
            .replace("COMMITS", commits);
        std::fs::write(root.join(".orchestrate/plan.md"), plan).unwrap();
        let sb = Sandbox { _t: t, root, home };
        let o = sb.orch(&["sync"]);
        assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
        sb
    }

    fn orch(&self, args: &[&str]) -> Output {
        self.orch_with(args, None)
    }

    fn orch_with(&self, args: &[&str], extra_path: Option<&Path>) -> Output {
        let mut path = std::env::var("PATH").unwrap_or_default();
        if let Some(p) = extra_path {
            path = format!("{}:{}", p.display(), path);
        }
        Command::new(env!("CARGO_BIN_EXE_orch"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("HERDR", self.home.join("no-herdr"))
            .env("PATH", path)
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

/// A `gh` script that logs its arguments and answers with `reply` (exit `code`).
fn fake_gh(dir: &Path, log: &Path, reply: &str, code: i32) {
    std::fs::write(
        dir.join("gh"),
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> {}\necho '{}'\nexit {}\n",
            log.display(),
            reply,
            code
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(dir.join("gh")).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(dir.join("gh"), perms).unwrap();
}

fn commit(dir: &Path, name: &str) -> String {
    std::fs::write(dir.join(name), format!("{name}\n")).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", name]);
    git(dir, &["rev-parse", "HEAD"])
}

fn bare(base: &Path) -> PathBuf {
    let p = base.join("origin.git");
    git(base, &["init", "-q", "--bare", p.to_str().unwrap()]);
    p
}

#[test]
fn finish_refuses_then_finishes_through_the_binary() {
    let sb = Sandbox::new("feature/demo", "true", "per-milestone");
    // nothing landed yet
    let o = sb.orch(&["finish", "--no-push"]);
    assert_eq!(o.status.code(), Some(2), "{}{}", out(&o), err(&o));
    assert_eq!(
        err(&o).trim(),
        "ERROR: not every milestone has landed: M1 (pending), M2 (pending)"
    );
    sb.ok(&["start"]);
    commit(&sb.root, "m1.txt");
    commit(&sb.root, "m2.txt");
    for m in ["M1", "M2"] {
        sb.ok(&["set", m, "merged", "--force"]);
    }
    let o = sb.ok(&["finish", "--no-push"]);
    assert!(o.contains(&format!("REMOVED {}", sb.wt("claude-1").display())), "{o}");
    assert!(o.contains("CLEAN feature/demo is the only branch this run made (0 hidden ref(s) removed)"), "{o}");
    assert!(o.contains("NOT PUSHED (--no-push): push feature/demo and open a PR into main when ready"), "{o}");
    assert!(!sb.wt("claude-1").exists());
    assert!(!sb.wt("claude-2").exists());
    // the rerun is safe and says the same
    let o = sb.ok(&["finish", "--no-push"]);
    assert!(o.contains("(0 hidden ref(s) removed)"), "{o}");
}

#[test]
fn finish_runs_the_check_first_and_stops_before_any_destructive_step() {
    let sb = Sandbox::new("feature/demo", "echo boom; exit 6", "per-milestone");
    sb.ok(&["start"]);
    sb.ok(&["prepare", "M1", "claude-1"]);
    std::fs::write(sb.wt("claude-1").join("w.txt"), "w\n").unwrap();
    sb.ok(&["park", "M1", "paused", "hold"]);
    for m in ["M1", "M2"] {
        sb.ok(&["set", m, "merged", "--force"]);
    }
    let o = sb.orch(&["finish", "--no-push"]);
    assert_eq!(o.status.code(), Some(2), "{}{}", out(&o), err(&o));
    assert!(
        err(&o).contains("ERROR: the check fails on feature/demo — fix it before finishing:"),
        "{}",
        err(&o)
    );
    assert!(err(&o).contains("boom"), "{}", err(&o));
    // nothing was deleted: the worktree and the hidden ref are still there
    assert!(sb.wt("claude-1").join("w.txt").exists());
    assert_eq!(git(&sb.root, &["show", "refs/orch/feature-demo/M1:w.txt"]), "w");
}

#[test]
fn finish_pushes_the_feature_branch_and_opens_a_pr_with_a_fake_gh() {
    let sb = Sandbox::new("feature/demo", "true", "per-milestone");
    let base = sb.root.parent().unwrap();
    let origin = bare(base);
    let gh_dir = base.join("bin");
    std::fs::create_dir(&gh_dir).unwrap();
    fake_gh(&gh_dir, &base.join("gh.log"), "https://github.com/acme/demo/pull/7", 0);
    sb.ok(&["start"]);
    commit(&sb.root, "m1.txt");
    for m in ["M1", "M2"] {
        sb.ok(&["set", m, "merged", "--force"]);
    }
    // a GitHub-shaped remote whose pushes really go to the local bare repo
    git(&sb.root, &["remote", "add", "origin", "https://github.com/acme/demo.git"]);
    git(&sb.root, &["config", "remote.origin.pushurl", origin.to_str().unwrap()]);
    let o = sb.orch_with(&["finish"], Some(&gh_dir));
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    assert!(out(&o).contains("PUSHED feature/demo to origin"), "{}", out(&o));
    assert_eq!(
        out(&o).lines().last().unwrap(),
        "PR https://github.com/acme/demo/pull/7"
    );
    // the PR is opened into the base, from the plan title, with no attribution
    let call = std::fs::read_to_string(base.join("gh.log")).unwrap();
    for a in ["pr", "create", "--base", "main", "--head", "feature/demo", "--title", "Demo"] {
        assert!(call.contains(a), "{a} missing from {call}");
    }
    assert!(call.contains("--body"), "{call}");
    assert!(!call.contains("Generated with"), "{call}");
    // only the feature branch was pushed
    assert_eq!(git(&origin, &["for-each-ref", "--format=%(refname)"]), "refs/heads/feature/demo");
    // without gh on PATH the compare link is printed instead
    let o = sb.orch(&["finish"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    assert!(
        out(&o).contains("OPEN A PR from feature/demo into main: https://github.com/acme/demo/compare/main...feature/demo?expand=1"),
        "{}",
        out(&o)
    );
}

#[test]
fn start_picks_up_legacy_branches_into_hidden_refs() {
    let sb = Sandbox::new("orch/demo", "true", "per-milestone");
    git(&sb.root, &["switch", "-q", "-c", "orch/demo-M1"]);
    std::fs::write(sb.root.join("old.txt"), "old\n").unwrap();
    git(&sb.root, &["add", "old.txt"]);
    git(&sb.root, &["commit", "-q", "-m", "old work"]);
    let legacy_tip = git(&sb.root, &["rev-parse", "HEAD"]);
    git(&sb.root, &["switch", "-q", "main"]);
    let o = sb.ok(&["start"]);
    assert!(
        o.contains("PICKED UP legacy branch orch/demo-M1: its work is in refs/orch/orch-demo/M1 now, and the branch is gone"),
        "{o}"
    );
    assert_eq!(git(&sb.root, &["show", "refs/orch/orch-demo/M1:old.txt"]), "old");
    assert_eq!(git(&sb.root, &["rev-parse", "refs/orch/orch-demo/M1"]), legacy_tip);
    assert!(git(&sb.root, &["branch", "--list", "orch/demo-M1"]).is_empty());
    // the rerun changes nothing
    let o = sb.ok(&["start"]);
    assert!(!o.contains("PICKED UP"), "{o}");
}

#[test]
fn finish_squashes_with_commits_single_through_the_binary() {
    let sb = Sandbox::new("feature/demo", "true", "single");
    sb.ok(&["start"]);
    commit(&sb.root, "m1.txt");
    commit(&sb.root, "m2.txt");
    for m in ["M1", "M2"] {
        sb.ok(&["set", m, "merged", "--force"]);
    }
    let o = sb.ok(&["finish", "--no-push"]);
    assert!(o.contains("SQUASHED 2 commits into one (Commits: single)"), "{o}");
    assert_eq!(git(&sb.root, &["rev-list", "--count", "main..HEAD"]), "1");
    let msg = git(&sb.root, &["log", "-1", "--format=%B"]);
    assert!(msg.starts_with("Demo\n"), "{msg}");
    assert!(msg.contains("- m1.txt") && msg.contains("- m2.txt"), "{msg}");
    assert!(msg.contains("Milestones: M1, M2"), "{msg}");
}

#[test]
fn finish_says_no_remote_when_there_is_none() {
    let sb = Sandbox::new("feature/demo", "true", "per-milestone");
    sb.ok(&["start"]);
    commit(&sb.root, "m1.txt");
    for m in ["M1", "M2"] {
        sb.ok(&["set", m, "merged", "--force"]);
    }
    let o = sb.ok(&["finish"]);
    assert!(
        o.contains("NO REMOTE: no `origin` remote — merge feature/demo into main yourself"),
        "{o}"
    );
}
