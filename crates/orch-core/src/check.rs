//! The one check runner (§13, Python `run_check`): run the plan's check command through the
//! shell in a folder, bounded by a time limit, and say whether it passed with the tail of its
//! output. The milestone check, the baseline, the final check, the check on a merged result and
//! `orch finish` all go through [`run`]; nothing else spawns the plan's command.
//!
//! The command runs as the leader of its own process group, so a timeout kills it *and* every
//! child it started (a hung `cargo test`, a dev server) rather than leaving them behind. Output
//! is read as it arrives and only its last lines are kept, so a noisy check cannot grow memory.
//!
//! Each run can be recorded for the milestone ([`Records`]): where it ran, whether it passed,
//! how long it took and when, as Python's `checks` list. The records also keep the files a
//! coder changed outside its `Likely files` (the scope check), for the run summary.

use crate::error::{Error, Result};
use crate::paths::ORCH_DIR;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Lines of output a failed check shows (`orch finish` and the fix prompts).
pub const TAIL_LINES: usize = 30;
/// `ORCH_CHECK_TIMEOUT_MIN`'s default.
pub const DEFAULT_TIMEOUT_MIN: u64 = 30;
/// How long to wait for the output readers once the command is gone.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// What one run of the check command came to.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckRun {
    pub ok: bool,
    /// The last lines of stdout then stderr; on a timeout, the line saying so.
    pub tail: String,
    /// Wall-clock seconds, one decimal (Python `round(…, 1)`).
    pub secs: f64,
    pub timed_out: bool,
}

/// `ORCH_CHECK_TIMEOUT_MIN` (default 30; a value that is not a positive number is the default).
pub fn timeout_from_env() -> Duration {
    let min = std::env::var("ORCH_CHECK_TIMEOUT_MIN")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|m| *m > 0)
        .unwrap_or(DEFAULT_TIMEOUT_MIN);
    Duration::from_secs(min * 60)
}

/// Run `command` through `sh -c` in `dir`, killing its whole process group after `limit`.
/// `tail_lines` bounds the output kept. An empty command passes at once (Python: no `Check
/// command` means nothing to run).
pub fn run(command: &str, dir: &Path, limit: Duration, tail_lines: usize) -> CheckRun {
    let start = Instant::now();
    let done = |ok: bool, tail: String, timed_out: bool| CheckRun {
        ok,
        tail,
        secs: (start.elapsed().as_secs_f64() * 10.0).round() / 10.0,
        timed_out,
    };
    if command.trim().is_empty() {
        return done(true, String::new(), false);
    }
    let spawned = Command::new("sh")
        .args(["-c", command])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0) // its own group: the timeout kills the command's children too
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return done(false, format!("could not run the check command: {e}"), false),
    };
    let readers = [
        child.stdout.take().map(|s| tail_reader(s, tail_lines)),
        child.stderr.take().map(|s| tail_reader(s, tail_lines)),
    ];
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if start.elapsed() >= limit => {
                kill_group(&mut child);
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => {
                kill_group(&mut child);
                return done(false, format!("waiting for the check command: {e}"), false);
            }
        }
    };
    // After a normal exit the pipes are at EOF unless a background child kept them open; either
    // way the wait is bounded, so a stray daemon can never hang the check.
    let mut text: Vec<String> = Vec::new();
    for r in readers.into_iter().flatten() {
        if let Ok(lines) = r.recv_timeout(DRAIN_GRACE) {
            text.extend(lines);
        }
    }
    let tail = text[text.len().saturating_sub(tail_lines)..].join("\n");
    match status {
        None => done(
            false,
            format!("check command timed out after {}", describe(limit)),
            true,
        ),
        Some(st) => done(st.success(), tail, false),
    }
}

/// "30 min", or seconds for a limit under a minute (tests use short ones).
fn describe(limit: Duration) -> String {
    if limit.as_secs() >= 60 {
        format!("{} min", limit.as_secs() / 60)
    } else {
        format!("{:.1} s", limit.as_secs_f64())
    }
}

/// Kill the command's process group (`sh` and everything it started), then reap `sh`.
fn kill_group(child: &mut Child) {
    let group = format!("-{}", child.id());
    let _ = Command::new("kill")
        .args(["-KILL", "--", &group])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill(); // in case the group kill itself could not run
    let _ = child.wait();
}

/// Read a pipe on its own thread, keeping the last `keep` lines; the result is sent once the
/// pipe closes.
fn tail_reader<R: Read + Send + 'static>(src: R, keep: usize) -> mpsc::Receiver<Vec<String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut ring: VecDeque<String> = VecDeque::with_capacity(keep + 1);
        let mut rd = BufReader::new(src);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match rd.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf);
                    ring.push_back(line.trim_end_matches(['\n', '\r']).to_string());
                    if ring.len() > keep {
                        ring.pop_front();
                    }
                }
            }
        }
        let _ = tx.send(ring.into());
    });
    rx
}

// ---------------------------------------------------------------- the milestone's record

/// One recorded check: Python's `{"where", "ok", "secs", "ts"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckRecord {
    /// `worktree` (the coder's folder, or the main folder in shared mode) or `branch` (the run
    /// branch in the main folder: the baseline, the final check, a merge).
    #[serde(rename = "where")]
    pub place: String,
    pub ok: bool,
    pub secs: f64,
    pub ts: i64,
}

