//! The `orch serve` child processes: one per open project. Start one for a project folder, learn
//! its address and token, stop it (or all of them when the app quits). It has no knowledge of runs, milestones or agents.

use crate::serve::{
    allowed_origins, find_in_environment, parse_listening, parse_token_path, read_token,
};
use serde::Serialize;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

/// What the page needs to talk to the server; the only thing the shell hands over.
#[derive(Debug, Clone, Serialize)]
pub struct Connection {
    pub url: String,
    pub token: String,
}

struct Running {
    child: Child,
    connection: Connection,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The servers this app started, one per open project. A project is identified by its folder;
/// every server has its own process, port and (shared on disk) token.
pub struct Servers {
    running: Mutex<HashMap<String, Running>>,
    /// Projects whose server is being started right now (a second start waits for the first).
    starting: Mutex<HashSet<String>>,
    /// How long `orch serve` gets to print its address and token file.
    start_timeout: Duration,
}

impl Default for Servers {
    fn default() -> Self {
        Servers::with_timeout(Duration::from_secs(10))
    }
}

/// The key a project's server is stored under: the project's one canonical spelling (symlinks
/// resolved, no trailing slash), the same one the project list stores.
pub fn key_of(project: &Path) -> String {
    crate::projects::canonical(&project.display().to_string())
}

/// Removes the project from `starting` however the start ends.
struct StartingGuard<'a>(&'a Servers, String);
impl Drop for StartingGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut s) = self.0.starting.lock() {
            s.remove(&self.1);
        }
    }
}

impl Servers {
    pub fn with_timeout(start_timeout: Duration) -> Servers {
        Servers {
            running: Mutex::new(HashMap::new()),
            starting: Mutex::new(HashSet::new()),
            start_timeout,
        }
    }

    /// Stop one project's server, if it runs. Safe to call twice; other projects are untouched.
    pub fn stop(&self, project: &Path) {
        let removed = self
            .running
            .lock()
            .ok()
            .and_then(|mut g| g.remove(&key_of(project)));
        drop(removed); // kills the process (Drop), outside the lock
    }

    /// Stop every server (the app is quitting).
    pub fn stop_all(&self) {
        let all: Vec<Running> = match self.running.lock() {
            Ok(mut g) => g.drain().map(|(_, r)| r).collect(),
            Err(_) => return,
        };
        drop(all);
    }

    /// The project's running server; forgets one that has exited.
    pub fn current(&self, project: &Path) -> Option<Connection> {
        let key = key_of(project);
        let mut guard = self.running.lock().ok()?;
        let running = guard.get_mut(&key)?;
        if !matches!(running.child.try_wait(), Ok(None)) {
            guard.remove(&key);
            return None;
        }
        guard.get(&key).map(|r| r.connection.clone())
    }

    /// The folders that have a live server.
    pub fn open_paths(&self) -> Vec<String> {
        let keys: Vec<String> = match self.running.lock() {
            Ok(g) => g.keys().cloned().collect(),
            Err(_) => return Vec::new(),
        };
        let mut live: Vec<String> = keys
            .into_iter()
            .filter(|k| self.current(Path::new(k)).is_some())
            .collect();
        live.sort();
        live
    }

    /// Start `orch serve` in `project`, or return the one already running for it. The `orch`
    /// binary is `ORCH_BIN`, else the first one on the PATH.
    pub fn start(&self, project: &Path) -> Result<Connection, String> {
        self.start_using(project, None)
    }

    /// [`start`](Self::start) with the `orch` path from the app's settings: when it is set
    /// it wins over `ORCH_BIN` and the PATH.
    pub fn start_using(
        &self,
        project: &Path,
        orch_setting: Option<String>,
    ) -> Result<Connection, String> {
        if !project.is_dir() {
            return Err(format!("{} is not a folder", project.display()));
        }
        if let Some(c) = self.current(project) {
            return Ok(c);
        }
        let key = key_of(project);
        {
            let mut s = self.starting.lock().map_err(|_| "the server list is locked")?;
            if !s.insert(key.clone()) {
                return Err(format!("the server for {} is already starting", project.display()));
            }
        }
        let _guard = StartingGuard(self, key);
        let orch = match orch_setting.filter(|s| !s.is_empty()) {
            Some(path) => crate::serve::find_binary(
                "orch",
                "the orch path in Settings",
                Some(path.into()),
                None,
            )?,
            None => find_in_environment("orch", "ORCH_BIN")?,
        };
        self.start_with(&orch, project)
    }

