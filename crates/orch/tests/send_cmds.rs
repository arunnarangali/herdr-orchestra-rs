//! `orch send-job`, `orch handoff` and `orch needs-fix` through the real binary in a real
//! temporary repo (and the workspace's `fake-herdr` for the prompt): Python's output lines and
//! exit codes.

use orch_core::store::{Counter, Store};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLAN: &str = "\
# Plan: Demo
Branch: feature/demo
Base: main
Check command: CHECK
Workers: claude-1 (claude: sonnet), claude-2 (claude: sonnet)
Reviewer: none
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
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
        let plan = PLAN.replace("CHECK", check).replace("ISOLATION", isolation);
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

    fn driver_json(&self) -> String {
        std::fs::read_to_string(self.root.join(".orchestrate/driver.json")).unwrap_or_default()
    }

    /// M1 reviewed once, with these lines in `review-1.md`.
    fn reviewed(&self, md: &str) {
        self.ok(&["prepare", "M1", "claude-1"]);
        self.ok(&["set", "M1", "reviewed", "--force"]);
        self.store().bump("M1", Counter::Review).unwrap();
        let d = self.root.join(".orchestrate/review/M1");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("review-1.md"), md).unwrap();
    }
}

fn fake_herdr() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_orch")).with_file_name("fake-herdr")
}

