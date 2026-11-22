//! The Herdr the app runs on. The app never bundles or copies Herdr: it finds the one on the
//! machine, and when there is none (or it is too old) it offers Herdr's own official route,
//! shown to the developer first and run only on their click:
//!
//! - install: the official installer script, fetched from <https://herdr.dev/install.sh> and
//!   shown word for word; exactly the text shown is run (`sh -s`), nothing else;
//! - update: `<herdr> update`, Herdr's own updater.
//!
//! When the app opens it makes sure Herdr's background server runs: a server that already runs
//! is used as it is (never replaced, never a second one), an installed Herdr that is not
//! running is started headless (`herdr server`). One check at a time, so two windows or
//! screens asking at once never start two servers. What the app started it stops when it
//! closes; what it found running it leaves alone.

use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The oldest Herdr whose commands orch uses.
pub const MIN_HERDR_VERSION: (u64, u64, u64) = (0, 9, 0);
/// Herdr's official site: where the app sends the developer when it cannot install from here.
pub const OFFICIAL_URL: &str = "https://herdr.dev";
/// Herdr's official installer script.
pub const INSTALL_SCRIPT_URL: &str = "https://herdr.dev/install.sh";
/// How the install is described next to the script it runs.
pub const INSTALL_COMMAND: &str = "curl -fsSL https://herdr.dev/install.sh | sh";
/// What the app says when there is no Herdr on the machine.
pub const NOT_FOUND: &str = "Herdr is not installed on this computer. Orchestra needs it to run \
the Lead and its agents: choose Install Herdr to install it from Herdr's official site \
(herdr.dev).";

/// Where Herdr stands, for the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrState {
    /// Herdr's server runs and the app uses it.
    Running,
    /// No Herdr on this computer: offer the install.
    Missing,
    /// Herdr is older than orch needs: offer the update.
    TooOld,
    /// Herdr is there but cannot be used (cannot be run, did not start): `error` says why.
    Failed,
}

/// A fix the page may offer: shown first, run only on the developer's click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixAction {
    Install,
    Update,
}

/// Herdr's status for the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrInfo {
    pub state: HerdrState,
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub running: bool,
    /// The app started this server (and stops it when it closes).
    pub managed: bool,
    pub too_old: bool,
    /// What went wrong and what to do, in plain words.
    pub error: Option<String>,
    /// The fix to offer, if one helps.
    pub fix: Option<FixAction>,
}

/// A fix ready to run: exactly what will run, for the developer to read before the click.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixPlan {
    pub action: FixAction,
    /// The command, as it would be typed.
    pub command: String,
    /// For the install: the installer script's full text, which is what runs.
    pub script: Option<String>,
    /// Hand it back to run this plan, and only this one.
    pub token: String,
}

/// Why Herdr could not be located.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotLocated {
    /// Nowhere on this computer.
    Missing,
    /// An explicit setting names something that cannot be run: it is not skipped silently.
    BadSetting(String),
}

type Locate = Box<dyn Fn() -> Result<PathBuf, NotLocated> + Send + Sync>;
type Fetch = Box<dyn Fn() -> Result<String, String> + Send + Sync>;

fn executable(p: &Path) -> bool {
    crate::serve::is_executable_file(p)
}

/// Find Herdr through `env`: `HERDR_BIN`, then orch's own `HERDR` setting (a path, or a name
/// looked up on the PATH), then the PATH, then where Herdr's official installer puts it
/// (`HERDR_INSTALL_DIR`, else `~/.local/bin`: a desktop-started app often lacks that on its
/// PATH, notably right after the install created it).
pub fn locate_in(env: &dyn Fn(&str) -> Option<OsString>) -> Result<PathBuf, NotLocated> {
    let set = |k: &str| env(k).filter(|v| !v.is_empty());
    let on_path = |name: &str| {
        set("PATH")
            .iter()
            .flat_map(std::env::split_paths)
            .map(|d| d.join(name))
            .find(|p| executable(p))
    };
    let bad = |k: &str, v: &OsString| {
        NotLocated::BadSetting(format!(
            "The {k} setting names {}, which is not a program this computer can run. Correct \
or remove that setting, then check again.",
            Path::new(v).display()
        ))
    };
    if let Some(v) = set("HERDR_BIN") {
        let p = PathBuf::from(&v);
        return if executable(&p) {
            Ok(p)
        } else {
            Err(bad("HERDR_BIN", &v))
        };
    }
    if let Some(v) = set("HERDR") {
        let p = PathBuf::from(&v);
        if p.components().count() > 1 {
            return if executable(&p) {
                Ok(p)
            } else {
                Err(bad("HERDR", &v))
            };
        }
        if let Some(found) = on_path(&p.to_string_lossy()) {
            return Ok(found);
        }
    }
    if let Some(found) = on_path("herdr") {
        return Ok(found);
    }
    let install_dir = set("HERDR_INSTALL_DIR")
        .map(PathBuf::from)
        .or_else(|| set("HOME").map(|h| PathBuf::from(h).join(".local/bin")));
    install_dir
        .map(|d| d.join("herdr"))
        .filter(|p| executable(p))
        .ok_or(NotLocated::Missing)
}

