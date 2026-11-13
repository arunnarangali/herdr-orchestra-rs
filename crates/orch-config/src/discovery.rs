//! Agent discovery: for one registered kind, is it installed, which version, are its
//! launch flags still in the tool's help, is the account logged in. Every external call
//! runs with a timeout; nothing here reads or prints credential contents. Herdr
//! detection and the printing live in the `orch` crate (herdr only via the adapter).

pub mod skills;

use crate::agents::AgentKind;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Default timeout of one external call, seconds (Python's `_probe`).
pub const PROBE_TIMEOUT_S: u64 = 15;

/// The timeout for external calls: `ORCH_PROBE_TIMEOUT_S` or [`PROBE_TIMEOUT_S`].
pub fn probe_timeout() -> Duration {
    let s = std::env::var("ORCH_PROBE_TIMEOUT_S")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(PROBE_TIMEOUT_S);
    Duration::from_secs(s)
}

/// What a probe got: the exit code and stdout+stderr, or why it did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    Ran { code: Option<i32>, text: String },
    Failed(String),
}

impl Probe {
    /// The output when it ran and exited 0.
    pub fn ok_text(&self) -> Option<&str> {
        match self {
            Probe::Ran {
                code: Some(0),
                text,
            } => Some(text),
            _ => None,
        }
    }
}

/// Split a command line into words with shell quoting (Python's `shlex.split`).
pub fn split_words(s: &str) -> Result<Vec<String>, String> {
    let (mut out, mut cur, mut have) = (Vec::new(), String::new(), false);
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if have {
                    out.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            '\'' => {
                have = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => cur.push(c),
                        None => return Err("no closing quotation".into()),
                    }
                }
            }
            '"' => {
                have = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => cur.push(c),
                            Some(c) => {
                                cur.push('\\');
                                cur.push(c);
                            }
                            None => return Err("no closing quotation".into()),
                        },
                        Some(c) => cur.push(c),
                        None => return Err("no closing quotation".into()),
                    }
                }
            }
            '\\' => {
                have = true;
                match chars.next() {
                    Some(c) => cur.push(c),
                    None => return Err("no escaped character".into()),
                }
            }
            c => {
                have = true;
                cur.push(c);
            }
        }
    }
    if have {
        out.push(cur);
    }
    Ok(out)
}

fn drain<R: Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut r) = r {
            let _ = r.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    })
}

/// Run a command line (words split like a shell, no shell involved) with no stdin, to
/// completion or to `timeout`, then kill it.
pub fn probe(cmd: &str, timeout: Duration) -> Probe {
    let words = match split_words(cmd) {
        Ok(w) if !w.is_empty() => w,
        Ok(_) => return Probe::Failed("empty command".into()),
        Err(e) => return Probe::Failed(e),
    };
    let mut child = match Command::new(&words[0])
        .args(&words[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return Probe::Failed(e.to_string()),
    };
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let text = out.join().unwrap_or_default() + &err.join().unwrap_or_default();
                return Probe::Ran {
                    code: status.code(),
                    text,
                };
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out.join();
                let _ = err.join();
                return Probe::Failed(format!("timed out after {}s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Probe::Failed(e.to_string());
            }
        }
    }
}

/// The version command of a kind (`version_cmd`, default `<binary> --version`).
pub fn version_cmd(ak: &AgentKind) -> String {
    ak.version_cmd
        .clone()
        .unwrap_or_else(|| format!("{} --version", ak.binary))
}

/// The help command of a kind (`help_cmd`, default `<binary> --help`).
pub fn help_cmd(ak: &AgentKind) -> String {
    ak.help_cmd
        .clone()
        .unwrap_or_else(|| format!("{} --help", ak.binary))
}

/// The first non-blank line of the version command's output, or `Err(command)` when it
/// failed or printed nothing.
pub fn version(ak: &AgentKind, timeout: Duration) -> Result<String, String> {
    let cmd = version_cmd(ak);
    match probe(&cmd, timeout) {
        Probe::Ran {
            code: Some(0),
            text,
        } => text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_string)
            .ok_or(cmd),
        _ => Err(cmd),
    }
}

/// How one launch flag fared against the help output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagCheck {
    pub flag: String,
    pub result: FlagResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlagResult {
    Listed,
    /// The flag is not in the help text.
    Missing,
    /// The help command did not run (why).
    HelpFailed(String),
}