/// What the run summary needs to know about one milestone.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TaskRecord {
    #[serde(default)]
    pub checks: Vec<CheckRecord>,
    /// Files changed outside the milestone's `Likely files` (Python `out_of_scope`).
    #[serde(default)]
    pub out_of_scope: Vec<String>,
}

/// `.orchestrate/records.json`: the per-milestone records, kept apart from state.db because only
/// the single driver writes them and a summary only reads them. Replaced atomically.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Records(pub BTreeMap<String, TaskRecord>);

impl Records {
    pub fn path(root: &Path) -> PathBuf {
        root.join(ORCH_DIR).join("records.json")
    }

    /// A missing or unreadable file is an empty record.
    pub fn load(root: &Path) -> Records {
        std::fs::read(Self::path(root))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let path = Self::path(root);
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| Error::io("serialize records", std::io::Error::other(e)))?;
        std::fs::write(&tmp, bytes).map_err(|e| Error::io("write records.json.tmp", e))?;
        std::fs::rename(&tmp, &path).map_err(|e| Error::io("rename records.json", e))
    }

    /// Append one check run to `task`'s record and save.
    pub fn record_check(root: &Path, task: &str, place: &str, run: &CheckRun) -> Result<()> {
        let mut all = Self::load(root);
        all.0.entry(task.to_string()).or_default().checks.push(CheckRecord {
            place: place.to_string(),
            ok: run.ok,
            secs: run.secs,
            ts: now_s(),
        });
        all.save(root)
    }

    /// Replace `task`'s out-of-scope list and save.
    pub fn set_out_of_scope(root: &Path, task: &str, files: &[String]) -> Result<()> {
        let mut all = Self::load(root);
        all.0.entry(task.to_string()).or_default().out_of_scope = files.to_vec();
        all.save(root)
    }
}

fn now_s() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: Duration = Duration::from_secs(30);

    fn here() -> PathBuf {
        std::env::current_dir().unwrap()
    }

    #[test]
    fn exit_zero_passes_and_an_empty_command_passes() {
        let r = run("echo fine", &here(), LONG, TAIL_LINES);
        assert!(r.ok && !r.timed_out);
        assert!(run("   ", &here(), LONG, TAIL_LINES).ok);
    }

    #[test]
    fn non_zero_fails_with_the_last_30_lines_of_stdout_and_stderr() {
        let r = run(
            "for i in $(seq 1 100); do echo out$i; done; echo ERR-LAST >&2; exit 2",
            &here(),
            LONG,
            TAIL_LINES,
        );
        assert!(!r.ok && !r.timed_out);
        assert_eq!(r.tail.lines().count(), 30);
        assert!(r.tail.ends_with("ERR-LAST"), "{}", r.tail);
        assert!(r.tail.contains("out100"));
        assert!(!r.tail.contains("out70\n"));
    }

    #[test]
    fn it_runs_in_the_given_folder() {
        let d = tempfile::tempdir().unwrap();
        let r = run("pwd; exit 1", d.path(), LONG, TAIL_LINES);
        let want = d.path().canonicalize().unwrap();
        assert_eq!(Path::new(r.tail.trim()).canonicalize().unwrap(), want);
    }

    #[test]
    fn a_hung_check_is_killed_with_its_children_and_says_it_timed_out() {
        let d = tempfile::tempdir().unwrap();
        let marker = d.path().join("child-survived");
        // a background child that would write the marker after 2s, and a foreground hang
        let cmd = format!("(sleep 2; touch {}) & sleep 30", marker.display());
        let t0 = Instant::now();
        let r = run(&cmd, d.path(), Duration::from_millis(300), TAIL_LINES);
        assert!(!r.ok && r.timed_out);
        assert!(r.tail.contains("timed out"), "{}", r.tail);
        assert!(t0.elapsed() < Duration::from_secs(10), "must not wait for the hang");
        std::thread::sleep(Duration::from_millis(2500));
        assert!(!marker.exists(), "the child process group was not killed");
    }

    #[test]
    fn a_background_child_holding_the_pipes_does_not_hang_a_finished_check() {
        let t0 = Instant::now();
        let r = run("sleep 20 & echo done", &here(), LONG, TAIL_LINES);
        assert!(r.ok);
        assert!(t0.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn the_env_timeout_defaults_to_30_minutes() {
        // not set in the test environment; a bad value falls back too
        if std::env::var("ORCH_CHECK_TIMEOUT_MIN").is_err() {
            assert_eq!(timeout_from_env(), Duration::from_secs(1800));
        }
    }

    #[test]
    fn records_keep_checks_and_strays_across_loads() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(ORCH_DIR)).unwrap();
        let ok = CheckRun { ok: true, tail: String::new(), secs: 1.5, timed_out: false };
        Records::record_check(d.path(), "M1", "worktree", &ok).unwrap();
        Records::record_check(d.path(), "M1", "branch", &ok).unwrap();
        Records::set_out_of_scope(d.path(), "M1", &["x.rs".to_string()]).unwrap();
        let r = Records::load(d.path());
        let m1 = &r.0["M1"];
        assert_eq!(m1.checks.len(), 2);
        assert_eq!(m1.checks[0].place, "worktree");
        assert_eq!(m1.checks[1].place, "branch");
        assert!(m1.checks[0].ts > 0);
        assert_eq!(m1.out_of_scope, vec!["x.rs"]);
        let raw = std::fs::read_to_string(Records::path(d.path())).unwrap();
        assert!(raw.contains("\"where\""), "{raw}");
    }
}