/// `(major, minor, patch)` from `herdr --version` output (`herdr 0.9.5`).
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    text.split_whitespace().find_map(|word| {
        let mut parts = word.trim_start_matches('v').split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts
            .next()
            .map(|s| {
                s.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
            })
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Some((major, minor, patch))
    })
}

fn run(cmd: &mut Command) -> std::io::Result<Output> {
    #[cfg(test)]
    let _spawning = crate::server::SPAWN
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    cmd.stdin(Stdio::null()).output()
}

fn version_of(path: &Path) -> Result<(u64, u64, u64), String> {
    let out = run(Command::new(path).arg("--version"))
        .map_err(|e| format!("Herdr at {} cannot be run ({e}).", path.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    parse_version(&text).ok_or_else(|| {
        format!(
            "Herdr at {} did not say which version it is (`--version` printed `{}`).",
            path.display(),
            text.trim()
        )
    })
}

/// Whether Herdr's server answers (`herdr status` reports it running).
fn server_runs(path: &Path) -> bool {
    run(Command::new(path).arg("status")).is_ok_and(|o| {
        o.status.success() && String::from_utf8_lossy(&o.stdout).contains("status: running")
    })
}

/// The last lines a command printed, for an error message.
fn tail(out: &Output) -> String {
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines[lines.len().saturating_sub(6)..].join("\n")
}

fn token_of(action: FixAction, command: &str, script: Option<&str>) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (action, command, script).hash(&mut h);
    format!("{:016x}", h.finish())
}

impl Hash for FixAction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (*self as u8).hash(state);
    }
}

/// The server this app started.
struct Started {
    child: Child,
    path: PathBuf,
}

/// The app's Herdr: found, started when needed, stopped when the app started it.
pub struct HerdrManager {
    locate: Locate,
    fetch: Fetch,
    start_timeout: Duration,
    /// One check (and start) at a time: never two servers.
    check: Mutex<Option<Started>>,
    /// The fix last shown to the developer.
    plan: Mutex<Option<FixPlan>>,
}

impl Default for HerdrManager {
    fn default() -> Self {
        HerdrManager::new(
            Box::new(|| locate_in(&|k| std::env::var_os(k))),
            Box::new(fetch_installer),
            Duration::from_secs(15),
        )
    }
}

/// Download Herdr's official installer script (to show it, then run exactly that text).
fn fetch_installer() -> Result<String, String> {
    let out = run(Command::new("curl").args([
        "-fsSL",
        "--proto",
        "=https",
        "--max-time",
        "30",
        INSTALL_SCRIPT_URL,
    ]))
    .map_err(|e| format!("the app could not download Herdr's installer (curl: {e})"))?;
    if !out.status.success() {
        return Err(format!(
            "the app could not download Herdr's installer from {INSTALL_SCRIPT_URL} ({})",
            tail(&out)
        ));
    }
    let script = String::from_utf8(out.stdout)
        .map_err(|_| "Herdr's installer is not text the app can show".to_string())?;
    if !script.starts_with("#!") {
        return Err(format!(
            "{INSTALL_SCRIPT_URL} did not answer with an installer script"
        ));
    }
    Ok(script)
}

impl HerdrManager {
    pub fn new(locate: Locate, fetch: Fetch, start_timeout: Duration) -> Self {
        HerdrManager {
            locate,
            fetch,
            start_timeout,
            check: Mutex::new(None),
            plan: Mutex::new(None),
        }
    }

    /// The Herdr executable, when there is one.
    pub fn binary_path(&self) -> Option<PathBuf> {
        (self.locate)().ok()
    }