#[test]
fn the_three_commands_validate_like_python() {
    let sb = Sandbox::new("worktree", "true");
    for (cmd, usage) in [("send-job", "send-job <Mn>"), ("handoff", "handoff <Mn>"), ("needs-fix", "needs-fix <Mn>")] {
        let o = sb.orch(&[cmd]);
        assert_eq!(o.status.code(), Some(2), "{cmd}");
        assert_eq!(err(&o).trim(), format!("ERROR: usage: orch {usage}"));
        let o = sb.orch(&[cmd, "M9"]);
        assert_eq!(o.status.code(), Some(2), "{cmd}");
        assert_eq!(err(&o).trim(), "ERROR: unknown task 'M9' — known: M1, M2");
        assert!(!err(&o).contains("not implemented"));
    }
    let o = sb.orch(&["send-job", "M1"]);
    assert_eq!(err(&o).trim(), "ERROR: M1 has no queued job");
    let o = sb.orch(&["handoff", "M1"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(err(&o).trim(), "ERROR: M1 does not hold a coder");
    let o = sb.orch(&["needs-fix", "M1"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(
        err(&o).trim(),
        "ERROR: no blockers in review round 0 of M1 (.orchestrate/review/M1/review-0.json or .md)"
    );
}

#[test]
fn handoff_saves_the_work_and_frees_the_coder() {
    let sb = Sandbox::new("worktree", "true");
    sb.ok(&["prepare", "M1", "claude-1"]);
    std::fs::write(sb.wt("claude-1").join("new.txt"), "n\n").unwrap();
    sb.ok(&["set", "M1", "verifying", "--force"]);
    let o = sb.ok(&["handoff", "M1"]);
    assert_eq!(o.trim(), "HANDED_OFF M1: saved to refs/orch/feature-demo/M1; claude-1 is free");
    assert!(git(&sb.root, &["rev-parse", "--verify", "refs/orch/feature-demo/M1"]).len() == 40);
    // the coder is free: M2 can take its folder
    sb.ok(&["prepare", "M2", "claude-1"]);
    // a second handoff of a milestone that no longer holds a coder is refused
    let o = sb.orch(&["handoff", "M1"]);
    assert_eq!(err(&o).trim(), "ERROR: M1 does not hold a coder");
}

#[test]
fn handoff_in_shared_mode_is_a_skip() {
    let sb = Sandbox::new("shared", "true");
    let o = sb.orch(&["handoff", "M1"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o).trim(), "SKIP shared mode keeps the coder until the task lands");
}

#[test]
fn needs_fix_queues_the_blockers_as_a_job_for_the_coder() {
    let sb = Sandbox::new("worktree", "make check");
    sb.reviewed("intro\nBLOCKER: a.rs:3 — wrong — breaks\nSUGGESTION: later\nBLOCKER: b.rs:9 — bad — also\n");
    let o = sb.ok(&["needs-fix", "M1"]);
    assert_eq!(o.trim(), "QUEUED fix job for M1 (2 blocker(s))");
    assert!(sb.ok(&["status"]).contains("needs-fix"));
    let job = sb.driver_json();
    assert!(job.contains("review-fix-1.md") && job.contains("BLOCKER: a.rs:3") && job.contains("BLOCKER: b.rs:9"), "{job}");
    assert!(job.contains("make check"), "{job}");
    assert!(!job.contains("SUGGESTION"), "{job}");
}

#[test]
fn needs_fix_for_a_milestone_without_a_coder_says_any_free_coder() {
    let sb = Sandbox::new("worktree", "true");
    sb.reviewed("BLOCKER: x\n");
    sb.ok(&["handoff", "M1"]);
    let o = sb.ok(&["needs-fix", "M1"]);
    assert_eq!(o.trim(), "QUEUED fix job for M1 (1 blocker(s)) — any free coder");
}

#[test]
fn send_job_writes_the_job_into_the_coders_folder_and_sends_it() {
    let sb = Sandbox::new("worktree", "true");
    sb.reviewed("BLOCKER: a.rs:3 — wrong — breaks\n");
    sb.ok(&["needs-fix", "M1"]);
    let state = sb.home.join("fake");
    std::fs::create_dir_all(state.join("agents")).unwrap();
    std::fs::write(
        state.join("agents/claude-1.json"),
        r#"{"name":"claude-1","agent_status":"idle","pane_id":"w1:p1","tab_id":"w1:t1","workspace_id":"w1"}"#,
    )
    .unwrap();
    let fake = fake_herdr();
    let env = [("HERDR", fake.to_str().unwrap()), ("FAKE_HERDR_DIR", state.to_str().unwrap())];
    let o = sb.orch_env(&["send-job", "M1"], &env);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    assert_eq!(out(&o).trim(), "SENT M1 -> claude-1 (attempt 1)");
    let file = sb.wt("claude-1").join(".orch-task/M1/review-fix-1.md");
    assert!(std::fs::read_to_string(file).unwrap().contains("BLOCKER: a.rs:3"));
    let sent = std::fs::read_to_string(state.join("sent.jsonl")).unwrap();
    assert!(sent.contains(".orch-task/M1/review-fix-1.md") && sent.contains("result-1.json"), "{sent}");
    // delivered: the job is the coder's now
    let o = sb.orch(&["send-job", "M1"]);
    assert_eq!(err(&o).trim(), "ERROR: M1 has no queued job");
    assert!(sb.ok(&["status"]).contains("coding"));
}

#[test]
fn send_job_keeps_the_job_when_the_coder_cannot_be_reached() {
    let sb = Sandbox::new("worktree", "true");
    sb.reviewed("BLOCKER: a.rs:3 — wrong — breaks\n");
    sb.ok(&["needs-fix", "M1"]);
    let o = sb.orch(&["send-job", "M1"]); // no herdr at all
    assert_eq!(o.status.code(), Some(5), "{}{}", out(&o), err(&o));
    assert!(out(&o).starts_with("DELIVERY_FAILED M1 -> claude-1: "), "{}", out(&o));
    assert!(sb.driver_json().contains("review-fix-1.md"), "the job is kept");
    assert!(sb.ok(&["status"]).contains("needs-fix"));
}

#[test]
fn send_job_sends_the_check_fix_job_orch_merge_queued() {
    let sb = Sandbox::new("worktree", "! [ -e a1.txt -a -e a2.txt ]");
    for (t, w, f) in [("M1", "claude-1", "a1.txt"), ("M2", "claude-2", "a2.txt")] {
        sb.ok(&["prepare", t, w]);
        std::fs::write(sb.wt(w).join(f), "x\n").unwrap();
        sb.ok(&["set", t, "verifying", "--force"]);
        sb.ok(&["commit", t]);
    }
    sb.ok(&["merge", "M1"]);
    let o = sb.orch(&["merge", "M2"]);
    assert_eq!(o.status.code(), Some(6), "{}{}", out(&o), err(&o));
    let o = sb.orch(&["send-job", "M2"]);
    assert_eq!(o.status.code(), Some(5), "{}{}", out(&o), err(&o));
    let file = std::fs::read_to_string(sb.wt("claude-2").join(".orch-task/M2/integration-fix-1.md")).unwrap();
    assert!(file.contains("# M2 fails the check once merged onto `feature/demo` (round 1)"), "{file}");
    assert!(file.contains("git merge --no-commit feature/demo"), "{file}");
}
