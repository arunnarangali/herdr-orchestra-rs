//! `orch review`, `orch review-ok` and `orch requeue-review` through the real binary in a real
//! temporary repo (and the workspace's `fake-herdr` for the prompt): Python's output lines and
//! exit codes.

use orch_core::store::Store;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLAN: &str = "\
# Plan: Demo
Branch: feature/demo
Base: main
Check command: CHECK
Workers: claude-1 (claude: sonnet), claude-2 (claude: sonnet)
Reviewer: REVIEWER
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
        Sandbox::with(isolation, check, "reviewer (claude: opus)")
    }

    fn with(isolation: &str, check: &str, reviewer: &str) -> Sandbox {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let (root, home) = (base.join("repo"), base.join("home"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
        let plan = PLAN.replace("REVIEWER", reviewer).replace("CHECK", check).replace("ISOLATION", isolation);
        std::fs::write(root.join(".orchestrate/plan.md"), plan).unwrap();
        let sb = Sandbox { _t: t, root, home };
        sb.ok(&["sync"]);
        sb.ok(&["start"]);
        sb
    }

    fn orch_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_orch"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("HERDR", self.home.join("no-herdr"))
            .envs(env.iter().copied())
            .env_remove("ORCH_AGENTS_FILE")
            .env_remove("ORCH_MODELS_FILE")
            .env_remove("ORCH_LOG")
            .output()
            .unwrap()
    }

    fn orch(&self, args: &[&str]) -> Output {
        self.orch_env(args, &[])
    }

    fn ok(&self, args: &[&str]) -> String {
        let o = self.orch(args);
        assert_eq!(o.status.code(), Some(0), "orch {args:?}: {}{}", out(&o), err(&o));
        out(&o)
    }

    fn wt(&self, w: &str) -> PathBuf {
        self.root.parent().unwrap().join("repo-wt").join(w)
    }

    fn store(&self) -> Store {
        Store::open_in(&self.root.join(".orchestrate")).unwrap()
    }
}

fn fake_herdr() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_orch")).with_file_name("fake-herdr")
}

impl Sandbox {
    /// M1 coded by claude-1, handed off (its work is in the hidden ref) and waiting for review.
    fn awaiting(&self, tid: &str, worker: &str, file: &str) {
        self.ok(&["prepare", tid, worker]);
        std::fs::write(self.wt(worker).join(file), "x\n").unwrap();
        self.ok(&["set", tid, "verifying", "--force"]);
        self.ok(&["handoff", tid]);
        self.ok(&["set", tid, "awaiting-review", "--force"]);
    }

    fn fake(&self) -> (PathBuf, Vec<(String, String)>) {
        let state = self.home.join("fake");
        std::fs::create_dir_all(state.join("agents")).unwrap();
        std::fs::write(
            state.join("agents/reviewer.json"),
            r#"{"name":"reviewer","agent_status":"idle","pane_id":"w1:p9","tab_id":"w1:t1","workspace_id":"w1"}"#,
        )
        .unwrap();
        let env = vec![
            ("HERDR".to_string(), fake_herdr().to_str().unwrap().to_string()),
            ("FAKE_HERDR_DIR".to_string(), state.to_str().unwrap().to_string()),
        ];
        (state, env)
    }

    fn orch_fake(&self, args: &[&str]) -> Output {
        let (_, env) = self.fake();
        let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        self.orch_env(args, &env)
    }
}

#[test]
fn the_three_commands_validate_like_python() {
    let sb = Sandbox::new("worktree", "true");
    for (cmd, usage) in [("review", "review <Mn>"), ("review-ok", "review-ok <Mn>"), ("requeue-review", "requeue-review <Mn>")] {
        let o = sb.orch(&[cmd]);
        assert_eq!(o.status.code(), Some(2), "{cmd}");
        assert_eq!(err(&o).trim(), format!("ERROR: usage: orch {usage}"));
        let o = sb.orch(&[cmd, "M9"]);
        assert_eq!(o.status.code(), Some(2), "{cmd}");
        assert_eq!(err(&o).trim(), "ERROR: unknown task 'M9' — known: M1, M2");
        assert!(!err(&o).contains("not implemented"));
    }
    let o = sb.orch(&["requeue-review", "M1"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err(&o).trim(), "ERROR: M1 is pending, expected reviewing");
}

#[test]
fn review_without_a_reviewer_is_refused() {
    let sb = Sandbox::with("worktree", "true", "none");
    let o = sb.orch(&["review", "M1"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err(&o).trim(), "ERROR: no reviewer configured");
}

#[test]
fn review_ok_prints_python_s_line() {
    let sb = Sandbox::new("worktree", "true");
    let o = sb.orch(&["review-ok", "M1"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o).trim(), "OK M1 review approved");
}

#[test]
fn review_writes_the_diff_and_the_prompt_and_sends_the_reviewer_the_round() {
    let sb = Sandbox::new("worktree", "true");
    sb.awaiting("M1", "claude-1", "new.txt");
    let o = sb.orch_fake(&["review", "M1"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    assert_eq!(out(&o).trim(), "SENT review M1 -> reviewer (round 1)");
    let rd = sb.root.join(".orchestrate/review/M1");
    assert!(std::fs::read_to_string(rd.join("diff.patch")).unwrap().contains("new.txt"));
    let prompt = std::fs::read_to_string(rd.join("review-task-1.md")).unwrap();
    assert!(prompt.contains(".orchestrate/review/M1/review-1.json") && prompt.contains("Do NOT modify"), "{prompt}");
    let sent = std::fs::read_to_string(sb.home.join("fake/sent.jsonl")).unwrap();
    assert!(sent.contains("review-task-1.md"), "{sent}");
    assert!(sb.ok(&["status"]).contains("reviewing"));
    // the reviewer is on M1: M2 waits
    sb.awaiting("M2", "claude-2", "other.txt");
    let o = sb.orch_fake(&["review", "M2"]);
    assert_eq!(o.status.code(), Some(3), "{}{}", out(&o), err(&o));
    assert_eq!(out(&o).trim(), "BUSY reviewer is on M1; M2 queued as awaiting-review");
}

#[test]
fn requeue_review_takes_the_round_back_and_review_sends_the_same_round() {
    let sb = Sandbox::new("worktree", "true");
    sb.awaiting("M1", "claude-1", "new.txt");
    sb.orch_fake(&["review", "M1"]);
    let o = sb.orch(&["requeue-review", "M1"]);
    assert_eq!(out(&o).trim(), "REQUEUED M1 for review");
    assert_eq!(sb.store().get_task("M1").unwrap().review_rounds, 0);
    assert!(sb.ok(&["status"]).contains("awaiting-review"));
    let o = sb.orch_fake(&["review", "M1"]);
    assert_eq!(out(&o).trim(), "SENT review M1 -> reviewer (round 1)");
}

#[test]
fn an_unreachable_reviewer_leaves_the_milestone_waiting_for_the_same_round() {
    let sb = Sandbox::new("worktree", "true");
    sb.awaiting("M1", "claude-1", "new.txt");
    let o = sb.orch(&["review", "M1"]); // no herdr at all
    assert_eq!(o.status.code(), Some(5), "{}{}", out(&o), err(&o));
    assert!(out(&o).starts_with("DELIVERY_FAILED review M1 -> reviewer: "), "{}", out(&o));
    assert!(sb.ok(&["status"]).contains("awaiting-review"));
    assert_eq!(sb.store().get_task("M1").unwrap().review_rounds, 0);
}
