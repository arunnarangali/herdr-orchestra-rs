//! `orch say` against the real binary in a temporary repo, and the playbooks that tell the
//! lead to use it.

use orch_core::chat::{ChatKind, Delivery, Sender};
use orch_core::store::Store;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLAN: &str = "\
# Plan: Demo
Branch: feature/demo
Check command: cargo test
Workers: claude-1 (claude: sonnet)
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
";

struct Sandbox {
    _t: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(st.success());
}

impl Sandbox {
    fn new() -> Sandbox {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let (root, home) = (base.join("repo"), base.join("home"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
        std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
        std::fs::write(root.join(".orchestrate/plan.md"), PLAN).unwrap();
        let sb = Sandbox { _t: t, root, home };
        let o = sb.orch(&["sync"]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        sb
    }

    fn orch(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_orch"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("HERDR", self.home.join("no-herdr"))
            .env("ORCH_NO_AUTOSTART", "1")
            .output()
            .unwrap()
    }

    fn store(&self) -> Store {
        Store::open_in(&self.root.join(".orchestrate")).unwrap()
    }
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn say_stores_the_lead_reply_and_acknowledges_what_was_sent() {
    let sb = Sandbox::new();
    let mut s = sb.store();
    let asked = s.post_developer_message(ChatKind::Message, "how is M1?", None).unwrap();
    // pretend the event manager typed it
    drop(s);
    let db = rusqlite_path(&sb);
    mark_sent(&db, asked.id);

    let o = sb.orch(&["say", "M1", "is", "running", "--about", "M1"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "SAID #2 to the developer (about M1)\n");

    let s = sb.store();
    let all = s.chat_messages(None, 10).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(
        (all[1].sender, all[1].text.as_str(), all[1].about.as_deref()),
        (Sender::Lead, "M1 is running", Some("M1"))
    );
    assert_eq!(all[0].delivery, Some(Delivery::Acknowledged));

    let o = sb.orch(&["say", "all", "fine"]);
    assert_eq!(out(&o), "SAID #3 to the developer\n");
}

#[test]
fn say_refuses_what_it_cannot_store_and_stores_nothing() {
    let sb = Sandbox::new();
    for (args, msg) in [
        (vec!["say"], "usage: orch say \"<text>\" [--about Mn]"),
        (vec!["say", "  "], "usage: orch say \"<text>\" [--about Mn]"),
        (vec!["say", "hi", "--about"], "--about needs a milestone, like `--about M2`"),
        (vec!["say", "hi", "--about", "M9"], "unknown task 'M9' — known: M1"),
    ] {
        let o = sb.orch(&args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert_eq!(err(&o), format!("ERROR: {msg}\n"), "{args:?}");
    }
    assert!(sb.store().chat_messages(None, 10).unwrap().is_empty());
}

#[test]
fn say_without_a_run_points_at_sync() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let o = Command::new(env!("CARGO_BIN_EXE_orch"))
        .args(["say", "hi"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("run `orch sync`"), "{}", err(&o));
}

#[test]
fn both_playbooks_tell_the_lead_to_answer_gui_messages_with_orch_say() {
    for which in ["plan", "run"] {
        let o = Command::new(env!("CARGO_BIN_EXE_orch"))
            .args(["lead", which])
            .env("HOME", std::env::temp_dir())
            .output()
            .unwrap();
        let text = out(&o);
        assert!(text.contains("Messages from the GUI chat"), "{which}");
        assert!(text.contains("orch say"), "{which}");
        assert!(text.contains("orch chat #"), "{which}");
    }
}

fn rusqlite_path(sb: &Sandbox) -> PathBuf {
    sb.root.join(".orchestrate").join(orch_core::store::DB_FILE)
}

/// Record the message as typed, the way the event manager does after a delivery.
fn mark_sent(db: &Path, id: i64) {
    let mut s = Store::open(db).unwrap();
    let lead = orch_core::wake::LeadPane::Idle;
    struct R(orch_core::wake::LeadPane);
    impl orch_core::wake::LeadRuntime for R {
        fn pane(&self, _: &str) -> orch_core::wake::LeadPane {
            self.0
        }
        fn send(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
    }
    struct N;
    impl orch_core::wake::Notifier for N {
        fn notify(&self, _: &str) {}
    }
    let rt = R(lead);
    let env = orch_core::wake::WakeEnv {
        runtime: &rt,
        notifier: &N,
        clock: &orch_core::wake::SystemClock,
        tun: orch_core::wake::WakeTunables::default(),
    };
    let t = orch_core::chat::chat_tick(&mut s, &env, &mut |_| {}).unwrap();
    assert_eq!(t, orch_core::chat::ChatTick::Sent(id));
}