/// Whether `flag` appears in `help` as a whole option (not inside a longer one).
fn mentions(help: &str, flag: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    help.match_indices(flag).any(|(i, _)| {
        let before = help[..i].chars().next_back();
        let after = help[i + flag.len()..].chars().next();
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

/// Every option of the launch command (`--flag` or `--flag=value`, after the binary),
/// checked against the help output (Python's `doctor_kind`).
pub fn launch_flags(ak: &AgentKind, timeout: Duration) -> Vec<FlagCheck> {
    let launch = split_words(if ak.launch.is_empty() {
        &ak.binary
    } else {
        &ak.launch
    })
    .unwrap_or_else(|_| vec![ak.binary.clone()]);
    let flags: Vec<String> = launch
        .iter()
        .skip(1)
        .filter(|t| t.starts_with('-'))
        .map(|t| t.split('=').next().unwrap_or(t).to_string())
        .collect();
    if flags.is_empty() {
        return Vec::new();
    }
    let help = probe(&help_cmd(ak), timeout);
    flags
        .into_iter()
        .map(|flag| {
            let result = match &help {
                Probe::Failed(why) => FlagResult::HelpFailed(why.chars().take(80).collect()),
                Probe::Ran { text, .. } if mentions(text, &flag) => FlagResult::Listed,
                Probe::Ran { .. } => FlagResult::Missing,
            };
            FlagCheck { flag, result }
        })
        .collect()
}

/// Whether the account is logged in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthState {
    Authenticated,
    NotAuthenticated,
    Unknown,
}

impl AuthState {
    pub fn label(self) -> &'static str {
        match self {
            AuthState::Authenticated => "authenticated",
            AuthState::NotAuthenticated => "not authenticated",
            AuthState::Unknown => "unknown",
        }
    }
}

/// Authentication status with a short reason. Never carries credential contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Auth {
    pub state: AuthState,
    pub reason: String,
}

