//! Setup freshness (§13, §16): the plan's setup command (`npm install`, ...) runs in a folder
//! when none ran there before, or when the fingerprint of the dependency files changed since
//! its last run there, and not otherwise.
//!
//! The fingerprint covers the files in the folder's git index that match the plan's
//! `Setup inputs:` globs (default: lockfiles and manifests) — their paths and blob ids, so a
//! change in the index shows, a rebuild of ignored files does not. Fingerprints are stored per
//! folder in the database (migration 6) and so survive a restart.

use crate::error::{Error, Result};
use crate::git;
use crate::run_store::RunSettings;
use crate::store::{Store, now};
use globset::{Glob, GlobSetBuilder};
use rusqlite::{OptionalExtension, params};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// File names (any folder) that count as dependency files when the plan gives no globs.
const LOCKFILES: &[&str] = &[
    "package.json",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "pubspec.yaml",
    "pubspec.lock",
    "go.mod",
    "go.sum",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "cargo.toml",
    "cargo.lock",
    "pyproject.toml",
    "poetry.lock",
    "uv.lock",
    "pipfile",
    "pipfile.lock",
    "gemfile",
    "gemfile.lock",
    "composer.json",
    "composer.lock",
];

/// Whether `path` is a dependency file by name (Python's `LOCKFILE_RE`, case-insensitive).
pub fn is_lockfile(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    if LOCKFILES.contains(&name.as_str()) {
        return true;
    }
    name.starts_with("requirements") && name.ends_with(".txt")
}

/// Whether `path` is selected by the plan's globs (Python's `path_allowed`: equal, a glob match,
/// or inside a listed folder).
pub fn path_allowed(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| {
        path == p
            || path.starts_with(&format!("{}/", p.trim_end_matches('/')))
            || Glob::new(p).is_ok_and(|g| {
                GlobSetBuilder::new()
                    .add(g)
                    .build()
                    .is_ok_and(|s| s.is_match(path))
            })
    })
}

/// The fingerprint of the dependency files in `dir`'s git index: a short hash, or `none` when
/// there are no such files.
pub fn fingerprint(dir: &Path, patterns: &[String]) -> Result<String> {
    let files = git::check(dir, &["ls-files"])?;
    let selected: Vec<&str> = files
        .lines()
        .filter(|f| !f.is_empty())
        .filter(|f| {
            if patterns.is_empty() {
                is_lockfile(f)
            } else {
                path_allowed(f, patterns)
            }
        })
        .collect();
    if selected.is_empty() {
        return Ok("none".into());
    }
    let mut args = vec!["ls-files", "-s", "--"];
    args.extend(selected);
    let staged = git::check(dir, &args)?;
    let hash = git::run_stdin(dir, &["hash-object", "--stdin"], &staged)?;
    if !hash.success() {
        return Err(git::failed(dir, &["hash-object", "--stdin"], &hash));
    }
    Ok(hash.out().chars().take(16).collect())
}

/// Why the setup command should run in a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupNeed {
    /// No setup command, or the folder is up to date.
    No,
    /// It never ran in this folder.
    First,
    /// It ran, but the dependency files changed since.
    Changed,
}

/// Whether the plan has a setup command (`none`, any case, or empty means no).
pub fn has_setup(settings: &RunSettings) -> bool {
    let c = settings.setup_command.trim();
    !c.is_empty() && !c.eq_ignore_ascii_case("none")
}

impl Store {
    /// The fingerprint recorded for a folder's last setup run; `None` when it never ran there.
    pub fn setup_fingerprint(&self, folder: &Path) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT fingerprint FROM setup_fingerprints WHERE folder=?1",
                params![folder.to_string_lossy()],
                |r| r.get(0),
            )
            .optional()
            .map_err(crate::store::StoreError::from)?)
    }

    /// Record that setup is done in `folder` for the dependency files fingerprinted `fp`.
    pub fn record_setup(&mut self, folder: &Path, fp: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO setup_fingerprints (folder, fingerprint, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(folder) DO UPDATE SET fingerprint=?2, updated_at=?3",
                params![folder.to_string_lossy(), fp, now()],
            )
            .map_err(crate::store::StoreError::from)?;
        Ok(())
    }

    /// Forget a folder's setup (its worktree was created fresh or removed).
    pub fn forget_setup(&mut self, folder: &Path) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM setup_fingerprints WHERE folder=?1",
                params![folder.to_string_lossy()],
            )
            .map_err(crate::store::StoreError::from)?;
        Ok(())
    }
}

