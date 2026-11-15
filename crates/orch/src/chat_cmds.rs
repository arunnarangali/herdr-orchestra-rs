//! `orch say "<text>" [--about Mn]`: the lead's reply to the developer.
//!
//! The developer talks to the lead from the GUI; the event manager types each message into the
//! lead's pane. The lead answers with this command, which stores the reply in the same
//! conversation (the GUI shows it, and the developer's earlier messages count as read). It only
//! writes a row: it never types into a pane and never changes the run, so it works while the
//! event manager runs, like `orch events`.

use orch_core::paths;
use orch_core::plan_edit::py_repr;
use orch_core::store::{DB_FILE, Store};

const USAGE: &str = "usage: orch say \"<text>\" [--about Mn]";
const NO_RUN: &str = "no run in .orchestrate/state.db — run `orch sync` after writing plan.md";

fn die(msg: impl std::fmt::Display) -> i32 {
    eprintln!("ERROR: {msg}");
    2
}

/// `orch say "<text>" [--about Mn]`.
pub fn say(args: &[String]) -> i32 {
    let mut words: Vec<&str> = Vec::new();
    let mut about: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--about" {
            match it.next() {
                Some(v) => about = Some(v.clone()),
                None => return die("--about needs a milestone, like `--about M2`"),
            }
        } else {
            words.push(a);
        }
    }
    let text = words.join(" ");
    if text.trim().is_empty() {
        return die(USAGE);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let root = match paths::main_root(&cwd) {
        Ok(r) => r,
        Err(e) => return die(e),
    };
    let dir = root.join(paths::ORCH_DIR);
    if !dir.join(DB_FILE).exists() {
        return die(NO_RUN);
    }
    let mut store = match Store::open_in(&dir) {
        Ok(s) => s,
        Err(e) => return die(e),
    };
    let run = match store.load_run() {
        Ok(Some(r)) => r,
        Ok(None) => return die(NO_RUN),
        Err(e) => return die(e),
    };
    if let Some(a) = &about
        && !run.plan.iter().any(|p| &p.id == a)
    {
        let known: Vec<&str> = run.plan.iter().map(|p| p.id.as_str()).collect();
        return die(format!(
            "unknown task {} — known: {}",
            py_repr(a),
            if known.is_empty() { "none".into() } else { known.join(", ") }
        ));
    }
    match store.post_lead_reply(&text, about.as_deref()) {
        Ok(m) => {
            match &m.about {
                Some(a) => println!("SAID #{} to the developer (about {a})", m.id),
                None => println!("SAID #{} to the developer", m.id),
            }
            0
        }
        Err(e) => die(e),
    }
}
