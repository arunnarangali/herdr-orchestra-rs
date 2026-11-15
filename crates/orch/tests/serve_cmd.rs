//! `orch serve` as a real process: loopback only, the token file, real requests, clean stop.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

fn orch(dir: &Path, cfg: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_orch"));
    c.arg("serve")
        .args(args)
        .current_dir(dir)
        .env("XDG_CONFIG_HOME", cfg)
        .env_remove("ORCH_LOG")
        .stdin(Stdio::null());
    c
}

struct Running {
    child: Child,
    addr: String,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(dir: &Path, cfg: &Path) -> Running {
    let mut child = orch(dir, cfg, &["--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let url = line
        .trim()
        .strip_prefix("orch serve: listening on http://")
        .unwrap_or_else(|| panic!("unexpected first line: {line:?}"));
    Running {
        addr: url.to_string(),
        child,
        stdout,
    }
}

fn get(addr: &str, path: &str, token: Option<&str>) -> (u16, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n{auth}\r\n"
    )
    .unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).unwrap();
    let status = raw.split(' ').nth(1).unwrap().parse().unwrap();
    (
        status,
        raw.split_once("\r\n\r\n")
            .map(|x| x.1)
            .unwrap_or("")
            .to_string(),
    )
}

#[test]
fn refuses_a_non_loopback_host() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    for host in ["0.0.0.0", "192.168.0.10", "::"] {
        let o = orch(dir.path(), cfg.path(), &["--host", host])
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(2), "{host}");
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.contains("loopback"), "{err}");
        assert!(!cfg.path().join("herdr-orchestra/api-token").exists() || o.stdout.is_empty());
        assert!(o.stdout.is_empty(), "printed a URL for {host}");
    }
}

#[test]
fn rejects_a_bad_port_and_unknown_flags() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    assert_eq!(
        orch(dir.path(), cfg.path(), &["--port", "abc"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        orch(dir.path(), cfg.path(), &["--public"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn serves_health_with_the_token_creates_no_run_and_stops_on_sigterm() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let mut srv = start(dir.path(), cfg.path());
    assert!(srv.addr.starts_with("127.0.0.1:"), "{}", srv.addr);

    let token_file = cfg.path().join("herdr-orchestra/api-token");
    assert_eq!(
        std::fs::metadata(&token_file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let token = std::fs::read_to_string(&token_file)
        .unwrap()
        .trim()
        .to_string();

    let (status, body) = get(&srv.addr, "/api/v1/health", Some(&token));
    assert_eq!(status, 200, "{body}");
    assert!(body.contains(env!("CARGO_PKG_VERSION")));
    let (status, body) = get(&srv.addr, "/api/v1/health", None);
    assert_eq!(status, 401);
    assert!(body.contains("\"unauthorized\"") && !body.contains(&token));

    // Nothing here ever creates a run.
    assert!(!dir.path().join(".orchestrate").exists());

    // The token is never printed.
    let mut rest = String::new();
    // SIGTERM: in-flight requests finish, exit 0.
    let kill = Command::new("kill")
        .args(["-TERM", &srv.child.id().to_string()])
        .status()
        .unwrap();
    assert!(kill.success());
    let status = srv.child.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    srv.stdout.read_to_string(&mut rest).unwrap();
    assert!(!rest.contains(&token));
    assert!(rest.contains("stopped"));
}

#[test]
fn the_token_is_reused_on_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let token_file = cfg.path().join("herdr-orchestra/api-token");
    drop(start(dir.path(), cfg.path()));
    let first = std::fs::read_to_string(&token_file).unwrap();
    let second_srv = start(dir.path(), cfg.path());
    assert_eq!(std::fs::read_to_string(&token_file).unwrap(), first);
    assert_eq!(
        get(&second_srv.addr, "/api/v1/health", Some(first.trim())).0,
        200
    );
}

#[test]
fn sigint_closes_an_open_event_stream_and_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let mut srv = start(dir.path(), cfg.path());
    let token = std::fs::read_to_string(cfg.path().join("herdr-orchestra/api-token"))
        .unwrap()
        .trim()
        .to_string();
    let mut s = TcpStream::connect(&srv.addr).unwrap();
    write!(
        s,
        "GET /api/v1/stream HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {token}\r\n\r\n",
        srv.addr
    )
    .unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let mut head = [0u8; 12];
    s.read_exact(&mut head).unwrap();
    assert!(String::from_utf8_lossy(&head).contains("200"), "{head:?}");

    let kill = Command::new("kill")
        .args(["-INT", &srv.child.id().to_string()])
        .status()
        .unwrap();
    assert!(kill.success());
    // the stream ends (EOF) instead of holding the shutdown open
    let mut rest = Vec::new();
    let _ = s.read_to_end(&mut rest);
    assert_eq!(srv.child.wait().unwrap().code(), Some(0));
    let mut out = String::new();
    srv.stdout.read_to_string(&mut out).unwrap();
    assert!(out.contains("SIGINT received") && out.contains("stopped"), "{out}");
}
