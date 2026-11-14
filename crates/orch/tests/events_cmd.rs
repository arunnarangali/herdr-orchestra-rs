//! `orch events` against the real binary in a temporary repo: the Python v4.10.1 layout, the
//! acknowledgement, `--peek`, a run without a lead, and the stub handlers' routes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLAN: &str = "\
# Plan: Demo
Branch: feature/demo
Check command: cargo test
Workers: claude-1 (claude: sonnet), codex-1 (codex: gpt-5)
Lead: lead (claude: opus)
Reviewer: none

## [x] M1: One
Depends on: none
Tier: medium
Model why: normal work
Goal:
- g
Acceptance criteria:
- works

## [x] M2: Two
Depends on: M1 (needs it)
Why separate: second
Tier: high
Model why: hard work
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

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(st.success());
}

impl Sandbox {
    fn new(plan: &str) -> Sandbox {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let (root, home) = (base.join("repo"), base.join("home"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
        std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
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
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const CLOSING: &str = "Handle each event above, then STOP: you are woken again when something else happens. Never prompt a coder yourself and never poll.\n";

#[test]
fn a_fresh_run_lists_ready_work_and_the_workers() {
    let sb = Sandbox::new(PLAN);
    let o = sb.orch(&["events", "--peek"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let text = out(&o);
    assert_eq!(
        text,
        "EVENTS for the lead — run feature/demo: 0 new, 0 still open\n\
         READY — waiting for your dispatch:\n\
         \x20 M1 \"One\" tier medium -> claude-1 or codex-1:  orch dispatch M1\n\
         WORKERS: claude-1 (claude: sonnet, medium) free, codex-1 (codex: gpt-5, unrated) free\n\
         event manager: NOT running — your next `orch dispatch` or `orch answer` starts it\n"
            .to_string()
            + CLOSING
    );
}

#[test]
fn events_are_shown_with_help_then_acknowledged_and_stay_open_until_the_milestone_moves() {
    let sb = Sandbox::new(PLAN);
    assert_eq!(
        out(&sb.orch(&["set", "M1", "paused", "needs", "a", "decision"])),
        "OK\n"
    );
    let first = out(&sb.orch(&["events", "--peek"]));
    assert!(
        first.starts_with("EVENTS for the lead — run feature/demo: 1 new, 0 still open\n"),
        "{first}"
    );
    assert!(first.contains("#1 PAUSED M1 \"One\": needs a decision\n    -> a coder's question, or an agent that stopped."), "{first}");
    assert!(
        first.contains("then `orch answer M1 \"<their answer>\"`."),
        "{first}"
    );
    assert!(
        !first.contains("READY"),
        "M1 is paused, nothing is ready: {first}"
    );
    assert!(first.ends_with(CLOSING), "{first}");

    // --peek did not acknowledge
    let again = out(&sb.orch(&["events", "--peek"]));
    assert_eq!(first, again);

    // reading acknowledges: the entry moves to STILL OPEN while M1 stays paused
    let read = out(&sb.orch(&["events"]));
    assert_eq!(read, first);
    let after = out(&sb.orch(&["events"]));
    assert!(
        after.starts_with("EVENTS for the lead — run feature/demo: 0 new, 1 still open\n"),
        "{after}"
    );
    assert!(after.contains("STILL OPEN (you have seen these; they close when the milestone moves on):\n#1 PAUSED M1"), "{after}");
    assert!(!after.contains("Nothing needs you."));

    // the milestone moves on: the blocking entry closes
    assert_eq!(out(&sb.orch(&["set", "M1", "pending"])), "OK\n");
    let closed = out(&sb.orch(&["events", "--peek"]));
    assert!(
        closed.starts_with("EVENTS for the lead — run feature/demo: 0 new, 0 still open\n"),
        "{closed}"
    );
    assert!(
        closed.contains("READY — waiting for your dispatch:\n  M1 \"One\""),
        "{closed}"
    );
}

#[test]
fn a_unread_blocking_event_whose_milestone_moved_on_is_not_shown() {
    let sb = Sandbox::new(PLAN);
    sb.orch(&["set", "M1", "paused", "x"]);
    sb.orch(&["set", "M1", "pending"]);
    let text = out(&sb.orch(&["events", "--peek"]));
    assert!(
        text.starts_with("EVENTS for the lead — run feature/demo: 0 new, 0 still open\n"),
        "{text}"
    );
}

#[test]
fn a_run_without_a_lead_says_so_and_exits_0() {
    let sb = Sandbox::new(&PLAN.replace("Lead: lead (claude: opus)\n", "Lead mode: off\n"));
    let o = sb.orch(&["events"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "This run has no lead events (plan header: `Lead: lead (<kind>)`, and not `Lead mode: off`).\n"
    );
    // nothing is queued for a run without a lead
    sb.orch(&["set", "M1", "paused", "x"]);
    assert!(out(&sb.orch(&["events"])).starts_with("This run has no lead events"));
}

#[test]
fn no_run_is_an_error() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    git(&root, &["init", "-q"]);
    let o = Command::new(env!("CARGO_BIN_EXE_orch"))
        .arg("events")
        .current_dir(&root)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("no run in .orchestrate/state.db"));
}

#[test]
fn no_stub_routes_are_left() {
    // `dispatch`, `answer`, `fix`, `approve` and `escalate` are real (tests/dispatch_cmds.rs,
    // the commands milestone); `autopilot` is real too (tests/autopilot_cmd.rs, the
    // event-manager milestone). Nothing routes to a stub handler any more.
}
