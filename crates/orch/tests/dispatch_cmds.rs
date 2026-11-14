//! `orch dispatch` (and `retry`), `answer`, `fix`, `approve` and `escalate` against the real
//! binary in a temporary repo. The wording is Python v4.10.1's (both were run side by side).

use orch_core::store::{Store, TransitionRequest};
use orch_core::task::State;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLAN: &str = "\
# Plan: Demo
Branch: feature/demo
Check command: cargo test
Workers: claude-1 (claude: sonnet), claude-2 (claude: opus)
Lead: lead (claude: opus)
Reviewer: none

## [x] M1: One
Depends on: none
Why separate: first
Tier: medium
Model why: normal work
Review: yes
Likely files: src/a.rs, src/b.rs
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
Notes from user:
- earlier note
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
            .env("ORCH_NO_AUTOSTART", "1")
            .env_remove("ORCH_AGENTS_FILE")
            .env_remove("ORCH_MODELS_FILE")
            .env_remove("ORCH_LOG")
            .output()
            .unwrap()
    }

    fn store(&self) -> Store {
        Store::open_in(&self.root.join(".orchestrate")).unwrap()
    }

    fn plan(&self) -> String {
        std::fs::read_to_string(self.root.join(".orchestrate/plan.md")).unwrap()
    }

    fn set(&self, id: &str, to: State, force: bool) {
        self.store()
            .transition(id, &TransitionRequest::manual(to, Some("test"), force))
            .unwrap();
    }
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const NOT_RUNNING: &str =
    "the event manager is NOT running — ask the developer to run `orch autopilot` in a terminal\n";
const WOKEN: &str =
    "You will be woken with `orch events` when something needs you. Stop here; do not poll.\n";

fn refused(sb: &Sandbox, args: &[&str], code: i32, msg: &str) {
    let o = sb.orch(args);
    assert_eq!(
        o.status.code(),
        Some(code),
        "{args:?}: {}{}",
        out(&o),
        err(&o)
    );
    assert_eq!(err(&o), format!("ERROR: {msg}\n"), "{args:?}");
    assert_eq!(out(&o), "", "{args:?}");
}

