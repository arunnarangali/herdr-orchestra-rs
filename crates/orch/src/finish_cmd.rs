//! `orch finish [--no-push]`: the CLI wrapper around `orch_core::finish`. It prints the
//! lines finish produced in order and reports a failure the way Python v4.10.1's `die` does
//! (`ERROR: ...`, exit 2), after the lines that came before it.

use orch_core::finish::{self, Options};
use orch_core::paths;
use orch_core::store::{DB_FILE, Store};
use std::path::PathBuf;

const NO_RUN: &str = "no run in .orchestrate/state.db — run `orch sync` after writing plan.md";

fn die(msg: impl std::fmt::Display) -> i32 {
    eprintln!("ERROR: {msg}");
    2
}

/// `orch finish [--no-push]`.
pub fn finish(args: &[String]) -> i32 {
    let no_push = args.iter().any(|a| a == "--no-push");
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
    let opts = Options {
        no_push,
        gh: finish::gh_available().then(|| PathBuf::from("gh")),
    };
    match finish::run(&mut store, &root, &opts) {
        Ok(lines) => {
            for l in lines {
                println!("{l}");
            }
            0
        }
        Err(f) => {
            for l in f.lines {
                println!("{l}");
            }
            eprintln!("ERROR: {}", f.message);
            2
        }
    }
}