    /// Stop this project's server (if any) and start a fresh one: a retry.
    pub fn restart_using(
        &self,
        project: &Path,
        orch_setting: Option<String>,
    ) -> Result<Connection, String> {
        self.stop(project);
        self.start_using(project, orch_setting)
    }

    fn start_with(&self, orch: &Path, project: &Path) -> Result<Connection, String> {
        let mut command = Command::new(orch);
        command.arg("serve").args(["--port", "0"]);
        for origin in allowed_origins() {
            command.args(["--allow-origin", origin]);
        }
        #[cfg(test)]
        let _spawning = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let mut child = command
            .current_dir(project)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", orch.display()))?;

        let stdout = child.stdout.take().ok_or("no output from orch serve")?;
        let stderr = child.stderr.take();
        let (tx, rx) = mpsc::channel::<String>();
        // Keep reading after start-up so the pipe never fills and blocks the server.
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });

        let mut url = None;
        let mut token_path = None;
        let deadline = std::time::Instant::now() + self.start_timeout;
        while url.is_none() || token_path.is_none() {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match rx.recv_timeout(left) {
                Ok(line) => {
                    url = url.or_else(|| parse_listening(&line));
                    token_path = token_path.or_else(|| parse_token_path(&line));
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let said = stderr
                        .map(|mut s| {
                            let mut text = String::new();
                            let _ = s.by_ref().take(2000).read_to_string(&mut text);
                            text.trim().to_string()
                        })
                        .unwrap_or_default();
                    return Err(if said.is_empty() {
                        "orch serve did not report its address in time".to_string()
                    } else {
                        format!("orch serve did not start: {said}")
                    });
                }
            }
        }
        let (Some(url), Some(token_path)) = (url, token_path) else {
            unreachable!("the loop ends only with both set")
        };
        let token = match read_token(&token_path) {
            Ok(t) => t,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        let connection = Connection { url, token };
        if let Ok(mut guard) = self.running.lock() {
            guard.insert(
                key_of(project),
                Running {
                    child,
                    connection: connection.clone(),
                },
            );
        }
        Ok(connection)
    }
}