    /// Whether the running server is one this app started.
    #[cfg(test)]
    pub fn is_started_by_app(&self) -> bool {
        self.check
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// Make sure Herdr's server runs, and say how Herdr stands. A running server is used as it
    /// is; an installed, new enough Herdr that is not running is started headless; nothing is
    /// started when Herdr is missing, too old or cannot be run. Calls are one at a time.
    pub fn ensure_running(&self) -> HerdrInfo {
        let mut started = self.check.lock().unwrap_or_else(|e| e.into_inner());
        // our server ended by itself: it is no longer ours to stop
        if let Some(s) = started.as_mut()
            && !matches!(s.child.try_wait(), Ok(None))
        {
            *started = None;
        }
        let info = |state, path: Option<&Path>, version: Option<(u64, u64, u64)>| HerdrInfo {
            state,
            installed: path.is_some(),
            path: path.map(|p| p.display().to_string()),
            version: version.map(|(a, b, c)| format!("{a}.{b}.{c}")),
            running: state == HerdrState::Running,
            managed: false,
            too_old: state == HerdrState::TooOld,
            error: None,
            fix: None,
        };
        let path = match (self.locate)() {
            Ok(p) => p,
            Err(NotLocated::Missing) => {
                return HerdrInfo {
                    error: Some(NOT_FOUND.into()),
                    fix: Some(FixAction::Install),
                    ..info(HerdrState::Missing, None, None)
                };
            }
            Err(NotLocated::BadSetting(msg)) => {
                return HerdrInfo {
                    error: Some(msg),
                    ..info(HerdrState::Failed, None, None)
                };
            }
        };
        let version = match version_of(&path) {
            Ok(v) => v,
            Err(e) => {
                return HerdrInfo {
                    error: Some(format!(
                        "{e} Update Herdr, or install it again from herdr.dev, then check again."
                    )),
                    fix: Some(FixAction::Update),
                    ..info(HerdrState::Failed, Some(&path), None)
                };
            }
        };
        if version < MIN_HERDR_VERSION {
            let (a, b, c) = MIN_HERDR_VERSION;
            return HerdrInfo {
                error: Some(format!(
                    "Herdr {}.{}.{} is too old: Orchestra needs Herdr {a}.{b}.{c} or newer. Choose \
Update Herdr to update it with Herdr's own updater.",
                    version.0, version.1, version.2
                )),
                fix: Some(FixAction::Update),
                ..info(HerdrState::TooOld, Some(&path), Some(version))
            };
        }
        if server_runs(&path) {
            return HerdrInfo {
                managed: started.as_ref().is_some_and(|s| s.path == path),
                ..info(HerdrState::Running, Some(&path), Some(version))
            };
        }
        let failed = |why: String| HerdrInfo {
            error: Some(format!(
                "Herdr is installed, but its background server did not start ({why}). Check \
again; if it keeps failing, open herdr.dev for help."
            )),
            ..info(HerdrState::Failed, Some(&path), Some(version))
        };
        let spawned = {
            #[cfg(test)]
            let _spawning = crate::server::SPAWN
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            Command::new(&path)
                .arg("server")
                .stdin(Stdio::null())
                // nothing reads them: a pipe would fill and stall the server
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        };
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => return failed(format!("it could not be run: {e}")),
        };
        let deadline = Instant::now() + self.start_timeout;
        loop {
            if server_runs(&path) {
                break;
            }
            if let Ok(Some(status)) = child.try_wait() {
                return failed(format!("`herdr server` ended at once with {status}"));
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return failed(format!(
                    "it did not answer within {}s",
                    self.start_timeout.as_secs()
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        *started = Some(Started {
            child,
            path: path.clone(),
        });
        HerdrInfo {
            managed: true,
            ..info(HerdrState::Running, Some(&path), Some(version))
        }
    }

    /// Prepare `action` for the developer to read: the exact command and, for the install, the
    /// official installer's full text. Nothing runs here.
    pub fn prepare_fix(&self, action: FixAction) -> Result<FixPlan, String> {
        let (command, script) = match action {
            FixAction::Install => (INSTALL_COMMAND.to_string(), Some((self.fetch)()?)),
            FixAction::Update => {
                let path = self.binary_path().ok_or_else(|| NOT_FOUND.to_string())?;
                (format!("{} update", path.display()), None)
            }
        };
        let plan = FixPlan {
            action,
            token: token_of(action, &command, script.as_deref()),
            command,
            script,
        };
        *self.plan.lock().unwrap_or_else(|e| e.into_inner()) = Some(plan.clone());
        Ok(plan)
    }

    /// Run the plan last shown, when `token` names it: exactly the script or command shown.
    /// Then make sure Herdr runs. The error carries what the installer or updater printed.
    pub fn run_fix(&self, token: &str) -> Result<HerdrInfo, String> {
        let plan = self
            .plan
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .filter(|p| p.token == token)
            .ok_or_else(|| {
                "That install or update is no longer the one shown: show it again, read it, \
then run it."
                    .to_string()
            })?;
        let (what, out) = match (&plan.action, &plan.script) {
            (FixAction::Install, Some(script)) => ("installer", run_script(script)),
            _ => {
                let path = self.binary_path().ok_or_else(|| NOT_FOUND.to_string())?;
                ("updater", run(Command::new(path).arg("update")))
            }
        };
        let out = out.map_err(|e| {
            format!("The Herdr {what} could not be run ({e}). Open herdr.dev and follow its instructions, then check again.")
        })?;
        if !out.status.success() {
            return Err(format!(
                "The Herdr {what} stopped with an error:\n{}\nOpen herdr.dev and follow its \
instructions, then check again.",
                tail(&out)
            ));
        }
        Ok(self.ensure_running())
    }

    /// Stop Herdr's server when this app started it (and it still runs); leave any other alone.
    pub fn stop_managed(&self) {
        let Some(mut s) = self.check.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return;
        };
        if !matches!(s.child.try_wait(), Ok(None)) {
            return; // it ended by itself: whatever runs now is not ours
        }
        let _ = run(Command::new(&s.path).args(["server", "stop"]));
        let deadline = Instant::now() + Duration::from_secs(3);
        while matches!(s.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = s.child.kill();
        let _ = s.child.wait();
    }
}

/// Run an installer script's text with `sh`, its output captured.
fn run_script(script: &str) -> std::io::Result<Output> {
    let mut child = {
        #[cfg(test)]
        let _spawning = crate::server::SPAWN
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        Command::new("sh")
            .arg("-s")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?
    };
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes())?;
    }
    child.wait_with_output()
}

impl Drop for HerdrManager {
    fn drop(&mut self) {
        self.stop_managed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("orch-gui-herdr-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_exe(path: &Path, body: &str) {
        let _w = crate::server::SPAWN
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A fake `herdr` in `d`: `--version` prints `version`; `status` says running while the
    /// file `d/running` exists; `server` counts its starts in `d/starts`, comes up after
    /// `delay` seconds and stays until stopped; `server stop` records `d/stopped`.
    fn fake_herdr(d: &Path, version: &str, delay: &str) -> PathBuf {
        let p = d.join("herdr");
        write_exe(
            &p,
            &format!(
                r#"D="{d}"
case "$1" in
  --version) echo "herdr {version}" ;;
  status) if [ -f "$D/running" ]; then printf 'server:\n  status: running\n'; else echo "not running" >&2; exit 1; fi ;;
  server)
    if [ "$2" = "stop" ]; then rm -f "$D/running"; touch "$D/stopped"; exit 0; fi
    echo x >> "$D/starts"
    sleep {delay}
    touch "$D/running"
    while [ -f "$D/running" ]; do sleep 0.05; done ;;
  update) echo "updated"; exit 0 ;;
esac"#,
                d = d.display()
            ),
        );
        p
    }