/// Whether `dir` needs the setup command now, and why.
pub fn setup_need(store: &Store, settings: &RunSettings, dir: &Path) -> Result<SetupNeed> {
    if !has_setup(settings) {
        return Ok(SetupNeed::No);
    }
    let Some(recorded) = store.setup_fingerprint(dir)? else {
        return Ok(SetupNeed::First);
    };
    Ok(if recorded == fingerprint(dir, &settings.setup_inputs)? {
        SetupNeed::No
    } else {
        SetupNeed::Changed
    })
}

/// `orch setup-done`: record the folder's current fingerprint.
pub fn mark_setup_done(store: &mut Store, settings: &RunSettings, dir: &Path) -> Result<()> {
    let fp = fingerprint(dir, &settings.setup_inputs)?;
    store.record_setup(dir, &fp)
}

/// Minutes the setup command may run (`ORCH_CHECK_TIMEOUT_MIN`, as the check command).
fn timeout() -> Duration {
    let min = std::env::var("ORCH_CHECK_TIMEOUT_MIN")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(30);
    Duration::from_secs(min.max(1) * 60)
}

/// Kill the command's process group (`sh` and everything it started), then reap `sh`.
fn kill_group(child: &mut std::process::Child) {
    let group = format!("-{}", child.id());
    let _ = Command::new("kill")
        .args(["-KILL", "--", &group])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

/// Run `command` through the shell in `dir`. `Err` carries the last lines of its output.
pub fn run_command(command: &str, dir: &Path, limit: Duration) -> std::result::Result<(), String> {
    let mut child = Command::new("sh")
        .args(["-c", command])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("could not run `{command}` in {}: {e}", dir.display()))?;
    let drain = |s: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(mut s) = s {
                let _ = s.read_to_string(&mut buf);
            }
            buf
        })
    };
    let out = drain(child.stdout.take().map(|s| Box::new(s) as _));
    let err = drain(child.stderr.take().map(|s| Box::new(s) as _));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if start.elapsed() > limit => {
                kill_group(&mut child);
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                kill_group(&mut child);
                return Err(format!("waiting for the setup command: {e}"));
            }
        }
    };
    let text = format!("{}{}", out.join().unwrap_or_default(), err.join().unwrap_or_default());
    match status {
        None => Err(format!("setup command timed out in {}", dir.display())),
        Some(st) if st.success() => Ok(()),
        Some(_) => {
            let lines: Vec<&str> = text.lines().collect();
            let tail = lines[lines.len().saturating_sub(30)..].join("\n");
            Err(format!("setup command failed in {}:\n{tail}", dir.display()))
        }
    }
}

