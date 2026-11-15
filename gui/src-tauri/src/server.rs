//! The `orch serve` child process: start it for a project folder, learn its address and token,
//! stop it. It has no knowledge of runs, milestones or agents.

use crate::serve::{
    allowed_origins, find_in_environment, parse_listening, parse_token_path, read_token,
};
use serde::Serialize;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
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

/// The server this window started, if any.
pub struct ServerState {
    running: Mutex<Option<Running>>,
    /// How long `orch serve` gets to print its address and token file.
    start_timeout: Duration,
}

impl Default for ServerState {
    fn default() -> Self {
        ServerState::with_timeout(Duration::from_secs(10))
    }
}

impl ServerState {
    pub fn with_timeout(start_timeout: Duration) -> ServerState {
        ServerState {
            running: Mutex::new(None),
            start_timeout,
        }
    }

    /// Stop the server, if one is running. Safe to call twice.
    pub fn stop(&self) {
        if let Ok(mut guard) = self.running.lock() {
            guard.take();
        }
    }

    /// The running server's address and token; forgets a server that has exited.
    pub fn current(&self) -> Option<Connection> {
        let mut guard = self.running.lock().ok()?;
        let running = guard.as_mut()?;
        if !matches!(running.child.try_wait(), Ok(None)) {
            guard.take();
            return None;
        }
        guard.as_ref().map(|r| r.connection.clone())
    }

    /// Start `orch serve` in `project` (stopping a previous one first). The `orch` binary is
    /// `ORCH_BIN`, else the first one on the PATH.
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
        self.stop();
        if !project.is_dir() {
            return Err(format!("{} is not a folder", project.display()));
        }
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

    fn start_with(&self, orch: &Path, project: &Path) -> Result<Connection, String> {
        let mut command = Command::new(orch);
        command.arg("serve").args(["--port", "0"]);
        for origin in allowed_origins() {
            command.args(["--allow-origin", origin]);
        }
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
            *guard = Some(Running {
                child,
                connection: connection.clone(),
            });
        }
        Ok(connection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A stand-in for `orch` that prints `lines` like `orch serve` and then waits.
    fn fake_orch(dir: &Path, script: &str) -> std::path::PathBuf {
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
        let state = ServerState::with_timeout(Duration::from_secs(5));
        let c = state.start_with(&orch, &project).unwrap();
        assert_eq!(
            (c.url.as_str(), c.token.as_str()),
            ("http://127.0.0.1:4242", "s3cret")
        );
        assert_eq!(
            state.current().map(|c| c.url),
            Some("http://127.0.0.1:4242".into())
        );
        let cwd = std::fs::read_to_string(d.join("cwd")).unwrap();
        assert_eq!(
            std::fs::canonicalize(cwd.trim()).unwrap(),
            std::fs::canonicalize(&project).unwrap()
        );
        state.stop();
        assert!(state.current().is_none());
        state.stop(); // twice is fine
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
        let state = ServerState::with_timeout(Duration::from_secs(5));
        state.start_with(&orch, &d).unwrap();
        let pid = std::fs::read_to_string(d.join("pid"))
            .unwrap()
            .trim()
            .to_string();
        state.stop();
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
        let state = ServerState::with_timeout(Duration::from_secs(5));
        let e = state.start_with(&orch, &d).unwrap_err();
        assert!(e.contains("address in use"), "{e}");
        assert!(state.current().is_none());
    }

    #[test]
    fn a_silent_server_times_out_and_is_killed() {
        let d = scratch("silent");
        let orch = fake_orch(&d, "exec sleep 30");
        let state = ServerState::with_timeout(Duration::from_millis(300));
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
        let state = ServerState::with_timeout(Duration::from_secs(5));
        let e = state.start_with(&orch, &d).unwrap_err();
        assert!(e.contains("cannot read the API token file"), "{e}");
        assert!(state.current().is_none());
    }

    #[test]
    fn refuses_a_folder_that_does_not_exist() {
        let state = ServerState::default();
        let e = state.start(Path::new("/nonexistent/project")).unwrap_err();
        assert!(e.contains("is not a folder"), "{e}");
    }
}