    fn manager_at(path: PathBuf) -> HerdrManager {
        HerdrManager::new(
            Box::new(move || Ok(path.clone())),
            Box::new(|| Err("no network in tests".into())),
            Duration::from_secs(5),
        )
    }

    fn starts(d: &Path) -> usize {
        std::fs::read_to_string(d.join("starts"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    #[test]
    fn parses_version_formats() {
        assert_eq!(parse_version("herdr 0.9.5"), Some((0, 9, 5)));
        assert_eq!(parse_version("herdr v0.9.5"), Some((0, 9, 5)));
        assert_eq!(parse_version("herdr 1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.8"), Some((0, 8, 0)));
        assert_eq!(parse_version("invalid"), None);
    }

    // AC1 + AC5: a server that already runs is used as it is, no second one is started, and it
    // is left alone when the app closes.
    #[test]
    fn a_running_herdr_is_used_never_replaced_and_left_alone() {
        let d = scratch("running");
        let herdr = fake_herdr(&d, "0.9.5", "0");
        std::fs::write(d.join("running"), "").unwrap();
        let m = manager_at(herdr);
        let info = m.ensure_running();
        assert_eq!(info.state, HerdrState::Running);
        assert!(
            info.running && !info.managed && info.error.is_none(),
            "{info:?}"
        );
        assert_eq!(info.version.as_deref(), Some("0.9.5"));
        assert_eq!(starts(&d), 0, "no second server");
        m.stop_managed();
        drop(m);
        assert!(
            !d.join("stopped").exists(),
            "a Herdr the app did not start is not stopped"
        );
        assert!(d.join("running").exists());
    }

    // AC1 + AC5: an installed Herdr that is not running is started headless once, and that
    // server is stopped when the app closes.
    #[test]
    fn an_installed_herdr_is_started_once_and_stopped_on_close() {
        let d = scratch("started");
        let herdr = fake_herdr(&d, "0.9.5", "0.1");
        let m = manager_at(herdr);
        let info = m.ensure_running();
        assert_eq!(info.state, HerdrState::Running, "{info:?}");
        assert!(info.managed && m.is_started_by_app());
        // asking again uses the same server
        assert!(m.ensure_running().managed);
        assert_eq!(starts(&d), 1);
        m.stop_managed();
        assert!(
            d.join("stopped").exists(),
            "the app's own server is stopped"
        );
        assert!(!m.is_started_by_app());
    }

    // AC1: checks that overlap (startup, Onboarding, the Lead start) never start two servers.
    #[test]
    fn overlapping_checks_start_exactly_one_server() {
        let d = scratch("concurrent");
        let herdr = fake_herdr(&d, "0.9.5", "0.4");
        let m = Arc::new(manager_at(herdr));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let m = m.clone();
                std::thread::spawn(move || m.ensure_running())
            })
            .collect();
        for t in threads {
            assert_eq!(t.join().unwrap().state, HerdrState::Running);
        }
        assert_eq!(starts(&d), 1, "exactly one `herdr server`");
        m.stop_managed();
    }

    // AC1: a Herdr that is too old is said so in plain words, offered Herdr's own update, and
    // not started.
    #[test]
    fn a_too_old_herdr_is_reported_and_not_started() {
        let d = scratch("old");
        let m = manager_at(fake_herdr(&d, "0.8.2", "0"));
        let info = m.ensure_running();
        assert_eq!(
            (info.state, info.fix),
            (HerdrState::TooOld, Some(FixAction::Update))
        );
        let e = info.error.unwrap();
        assert!(e.contains("0.8.2 is too old") && e.contains("0.9.0"), "{e}");
        assert_eq!(starts(&d), 0);
        // the update shown is the command that runs
        let plan = m.prepare_fix(FixAction::Update).unwrap();
        assert_eq!(
            plan.command,
            format!("{} update", d.join("herdr").display())
        );
        assert!(plan.script.is_none());
    }

    // AC4: no Herdr at all is plain words with the install offered; a broken one is Failed.
    #[test]
    fn missing_and_unusable_herdr_say_what_to_do() {
        let m = HerdrManager::new(
            Box::new(|| Err(NotLocated::Missing)),
            Box::new(|| Err("offline".into())),
            Duration::from_secs(1),
        );
        let info = m.ensure_running();
        assert_eq!((info.state, info.installed), (HerdrState::Missing, false));
        assert_eq!(info.fix, Some(FixAction::Install));
        assert_eq!(info.error.as_deref(), Some(NOT_FOUND));
        // the install cannot be prepared without the official script: the error says why
        assert!(
            m.prepare_fix(FixAction::Install)
                .unwrap_err()
                .contains("offline")
        );

        let d = scratch("broken");
        let broken = d.join("herdr");
        write_exe(&broken, "echo 'something else'");
        let info = manager_at(broken).ensure_running();
        assert_eq!(info.state, HerdrState::Failed);
        assert!(info.error.unwrap().contains("did not say which version"));

        // a server that never comes up is a plain failure, and nothing is left running
        let d = scratch("never");
        let never = d.join("herdr");
        write_exe(
            &never,
            "case \"$1\" in --version) echo 'herdr 0.9.5';; status) exit 1;; server) exit 3;; esac",
        );
        let info = manager_at(never).ensure_running();
        assert_eq!(info.state, HerdrState::Failed);
        assert!(info.error.unwrap().contains("did not start"));
    }

    // AC2 + AC3: the install runs exactly the script shown, only with its token, and leaves a
    // running Herdr where the official installer puts it; a failing installer's own words reach
    // the developer.
    #[test]
    fn the_install_runs_exactly_the_script_shown() {
        let d = scratch("install");
        let bin = d.join("bin");
        let target = bin.join("herdr");
        let fake = fake_herdr(&d, "0.9.5", "0");
        let script = format!(
            "#!/bin/sh\nset -eu\nmkdir -p '{}'\ncp '{}' '{}'\necho installed\n",
            bin.display(),
            fake.display(),
            target.display()
        );
        let t2 = target.clone();
        let m = HerdrManager::new(
            Box::new(move || {
                if executable(&t2) {
                    Ok(t2.clone())
                } else {
                    Err(NotLocated::Missing)
                }
            }),
            Box::new(move || Ok(script.clone())),
            Duration::from_secs(5),
        );
        assert_eq!(m.ensure_running().state, HerdrState::Missing);
        let plan = m.prepare_fix(FixAction::Install).unwrap();
        assert_eq!(plan.command, INSTALL_COMMAND);
        assert!(plan.script.as_deref().unwrap().contains("echo installed"));
        // another token runs nothing
        assert!(m.run_fix("0000").is_err());
        assert!(!target.exists());
        let plan = m.prepare_fix(FixAction::Install).unwrap();
        let info = m.run_fix(&plan.token).unwrap();
        assert_eq!(info.state, HerdrState::Running, "{info:?}");
        assert_eq!(info.path.as_deref(), Some(target.to_str().unwrap()));
        // a plan runs once
        assert!(m.run_fix(&plan.token).is_err());
        m.stop_managed();

        let failing = HerdrManager::new(
            Box::new(|| Err(NotLocated::Missing)),
            Box::new(|| {
                Ok("#!/bin/sh\necho 'error: unsupported architecture: sparc' >&2\nexit 1\n".into())
            }),
            Duration::from_secs(1),
        );
        let plan = failing.prepare_fix(FixAction::Install).unwrap();
        let e = failing.run_fix(&plan.token).unwrap_err();
        assert!(e.contains("unsupported architecture: sparc"), "{e}");
    }

    // Herdr is found where its official installer puts it even when that folder is not on the
    // PATH; explicit settings are honoured, and a wrong one is reported, not skipped.
    #[test]
    fn herdr_is_located_from_settings_path_and_the_install_folder() {
        let d = scratch("locate");
        let home_bin = d.join("home/.local/bin/herdr");
        write_exe(&home_bin, "exit 0");
        let on_path = d.join("path/herdr");
        write_exe(&on_path, "exit 0");
        let env = |pairs: Vec<(&'static str, OsString)>| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone())
        };
        let home = || ("HOME", d.join("home").into_os_string());
        // not on the PATH: the installer's folder
        assert_eq!(locate_in(&env(vec![home()])), Ok(home_bin.clone()));
        // the PATH comes first
        let path = ("PATH", d.join("path").into_os_string());
        assert_eq!(
            locate_in(&env(vec![home(), path.clone()])),
            Ok(on_path.clone())
        );
        // HERDR_INSTALL_DIR moves the installer's folder
        let other = d.join("custom/herdr");
        write_exe(&other, "exit 0");
        let dir = ("HERDR_INSTALL_DIR", d.join("custom").into_os_string());
        assert_eq!(locate_in(&env(vec![home(), dir])), Ok(other.clone()));
        // explicit settings win, and a wrong one is an error in plain words
        let bin = ("HERDR_BIN", other.clone().into_os_string());
        assert_eq!(locate_in(&env(vec![bin, path.clone()])), Ok(other));
        let wrong = ("HERDR_BIN", d.join("nope").into_os_string());
        match locate_in(&env(vec![wrong, path])) {
            Err(NotLocated::BadSetting(m)) => assert!(m.contains("HERDR_BIN"), "{m}"),
            r => panic!("{r:?}"),
        }
        // nothing anywhere
        let empty = ("HOME", d.join("empty").into_os_string());
        assert_eq!(locate_in(&env(vec![empty])), Err(NotLocated::Missing));
    }
}