/// Writing an executable while another thread forks makes the exec fail with "text file busy";
/// tests take this lock around writing a fake `orch` and around every spawn (also `git`'s).
#[cfg(test)]
pub(crate) static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A stand-in for `orch` that prints `lines` like `orch serve` and then waits.
    fn fake_orch(dir: &Path, script: &str) -> std::path::PathBuf {
        let _w = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let p = dir.join("orch");
        std::fs::write(&p, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("orch-gui-srv-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn starts_in_the_project_and_returns_the_url_and_the_token_from_the_named_file() {
        let d = scratch("ok");
        std::fs::write(d.join("api-token"), "s3cret\n").unwrap();
        let orch = fake_orch(
            &d,
            &format!(
                "echo \"args: $@\" >&2\npwd > {0}/cwd\necho 'orch serve: listening on http://127.0.0.1:4242'\necho 'project: x'\necho 'token: read it from {0}/api-token and send `Authorization: Bearer <token>`'\nexec sleep 30",
                d.display()
            ),
        );
        let project = scratch("ok-project");
        let state = Servers::with_timeout(Duration::from_secs(5));
        let c = state.start_with(&orch, &project).unwrap();
        assert_eq!(
            (c.url.as_str(), c.token.as_str()),
            ("http://127.0.0.1:4242", "s3cret")
        );
        assert_eq!(
            state.current(&project).map(|c| c.url),
            Some("http://127.0.0.1:4242".into())
        );
        let cwd = std::fs::read_to_string(d.join("cwd")).unwrap();
        assert_eq!(
            std::fs::canonicalize(cwd.trim()).unwrap(),
            std::fs::canonicalize(&project).unwrap()
        );
        state.stop(&project);
        assert!(state.current(&project).is_none());
        state.stop(&project); // twice is fine
    }

    #[test]
    fn stopping_ends_the_process() {
        let d = scratch("stop");
        std::fs::write(d.join("api-token"), "t").unwrap();
        let orch = fake_orch(
            &d,
            &format!(
                "echo $$ > {0}/pid\necho 'orch serve: listening on http://127.0.0.1:1'\necho 'token: read it from {0}/api-token and send x'\nexec sleep 30",
                d.display()
            ),
        );
        let state = Servers::with_timeout(Duration::from_secs(5));
        state.start_with(&orch, &d).unwrap();
        let pid = std::fs::read_to_string(d.join("pid"))
            .unwrap()
            .trim()
            .to_string();
        state.stop(&d);
        let gone = (0..50).any(|_| {
            std::thread::sleep(Duration::from_millis(50));
            !Path::new(&format!("/proc/{pid}")).exists()
        });
        assert!(gone, "process {pid} is still running after stop");
    }

    #[test]
    fn a_server_that_exits_is_reported_with_what_it_said() {
        let d = scratch("fail");
        let orch = fake_orch(&d, "echo 'cannot listen: address in use' >&2\nexit 1");
        let state = Servers::with_timeout(Duration::from_secs(5));
        let e = state.start_with(&orch, &d).unwrap_err();
        assert!(e.contains("address in use"), "{e}");
        assert!(state.current(&d).is_none());
    }

    #[test]
    fn a_silent_server_times_out_and_is_killed() {
        let d = scratch("silent");
        let orch = fake_orch(&d, "exec sleep 30");
        let state = Servers::with_timeout(Duration::from_millis(300));
        let e = state.start_with(&orch, &d).unwrap_err();
        assert!(e.contains("did not report its address"), "{e}");
    }

    #[test]
    fn an_unreadable_token_file_stops_the_server_and_says_so() {
        let d = scratch("notoken");
        let orch = fake_orch(
            &d,
            &format!(
                "echo 'orch serve: listening on http://127.0.0.1:1'\necho 'token: read it from {0}/missing and send x'\nexec sleep 30",
                d.display()
            ),
        );
        let state = Servers::with_timeout(Duration::from_secs(5));
        let e = state.start_with(&orch, &d).unwrap_err();
        assert!(e.contains("cannot read the API token file"), "{e}");
        assert!(state.current(&d).is_none());
    }

    #[test]
    fn refuses_a_folder_that_does_not_exist() {
        let state = Servers::default();
        let e = state.start(Path::new("/nonexistent/project")).unwrap_err();
        assert!(e.contains("is not a folder"), "{e}");
    }

    /// A fake `orch` that records its pid and the folder it runs in, listens on `port`.
    fn fake_serving(d: &Path, tag: &str, port: u16) -> std::path::PathBuf {
        let tokens = d.join(format!("{tag}-token"));
        std::fs::write(&tokens, format!("token-{tag}\n")).unwrap();
        fake_orch(
            d,
            &format!(
                "echo $$ > {0}/{1}-pid\necho 'orch serve: listening on http://127.0.0.1:{2}'\necho 'token: read it from {3} and send x'\nexec sleep 30",
                d.display(),
                tag,
                port,
                tokens.display()
            ),
        )
    }

    fn alive(pid: &str) -> bool {
        Path::new(&format!("/proc/{pid}")).exists()
    }

    fn gone(pid: &str) -> bool {
        (0..60).any(|_| {
            std::thread::sleep(Duration::from_millis(50));
            !alive(pid)
        })
    }

    #[test]
    fn two_projects_run_at_once_each_with_its_own_server_and_token() {
        let d = scratch("two");
        let (a, b) = (scratch("two-a"), scratch("two-b"));
        let state = Servers::with_timeout(Duration::from_secs(5));
        let orch_a = fake_serving(&d, "a", 4001);
        let ca = state.start_with(&orch_a, &a).unwrap();
        // a second fake with another name, so each start reports its own address
        let d2 = scratch("two-bin");
        let orch_b = fake_serving(&d2, "b", 4002);
        let cb = state.start_with(&orch_b, &b).unwrap();
        assert_eq!((ca.url.as_str(), ca.token.as_str()), ("http://127.0.0.1:4001", "token-a"));
        assert_eq!((cb.url.as_str(), cb.token.as_str()), ("http://127.0.0.1:4002", "token-b"));
        assert_eq!(state.current(&a).map(|c| c.url), Some(ca.url.clone()));
        assert_eq!(state.current(&b).map(|c| c.url), Some(cb.url.clone()));
        assert_eq!(state.open_paths().len(), 2);
        state.stop_all();
    }

    #[test]
    fn closing_one_project_stops_only_its_process() {
        let d = scratch("close");
        let (a, b) = (scratch("close-a"), scratch("close-b"));
        let state = Servers::with_timeout(Duration::from_secs(5));
        state.start_with(&fake_serving(&d, "a", 4011), &a).unwrap();
        let d2 = scratch("close-bin");
        state.start_with(&fake_serving(&d2, "b", 4012), &b).unwrap();
        let pid_a = std::fs::read_to_string(d.join("a-pid")).unwrap().trim().to_string();
        let pid_b = std::fs::read_to_string(d2.join("b-pid")).unwrap().trim().to_string();
        state.stop(&a);
        assert!(gone(&pid_a), "A's process is still running");
        assert!(alive(&pid_b), "B's process must not be affected");
        assert!(state.current(&a).is_none());
        assert!(state.current(&b).is_some());
        state.stop_all();
        assert!(gone(&pid_b), "quitting leaves no orphan");
    }

    #[test]
    fn quitting_stops_every_server_and_leaves_no_orphan() {
        let (d1, d2, d3) = (scratch("q1"), scratch("q2"), scratch("q3"));
        let state = Servers::with_timeout(Duration::from_secs(5));
        let mut pids = Vec::new();
        for (i, d) in [&d1, &d2, &d3].into_iter().enumerate() {
            let project = scratch(&format!("q-proj-{i}"));
            state.start_with(&fake_serving(d, "p", 4020 + i as u16), &project).unwrap();
            pids.push(std::fs::read_to_string(d.join("p-pid")).unwrap().trim().to_string());
        }
        assert!(pids.iter().all(|p| alive(p)));
        state.stop_all();
        assert!(pids.iter().all(|p| gone(p)), "a server is still running after stop_all");
        assert!(state.open_paths().is_empty());
        state.stop_all(); // twice is fine
    }

    #[test]
    fn starting_an_open_project_again_returns_the_same_server() {
        let d = scratch("again");
        let project = scratch("again-p");
        let state = Servers::with_timeout(Duration::from_secs(5));
        let first = state.start_with(&fake_serving(&d, "a", 4031), &project).unwrap();
        // `start` finds the live one and does not spawn another (no `orch` is needed for that)
        let second = state.start_using(&project, Some("/nonexistent/orch".into())).unwrap();
        assert_eq!(first.url, second.url);
        state.stop_all();
    }

    #[test]
    fn a_project_that_fails_to_start_does_not_affect_the_others() {
        let d = scratch("iso");
        let (good, bad) = (scratch("iso-good"), scratch("iso-bad"));
        let state = Servers::with_timeout(Duration::from_secs(5));
        state.start_with(&fake_serving(&d, "g", 4041), &good).unwrap();
        let d2 = scratch("iso-bin");
        let failing = fake_orch(&d2, "echo 'cannot listen: address in use' >&2\nexit 1");
        let e = state.start_with(&failing, &bad).unwrap_err();
        assert!(e.contains("address in use"), "{e}");
        assert!(state.current(&good).is_some(), "the healthy project keeps its server");
        assert!(state.current(&bad).is_none());
        state.stop_all();
    }

    #[test]
    fn a_dead_server_is_forgotten_and_a_restart_starts_a_new_one() {
        let d = scratch("dead");
        let project = scratch("dead-p");
        let state = Servers::with_timeout(Duration::from_secs(5));
        state.start_with(&fake_serving(&d, "a", 4051), &project).unwrap();
        let pid = std::fs::read_to_string(d.join("a-pid")).unwrap().trim().to_string();
        let _ = Command::new("kill").arg(&pid).status();
        // `current` notices the exit (and reaps the process)
        let forgotten = (0..60).any(|_| {
            std::thread::sleep(Duration::from_millis(50));
            state.current(&project).is_none()
        });
        assert!(forgotten, "an exited server is not reported as open");
        assert!(gone(&pid));
        state.stop_all();
    }

    #[test]
    fn a_symlinked_spelling_of_the_folder_is_the_same_project() {
        let d = scratch("symsrv");
        let real = scratch("symsrv-real");
        let link = scratch("symsrv-links").join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let state = Servers::with_timeout(Duration::from_secs(5));
        let c = state.start_with(&fake_serving(&d, "a", 4061), &link).unwrap();
        // found, stopped and listed under any spelling; starting again does not spawn another
        assert_eq!(state.current(&real).map(|x| x.url), Some(c.url.clone()));
        assert_eq!(state.current(&link).map(|x| x.url), Some(c.url.clone()));
        let with_slash = std::path::PathBuf::from(format!("{}/", link.display()));
        assert_eq!(state.start_using(&with_slash, Some("/nonexistent/orch".into())).unwrap().url, c.url);
        assert_eq!(state.open_paths().len(), 1);
        let pid = std::fs::read_to_string(d.join("a-pid")).unwrap().trim().to_string();
        state.stop(&real);
        assert!(gone(&pid));
        assert!(state.current(&link).is_none());
    }
}