/// Ask the tool whether it is logged in, in the way the registry configures:
/// `auth_cmd` (exit 0 = logged in; its output is dropped unread) or else `auth_file`
/// (present = logged in). Neither set, or the command cannot run: unknown.
pub fn auth(ak: &AgentKind, home: &Path, timeout: Duration) -> Auth {
    let (state, reason) = if let Some(cmd) = ak.auth_cmd.as_deref().filter(|c| !c.is_empty()) {
        match probe(cmd, timeout) {
            Probe::Ran { code: Some(0), .. } => {
                (AuthState::Authenticated, format!("`{cmd}` exited 0"))
            }
            Probe::Ran { code, .. } => (
                AuthState::NotAuthenticated,
                match code {
                    Some(c) => format!("`{cmd}` exited {c}"),
                    None => format!("`{cmd}` was stopped by a signal"),
                },
            ),
            Probe::Failed(why) => (AuthState::Unknown, format!("`{cmd}` did not run ({why})")),
        }
    } else if let Some(f) = ak.auth_file.as_deref().filter(|f| !f.is_empty()) {
        let path = skills::expand(f, home);
        let shown = skills::short(&path, home);
        if path.exists() {
            (
                AuthState::Authenticated,
                format!("login state {shown} exists"),
            )
        } else {
            (
                AuthState::NotAuthenticated,
                format!("login state {shown} not found"),
            )
        }
    } else {
        (
            AuthState::Unknown,
            "no status command configured for this kind (auth_cmd / auth_file)".to_string(),
        )
    };
    Auth { state, reason }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn kind(extra: &str) -> AgentKind {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("agents.toml");
        std::fs::write(
            &p,
            format!(
                "[acme]\nbinary = \"acme\"\nlaunch = \"acme --go\"\nroles = [\"coder\"]\n{extra}"
            ),
        )
        .unwrap();
        crate::agents::registry(crate::agents::DEFAULT_TOML, &p)
            .unwrap()
            .0
            .remove("acme")
            .unwrap()
    }

    /// Write an executable `#!/bin/sh` script and return only once it can be exec'd.
    ///
    /// Another test thread may fork while the write handle is open; that child holds the
    /// handle until it execs, and exec'ing the script meanwhile fails with ETXTBSY. A child
    /// forked after our handle is closed cannot hold it, so one successful probe exec (the
    /// script exits at once when `ORCH_TEST_PROBE` is set) proves the file is safe for good.
    fn write_executable(p: &Path, body: &str) {
        let text = format!("#!/bin/sh\n[ -n \"$ORCH_TEST_PROBE\" ] && exit 0\n{body}\n");
        std::fs::write(p, text).unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match std::process::Command::new(p)
                .env("ORCH_TEST_PROBE", "1")
                .output()
            {
                Ok(_) => return,
                Err(e) if e.raw_os_error() == Some(26) && std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(e) => panic!("cannot exec {}: {e}", p.display()),
            }
        }
    }

    fn script(dir: &Path, name: &str, body: &str) -> String {
        let p = dir.join(name);
        write_executable(&p, body);
        p.display().to_string()
    }

    const T: Duration = Duration::from_secs(5);

    #[test]
    fn split_words_handles_quotes() {
        assert_eq!(
            split_words("a 'b c' \"d\\\"e\" f\\ g").unwrap(),
            ["a", "b c", "d\"e", "f g"]
        );
        assert!(split_words("a 'b").is_err());
        assert_eq!(split_words("a ''").unwrap(), ["a", ""]);
    }

    #[test]
    fn probe_reports_exit_output_and_missing_binaries() {
        let t = tempfile::tempdir().unwrap();
        let ok = script(t.path(), "ok", "echo hi; echo err >&2; exit 3");
        assert_eq!(
            probe(&ok, T),
            Probe::Ran {
                code: Some(3),
                text: "hi\nerr\n".into()
            }
        );
        assert!(matches!(probe("/nonexistent/bin", T), Probe::Failed(_)));
        assert!(matches!(probe("", T), Probe::Failed(_)));
    }

    #[test]
    fn probe_kills_a_command_that_outlives_the_timeout() {
        let t = tempfile::tempdir().unwrap();
        let slow = script(t.path(), "slow", "while :; do :; done");
        let started = Instant::now();
        let p = probe(&slow, Duration::from_millis(300));
        assert!(
            matches!(&p, Probe::Failed(w) if w.contains("timed out")),
            "{p:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn version_is_the_first_line_or_the_failed_command() {
        let t = tempfile::tempdir().unwrap();
        let v = script(t.path(), "v", "echo; echo 'acme 1.2.3'; echo more");
        let ak = kind(&format!("version_cmd = \"{v}\"\n"));
        assert_eq!(version(&ak, T), Ok("acme 1.2.3".into()));
        let bad = script(t.path(), "bad", "echo boom; exit 1");
        let ak = kind(&format!("version_cmd = \"{bad}\"\n"));
        assert_eq!(version(&ak, T), Err(bad));
    }

    #[test]
    fn launch_flags_are_looked_up_in_the_help() {
        let t = tempfile::tempdir().unwrap();
        let h = script(
            t.path(),
            "h",
            "echo '  --go-faster  do it'; echo '  --other'",
        );
        let ak = kind(&format!("help_cmd = \"{h}\"\n"));
        let r = launch_flags(&ak, T);
        assert_eq!(
            r,
            [FlagCheck {
                flag: "--go".into(),
                result: FlagResult::Missing
            }],
            "--go must not match --go-faster"
        );
        let h = script(t.path(), "h2", "echo 'options: --go, --x'");
        let ak = kind(&format!("help_cmd = \"{h}\"\n"));
        assert_eq!(launch_flags(&ak, T)[0].result, FlagResult::Listed);
        let ak = kind("help_cmd = \"/nonexistent/help\"\n");
        assert!(matches!(
            launch_flags(&ak, T)[0].result,
            FlagResult::HelpFailed(_)
        ));
    }

    #[test]
    fn auth_outcomes() {
        let t = tempfile::tempdir().unwrap();
        let yes = script(t.path(), "yes", "echo secret-token; exit 0");
        let no = script(t.path(), "no", "exit 1");
        let a = auth(&kind(&format!("auth_cmd = \"{yes}\"\n")), t.path(), T);
        assert_eq!(a.state, AuthState::Authenticated);
        assert!(
            !a.reason.contains("secret-token"),
            "command output is never kept"
        );
        let a = auth(&kind(&format!("auth_cmd = \"{no}\"\n")), t.path(), T);
        assert_eq!(a.state, AuthState::NotAuthenticated);
        let a = auth(&kind("auth_cmd = \"/nonexistent/status\"\n"), t.path(), T);
        assert_eq!(a.state, AuthState::Unknown);
        let ak = kind("auth_file = \"~/.acme/login\"\n");
        assert_eq!(auth(&ak, t.path(), T).state, AuthState::NotAuthenticated);
        std::fs::create_dir_all(t.path().join(".acme")).unwrap();
        std::fs::write(t.path().join(".acme/login"), "TOKEN").unwrap();
        let a = auth(&ak, t.path(), T);
        assert_eq!(a.state, AuthState::Authenticated);
        assert!(a.reason.contains("~/.acme/login") && !a.reason.contains("TOKEN"));
        assert_eq!(auth(&kind(""), t.path(), T).state, AuthState::Unknown);
    }
}