/// Run the setup command in `dir` when [`setup_need`] says so, and record the fingerprint after
/// it succeeded. Returns what ran (`Some(why)`), or `None` when `dir` was up to date.
pub fn ensure_setup(
    store: &mut Store,
    settings: &RunSettings,
    dir: &Path,
) -> Result<Option<SetupNeed>> {
    let need = setup_need(store, settings, dir)?;
    if need == SetupNeed::No {
        return Ok(None);
    }
    run_command(&settings.setup_command, dir, timeout())
        .map_err(|m| Error::wrap(std::io::Error::other(m), 2))?;
    mark_setup_done(store, settings, dir)?;
    Ok(Some(need))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::*;

    fn settings(cmd: &str, inputs: &[&str]) -> RunSettings {
        RunSettings {
            branch: "feat/x".into(),
            base: None,
            title: String::new(),
            commits: "per-milestone".into(),
            isolation: "worktree".into(),
            max_parallel: 2,
            setup_inputs: inputs.iter().map(|s| s.to_string()).collect(),
            scope: "warn".into(),
            risk_hold: true,
            budget_s: None,
            check_command: String::new(),
            setup_command: cmd.into(),
            lead_mode: "off".into(),
            reviewer: None,
        }
    }

    fn store(t: &Path) -> Store {
        Store::open_in(&t.join(".orchestrate")).unwrap()
    }

    #[test]
    fn lockfile_names() {
        for f in ["Cargo.lock", "a/b/package.json", "requirements-dev.txt", "web/Gemfile.lock", "Pipfile"] {
            assert!(is_lockfile(f), "{f}");
        }
        for f in ["src/main.rs", "README.md", "requirements.md", "notpackage.json.bak"] {
            assert!(!is_lockfile(f), "{f}");
        }
    }

    #[test]
    fn globs_files_and_folders() {
        let pats = vec!["deps/*.lock".to_string(), "vendor".to_string(), "x.cfg".to_string()];
        assert!(path_allowed("deps/a.lock", &pats));
        assert!(path_allowed("vendor/a/b.txt", &pats));
        assert!(path_allowed("x.cfg", &pats));
        assert!(!path_allowed("src/a.lock", &pats));
    }

    #[test]
    fn fingerprint_follows_dependency_files_only() {
        let (_t, root) = repo();
        let none = fingerprint(&root, &[]).unwrap();
        assert_ne!(none, "none", "Cargo.toml is a manifest");
        std::fs::write(root.join("src/a.txt"), "other\n").unwrap();
        g(&root, &["add", "-A"]);
        assert_eq!(fingerprint(&root, &[]).unwrap(), none, "source change is not a dependency change");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        g(&root, &["add", "-A"]);
        assert_ne!(fingerprint(&root, &[]).unwrap(), none);
        assert_eq!(fingerprint(&root, &["nothing/*".to_string()]).unwrap(), "none");
    }

    #[test]
    fn need_first_then_no_then_changed_and_it_survives_a_reopen() {
        let (t, root) = repo();
        let s = settings("true", &[]);
        let mut st = store(t.path());
        assert_eq!(setup_need(&st, &s, &root).unwrap(), SetupNeed::First);
        assert_eq!(ensure_setup(&mut st, &s, &root).unwrap(), Some(SetupNeed::First));
        assert_eq!(setup_need(&st, &s, &root).unwrap(), SetupNeed::No);
        drop(st);
        let mut st = store(t.path()); // "restart"
        assert_eq!(setup_need(&st, &s, &root).unwrap(), SetupNeed::No);
        std::fs::write(root.join("Cargo.toml"), "[package]\nname=\"y\"\n").unwrap();
        g(&root, &["add", "-A"]);
        assert_eq!(setup_need(&st, &s, &root).unwrap(), SetupNeed::Changed);
        assert_eq!(ensure_setup(&mut st, &s, &root).unwrap(), Some(SetupNeed::Changed));
        assert_eq!(ensure_setup(&mut st, &s, &root).unwrap(), None);
        st.forget_setup(&root).unwrap();
        assert_eq!(setup_need(&st, &s, &root).unwrap(), SetupNeed::First);
    }

    #[test]
    fn no_setup_command_never_needs_setup() {
        let (t, root) = repo();
        let st = store(t.path());
        for c in ["none", "None", "", "  "] {
            assert_eq!(setup_need(&st, &settings(c, &[]), &root).unwrap(), SetupNeed::No);
        }
    }

    #[test]
    fn a_failing_setup_records_nothing_and_shows_the_output() {
        let (t, root) = repo();
        let mut st = store(t.path());
        let s = settings("echo boom >&2; exit 3", &[]);
        let e = ensure_setup(&mut st, &s, &root).unwrap_err();
        assert!(e.to_string().contains("boom"), "{e}");
        assert_eq!(st.setup_fingerprint(&root).unwrap(), None);
    }

    #[test]
    fn a_hanging_setup_times_out() {
        let (_t, root) = repo();
        let e = run_command("sleep 30", &root, Duration::from_millis(200)).unwrap_err();
        assert!(e.contains("timed out"), "{e}");
    }

    #[test]
    fn a_hung_setup_is_killed_with_its_children_and_says_it_timed_out() {
        let d = tempfile::tempdir().unwrap();
        let marker = d.path().join("setup-child-survived");
        let cmd = format!("(sleep 2; touch {}) & sleep 30", marker.display());
        let t0 = Instant::now();
        let r = run_command(&cmd, d.path(), Duration::from_millis(300));
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("timed out"));
        assert!(t0.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(2500));
        assert!(!marker.exists(), "the child process group was not killed");
    }
}