#[test]
fn dispatch_prints_the_line_and_records_the_intent() {
    let sb = Sandbox::new(PLAN);
    let o = sb.orch(&["dispatch", "M1", "M2", "--tier", "high", "--to", "claude-2"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        out(&o),
        format!(
            "DISPATCH M1 -> claude-2 (tier high)\nDISPATCH M2 -> claude-2 (tier high) — starts when M1 merged\n{NOT_RUNNING}{WOKEN}"
        )
    );
    let s = sb.store();
    let i = s.intent("M1").unwrap().unwrap();
    assert_eq!(
        (i.go, i.assign.as_deref(), i.min_tier.as_deref()),
        (true, Some("claude-2"), Some("high"))
    );
    // started milestones leave READY
    let ev = out(&sb.orch(&["events", "--peek"]));
    assert!(!ev.contains("READY"), "{ev}");
    // no flags: the eligible coders of the plan tier
    let sb2 = Sandbox::new(PLAN);
    assert!(
        out(&sb2.orch(&["dispatch", "M1"]))
            .starts_with("DISPATCH M1 -> claude-1 or claude-2 (tier medium)\n")
    );
}

#[test]
fn retry_is_an_alias_and_a_paused_or_failed_milestone_starts_afresh() {
    let sb = Sandbox::new(PLAN);
    sb.set("M1", State::Failed, false);
    let o = sb.orch(&["retry", "M1"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).starts_with("DISPATCH M1 -> "));
    let t = sb.store().get_task("M1").unwrap();
    assert_eq!(t.status, State::Pending);
    assert_eq!(
        sb.store().history("M1").unwrap().last().unwrap().why,
        "dispatched again by the lead"
    );
    sb.set("M1", State::Paused, false);
    assert_eq!(sb.orch(&["dispatch", "M1"]).status.code(), Some(0));
    assert_eq!(sb.store().get_task("M1").unwrap().status, State::Pending);
}

#[test]
fn dispatch_refusals_use_pythons_words_and_change_nothing() {
    let sb = Sandbox::new(PLAN);
    refused(
        &sb,
        &["dispatch"],
        2,
        "usage: orch dispatch <Mn> [Mn...] [--to <worker>] [--tier small|medium|high]",
    );
    refused(&sb, &["dispatch", "M1", "--to"], 2, "--to needs a value");
    refused(
        &sb,
        &["dispatch", "M1", "--to", "nobody"],
        2,
        "unknown worker 'nobody' — workers: claude-1, claude-2",
    );
    refused(
        &sb,
        &["dispatch", "M1", "--tier", "huge"],
        2,
        "--tier must be small, medium or high",
    );
    refused(
        &sb,
        &["dispatch", "M9"],
        2,
        "unknown task 'M9' — known: M1, M2",
    );
    sb.set("M2", State::Skipped, false);
    refused(
        &sb,
        &["dispatch", "M1", "M2"],
        2,
        "M2 is skipped — only pending, paused or failed milestones can be dispatched",
    );
    assert_eq!(
        sb.store().intent("M1").unwrap(),
        None,
        "M1 was fine but M2 was not: nothing changed"
    );
}

#[test]
fn answer_writes_the_note_sends_the_milestone_back_and_tells_the_lead() {
    let sb = Sandbox::new(PLAN);
    refused(
        &sb,
        &["answer", "M1"],
        2,
        "usage: orch answer <Mn> <answer text>",
    );
    refused(
        &sb,
        &["answer", "M1", "yes"],
        2,
        "M1 is pending, not paused",
    );
    sb.set("M1", State::Paused, false);
    sb.set("M2", State::Paused, false);
    {
        let mut s = sb.store();
        s.assign_worker("M1", Some("claude-1"), None).unwrap();
        s.assign_worker("M1", None, None).unwrap(); // frees it; claude-1 stays last_worker
    }
    let before = sb.plan();
    let o = sb.orch(&["answer", "M1", "use", "sqlite,", " please "]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        out(&o),
        format!(
            "ANSWERED M1: written to plan.md; it goes back to claude-1 with its work so far\n{NOT_RUNNING}{WOKEN}"
        )
    );
    // M1 had no `Notes from user:`: the line is created at the end of its section, the rest untouched
    assert_eq!(
        sb.plan(),
        before.replace(
            "- works\n\n## [x] M2",
            "- works\nNotes from user:\n- use sqlite,  please\n\n## [x] M2"
        )
    );
    let s = sb.store();
    assert_eq!(s.get_task("M1").unwrap().status, State::Pending);
    assert_eq!(
        s.intent("M1").unwrap().unwrap().assign.as_deref(),
        Some("claude-1")
    );
    let ev = out(&sb.orch(&["events", "--peek"]));
    assert!(
        ev.contains("ANSWER_PROVIDED M1 \"One\": use sqlite,  please"),
        "{ev}"
    );
    // an existing `Notes from user:` line gets the note right under it
    let o = sb.orch(&["answer", "M2", "second"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(
        sb.plan()
            .ends_with("Notes from user:\n- second\n- earlier note\n"),
        "{}",
        sb.plan()
    );
    // a second sync is clean
    let o = sb.orch(&["sync"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(!out(&o).contains("ERROR"), "{}", out(&o));
}

#[test]
fn answer_with_no_section_in_the_plan_changes_nothing() {
    let sb = Sandbox::new(PLAN);
    sb.set("M1", State::Paused, false);
    // the plan loses M1's section after the sync
    let cut = sb.plan().replace("## [x] M1: One", "## Not a milestone");
    std::fs::write(sb.root.join(".orchestrate/plan.md"), &cut).unwrap();
    refused(&sb, &["answer", "M1", "yes"], 2, "M1 not found in plan.md");
    assert_eq!(sb.plan(), cut);
    assert_eq!(sb.store().get_task("M1").unwrap().status, State::Paused);
    assert!(
        sb.store()
            .lead_entries()
            .unwrap()
            .iter()
            .all(|e| e.kind.as_str() != "AnswerProvided")
    );
    let files: Vec<_> = std::fs::read_dir(sb.root.join(".orchestrate"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(
        files.iter().all(|f| !f.to_string_lossy().ends_with(".tmp")),
        "{files:?}"
    );
}

#[test]
fn two_answers_at_once_one_wins() {
    let sb = Sandbox::new(PLAN);
    sb.set("M1", State::Paused, false);
    let handles: Vec<_> = (0..2)
        .map(|n| {
            let (root, home) = (sb.root.clone(), sb.home.clone());
            std::thread::spawn(move || {
                Command::new(env!("CARGO_BIN_EXE_orch"))
                    .args(["answer", "M1", &format!("answer {n}")])
                    .current_dir(root)
                    .env("HOME", &home)
                    .env("HERDR", home.join("no-herdr"))
                    .env("ORCH_NO_AUTOSTART", "1")
                    .output()
                    .unwrap()
            })
        })
        .collect();
    let codes: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().unwrap().status.code().unwrap())
        .collect();
    assert_eq!(codes.iter().filter(|c| **c == 0).count(), 1, "{codes:?}");
    assert_eq!(
        sb.plan().matches("Notes from user:").count(),
        2,
        "one existing (M2) plus one new"
    );
}

#[test]
fn fix_appends_pythons_section_and_the_plan_still_syncs() {
    let sb = Sandbox::new(PLAN);
    refused(
        &sb,
        &["fix", "M1"],
        2,
        "usage: orch fix <Mn> <what should change> [--to <worker>]",
    );
    refused(
        &sb,
        &["fix", "M1", "  "],
        2,
        "say what should change: orch fix <Mn> \"<what should change>\"",
    );
    refused(
        &sb,
        &["fix", "M1", "x"],
        2,
        "M1 is pending, not landed yet — `orch fix` is for work that already landed",
    );
    sb.set("M1", State::Merged, true);
    refused(
        &sb,
        &["fix", "M1", "x", "--to", "nobody"],
        2,
        "unknown worker 'nobody' — workers: claude-1, claude-2",
    );
    sb.store()
        .assign_worker("M1", Some("claude-2"), None)
        .unwrap();
    let before = sb.plan();
    let o = sb.orch(&["fix", "M1", "tighten", "the", " error text"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        out(&o),
        format!(
            "FIX M3 \"Fix for M1: tighten the error text\" -> claude-2; added to plan.md, the check still run\n{NOT_RUNNING}It lands like any milestone: `orch status` shows M3, and the summary is written again when it is in.\n"
        )
    );
    assert_eq!(
        sb.plan(),
        format!(
            "{}\n\n{}",
            before.trim_end_matches('\n'),
            "\
## [x] M3: Fix for M1: tighten the error text
Depends on: M1 (a fix to what M1 built)
Why separate: a change the developer asked for after M1 landed.
Review: yes
Likely files: src/a.rs, src/b.rs
Tier: medium
Model why: a fix to M1, for the coder who built it.
Goal: tighten the error text
Why: the developer asked for this after M1 landed.
Acceptance criteria:
- tighten the error text
- nothing else that M1 built changes, and the check command passes
Open questions:
- none
"
        )
    );
    let s = sb.store();
    let t = s.get_task("M3").unwrap();
    assert_eq!(
        (t.status, t.deps.as_slice()),
        (State::Pending, &["M1".to_string()][..])
    );
    assert_eq!(
        s.intent("M3").unwrap().unwrap().assign.as_deref(),
        Some("claude-2")
    );
    let o = sb.orch(&["sync"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    assert!(out(&o).contains("OK: 3 tasks"), "{}", out(&o));
    assert!(!out(&o).contains("WARN: M3"), "{}", out(&o));
    // a second fix gets the next number; no one on the team built it when it has no history
    let o = sb.orch(&["fix", "M1", "again"]);
    assert!(
        out(&o).starts_with("FIX M4 \"Fix for M1: again\" -> claude-2;"),
        "{}",
        out(&o)
    );
}

#[test]
fn approve_lets_a_held_milestone_land_and_refuses_anything_else() {
    let sb = Sandbox::new(PLAN);
    refused(&sb, &["approve"], 2, "usage: orch approve <Mn>");
    refused(
        &sb,
        &["approve", "M9"],
        2,
        "unknown task 'M9' — known: M1, M2",
    );
    refused(&sb, &["approve", "M1"], 2, "M1 is pending, not held");
    sb.set("M1", State::Held, true);
    let o = sb.orch(&["approve", "M1"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        out(&o),
        "APPROVED M1: it lands on the next autopilot step (or `orch merge M1`)\n"
    );
    assert_eq!(sb.store().get_task("M1").unwrap().status, State::Committed);
    assert_eq!(
        sb.store().history("M1").unwrap().last().unwrap().why,
        "approved by the developer"
    );
}

#[test]
fn escalate_moves_one_tier_up_once() {
    let sb = Sandbox::new(PLAN);
    refused(&sb, &["escalate"], 2, "usage: orch escalate <Mn> [reason]");
    refused(
        &sb,
        &["escalate", "M9"],
        2,
        "unknown task 'M9' — known: M1, M2",
    );
    // no coder yet: nothing to move up from
    let o = sb.orch(&["escalate", "M1"]);
    assert_eq!(
        (o.status.code(), out(&o).as_str()),
        (Some(3), "NO_ESCALATION M1\n")
    );
    {
        let mut s = sb.store();
        s.assign_worker("M1", Some("claude-1"), None).unwrap();
        s.transition(
            "M1",
            &TransitionRequest::auto(State::Failed, Some("rounds")),
        )
        .unwrap();
    }
    let o = sb.orch(&["escalate", "M1", "out", "of", "rounds"]);
    assert_eq!(
        (o.status.code(), out(&o).as_str()),
        (Some(0), "ESCALATED M1 -> high\n"),
        "{}",
        err(&o)
    );
    let s = sb.store();
    let t = s.get_task("M1").unwrap();
    assert_eq!((t.status, t.escalated), (State::Pending, true));
    assert_eq!(
        s.intent("M1").unwrap().unwrap().min_tier.as_deref(),
        Some("high")
    );
    let ev = out(&sb.orch(&["events", "--peek"]));
    assert!(
        ev.contains("ESCALATED M1 \"One\": escalated to a high-tier model: out of rounds"),
        "{ev}"
    );
    let o = sb.orch(&["escalate", "M1"]);
    assert_eq!(
        (o.status.code(), out(&o).as_str()),
        (Some(3), "NO_ESCALATION M1\n")
    );
}

#[test]
fn no_run_is_an_error_for_every_command() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    git(&root, &["init", "-q"]);
    for args in [
        vec!["dispatch", "M1"],
        vec!["answer", "M1", "x"],
        vec!["fix", "M1", "x"],
        vec!["approve", "M1"],
        vec!["escalate", "M1"],
    ] {
        let o = Command::new(env!("CARGO_BIN_EXE_orch"))
            .args(&args)
            .current_dir(&root)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(
            err(&o).contains("no run in .orchestrate/state.db"),
            "{args:?}: {}",
            err(&o)
        );
    }
}
