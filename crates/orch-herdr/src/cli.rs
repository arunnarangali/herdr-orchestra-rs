//! The herdr CLI backend: [`CliHerdr`] implements
//! [`RuntimeAdapter`](crate::RuntimeAdapter) by running the real `herdr`
//! binary (§4), one subprocess per call, with typed parsing of its JSON.
//!
//! Every call is bounded by a timeout (`ORCH_HERDR_TIMEOUT_S`); a herdr that
//! hangs is killed and reaped, so no child process is left behind. The binary
//! comes from the `HERDR` tunable. The status of an agent is read only from
//! the `agent_status` JSON field, never by searching herdr's text, so a name
//! or path containing "working" or "blocked" is never misread.
//!
//! Commands and JSON shapes were checked against herdr 0.9.5 (`herdr --help`,
//! the subcommand helps and `herdr api schema --json`): replies are
//! `{"id": "cli:agent:list", "result": {...}}`, errors
//! `{"error": {"code", "message"}, "id": ...}` with a non-zero exit.

use crate::adapter::{Availability, RuntimeAdapter, SplitDirection, SplitPane};
use crate::error::{HerdrError, Result};
use crate::model::{AgentInfo, PaneInfo, WorkspaceInfo};
use crate::status::AgentStatus;
use orch_config::tunables::Tunables;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The prefix of an agent target that names a bare pane instead of a herdr
/// agent (`name (kind @pane:<id>)` in plan.md, §4).
pub const PANE_TARGET_PREFIX: &str = "@pane:";

/// What a finished herdr process left behind.
struct Finished {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// The runtime adapter that drives the `herdr` command line.
#[derive(Debug, Clone)]
pub struct CliHerdr {
    bin: String,
    timeout_s: u64,
    /// Environment changes for every child (`None` removes the variable),
    /// also consulted for `HERDR_ENV` before the process environment.
    env: Vec<(String, Option<String>)>,
}

impl CliHerdr {
    /// A backend running `bin` (a name on `PATH` or a path), every call bounded
    /// by `timeout_s` seconds (at least 1).
    pub fn new(bin: impl Into<String>, timeout_s: u64) -> Self {
        CliHerdr {
            bin: bin.into(),
            timeout_s: timeout_s.max(1),
            env: Vec::new(),
        }
    }

    /// A backend from the tunables: the `HERDR` binary and `ORCH_HERDR_TIMEOUT_S`.
    pub fn from_tunables(t: &Tunables) -> Self {
        CliHerdr::new(t.herdr.clone(), t.herdr_timeout_s.max(1) as u64)
    }

    /// Set an environment variable for every herdr child (and for the
    /// `HERDR_ENV` check in [`availability`](RuntimeAdapter::availability)).
    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_string(), Some(value.to_string())));
        self
    }

    /// Hide the developer's own `HERDR_*` variables from every herdr child and
    /// from the `HERDR_ENV` check, so a test gives the same result inside and
    /// outside a real herdr pane. Variables set afterwards with
    /// [`with_env`](CliHerdr::with_env) still apply.
    pub fn without_inherited_herdr_env(mut self) -> Self {
        let mut hidden: Vec<(String, Option<String>)> = std::env::vars()
            .filter(|(k, _)| k.starts_with("HERDR_"))
            .map(|(k, _)| (k, None))
            .collect();
        hidden.append(&mut self.env);
        self.env = hidden;
        self
    }

    /// The configured call timeout in seconds.
    pub fn timeout_s(&self) -> u64 {
        self.timeout_s
    }

    fn env_var(&self, key: &str) -> Option<String> {
        match self.env.iter().rev().find(|(k, _)| k == key) {
            Some((_, v)) => v.clone(),
            None => std::env::var(key).ok(),
        }
    }

    /// Run `herdr <args>` to completion or to the timeout.
    fn run(&self, args: &[&str]) -> Result<Finished> {
        let label = format!(
            "herdr {}",
            args.iter().take(2).copied().collect::<Vec<_>>().join(" ")
        );
        let mut cmd = Command::new(&self.bin);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &self.env {
            match v {
                Some(v) => cmd.env(k, v),
                None => cmd.env_remove(k),
            };
        }
        let mut child = cmd.spawn().map_err(|e| HerdrError::Unavailable {
            message: format!("cannot run {:?}: {e}", self.bin),
        })?;
        let out = drain(child.stdout.take());
        let err = drain(child.stderr.take());
        let deadline = Instant::now() + Duration::from_secs(self.timeout_s);
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out.join();
                    let _ = err.join();
                    return Err(HerdrError::timed_out(
                        self.timeout_s,
                        format!("{label} timed out"),
                    ));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(HerdrError::other(format!("{label} failed: {e}")));
                }
            }
        };
        Ok(Finished {
            code: status.code(),
            stdout: out.join().unwrap_or_default(),
            stderr: err.join().unwrap_or_default(),
        })
    }

    /// Run herdr and return stdout, mapping a non-zero exit or a JSON error
    /// reply to the matching [`HerdrError`]. `target` is the agent or pane the
    /// call was about, kept in the error.
    fn call(&self, args: &[&str], target: &str) -> Result<String> {
        let label = format!(
            "herdr {}",
            args.iter().take(2).copied().collect::<Vec<_>>().join(" ")
        );
        let f = self.run(args)?;
        if f.code != Some(0) {
            return Err(failure(&label, &f, target));
        }
        if let Some(e) = error_reply(&f.stdout) {
            return Err(e.into_error(target));
        }
        Ok(f.stdout)
    }

    /// Run herdr and return the `result` object of its JSON reply.
    fn call_json(&self, args: &[&str], target: &str) -> Result<Value> {
        let stdout = self.call(args, target)?;
        let mut v: Value = serde_json::from_str(stdout.trim()).map_err(|e| {
            HerdrError::other(format!(
                "herdr {}: reply is not JSON ({e}): {}",
                args.join(" "),
                stdout.trim()
            ))
        })?;
        Ok(v.get_mut("result").map(Value::take).unwrap_or(Value::Null))
    }

    /// `result.<key>` of a reply, deserialised.
    fn field<T: DeserializeOwned>(&self, args: &[&str], target: &str, key: &str) -> Result<T> {
        let mut result = self.call_json(args, target)?;
        let v = result.get_mut(key).map(Value::take).ok_or_else(|| {
            HerdrError::other(format!(
                "herdr {}: reply has no result.{key}",
                args.join(" ")
            ))
        })?;
        serde_json::from_value(v).map_err(|e| {
            HerdrError::other(format!("herdr {}: bad result.{key}: {e}", args.join(" ")))
        })
    }
}

/// Read a child's pipe on its own thread so a chatty herdr cannot fill it up.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut p) = pipe {
            let mut buf = Vec::new();
            let _ = p.read_to_end(&mut buf);
            s = String::from_utf8_lossy(&buf).into_owned();
        }
        s
    })
}

/// herdr's `{"error": {"code", "message"}}`.
struct ErrorReply {
    code: String,
    message: String,
}

impl ErrorReply {
    fn into_error(self, target: &str) -> HerdrError {
        match self.code.as_str() {
            "agent_not_found" => HerdrError::AgentNotFound {
                agent: target.to_string(),
                message: self.message,
            },
            "agent_blocked" => HerdrError::AgentBlocked {
                agent: target.to_string(),
                message: self.message,
            },
            "pane_not_found" => HerdrError::StalePane {
                pane: target.to_string(),
                message: self.message,
            },
            _ => HerdrError::other(format!("{}: {}", self.code, self.message)),
        }
    }
}

fn error_reply(text: &str) -> Option<ErrorReply> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let e = v.get("error")?;
    Some(ErrorReply {
        code: e
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("error")
            .to_string(),
        message: e
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

/// The error for a herdr that exited non-zero: its JSON error when it printed
/// one (stdout or stderr), else whatever text it left.
fn failure(label: &str, f: &Finished, target: &str) -> HerdrError {
    if let Some(e) = error_reply(&f.stdout).or_else(|| error_reply(&f.stderr)) {
        return e.into_error(target);
    }
    let text = format!("{}{}", f.stdout, f.stderr);
    let text = text.trim();
    let code = f
        .code
        .map_or("a signal".to_string(), |c| format!("exit {c}"));
    HerdrError::other(if text.is_empty() {
        format!("{label} failed ({code})")
    } else {
        format!("{label} failed ({code}): {text}")
    })
}

/// An agent as herdr reports it; `name` is absent for an unnamed agent, which
/// is then addressed by its pane id.
#[derive(serde::Deserialize)]
struct RawAgent {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "agent")]
    kind: String,
    pane_id: String,
    tab_id: String,
    workspace_id: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(rename = "agent_status")]
    status: AgentStatus,
    #[serde(default)]
    focused: bool,
}

impl From<RawAgent> for AgentInfo {
    fn from(a: RawAgent) -> Self {
        AgentInfo {
            name: a.name.unwrap_or_else(|| a.pane_id.clone()),
            kind: a.kind,
            pane_id: a.pane_id,
            tab_id: a.tab_id,
            workspace_id: a.workspace_id,
            cwd: a.cwd,
            status: a.status,
            focused: a.focused,
        }
    }
}

impl RuntimeAdapter for CliHerdr {
    fn availability(&self) -> Result<Availability> {
        let in_pane = self.env_var("HERDR_ENV").as_deref() == Some("1");
        let reachable = match self.call(&["status"], "") {
            Ok(_) => true,
            Err(e @ HerdrError::TimedOut { .. }) => return Err(e),
            Err(_) => false,
        };
        Ok(Availability { in_pane, reachable })
    }

    fn current_pane(&self) -> Result<PaneInfo> {
        self.field(&["pane", "current", "--current"], "(current pane)", "pane")
            .map_err(|e| match e {
                HerdrError::Other { message } => HerdrError::StalePane {
                    pane: "(current pane)".into(),
                    message,
                },
                e => e,
            })
    }

    fn agents(&self) -> Result<Vec<AgentInfo>> {
        let raw: Vec<RawAgent> = self.field(&["agent", "list"], "", "agents")?;
        Ok(raw.into_iter().map(AgentInfo::from).collect())
    }

    fn agent_kinds(&self) -> Result<Vec<String>> {
        // Plain `herdr agent` prints its usage, ending in a `kinds: a|b|c` line
        // (herdr 0.9.5 prints it on stderr and exits 2), so look at both streams
        // and ignore the exit code.
        let f = self.run(&["agent"])?;
        let text = format!("{}\n{}", f.stdout, f.stderr);
        text.lines()
            .find_map(|l| l.trim().strip_prefix("kinds:"))
            .map(|k| {
                k.split('|')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .ok_or_else(|| HerdrError::other("herdr agent: no `kinds:` line in its usage text"))
    }

    fn agent(&self, target: &str) -> Result<AgentInfo> {
        let raw: RawAgent = self.field(&["agent", "get", target], target, "agent")?;
        Ok(raw.into())
    }

    fn send_prompt(&self, agent: &str, text: &str) -> Result<()> {
        if let Some(pane) = agent.strip_prefix(PANE_TARGET_PREFIX) {
            // herdr cannot recognise this agent: type into its pane, then Enter.
            return self
                .send_text(pane, text)
                .and_then(|()| self.send_keys(pane, &["enter"]))
                .map_err(|e| stale_pane(e, agent, pane));
        }
        self.call(&["agent", "prompt", agent, text], agent)
            .map(|_| ())
    }

    fn read_output(&self, agent: &str, lines: u32) -> Result<String> {
        let n = lines.to_string();
        self.call(
            &[
                "agent",
                "read",
                agent,
                "--source",
                "recent-unwrapped",
                "--lines",
                &n,
            ],
            agent,
        )
    }

    fn rename_agent(&self, target: &str, new_name: &str) -> Result<()> {
        self.call(&["agent", "rename", target, new_name], target)
            .map(|_| ())
    }

    fn rename_pane(&self, pane: &str, label: &str) -> Result<()> {
        self.call(&["pane", "rename", pane, label], pane)
            .map(|_| ())
    }

    fn pane(&self, pane: &str) -> Result<PaneInfo> {
        self.field(&["pane", "get", pane], pane, "pane")
    }

    fn panes(&self, workspace: Option<&str>) -> Result<Vec<PaneInfo>> {
        match workspace {
            Some(w) => self.field(&["pane", "list", "--workspace", w], w, "panes"),
            None => self.field(&["pane", "list"], "", "panes"),
        }
    }

    fn workspaces(&self) -> Result<Vec<WorkspaceInfo>> {
        self.field(&["workspace", "list"], "", "workspaces")
    }

    fn split_pane(&self, req: &SplitPane) -> Result<PaneInfo> {
        let mut args: Vec<String> = vec!["pane".into(), "split".into()];
        if let Some(p) = &req.from_pane {
            args.push(p.clone());
        }
        args.push("--direction".into());
        args.push(match req.direction {
            SplitDirection::Right => "right".into(),
            SplitDirection::Down => "down".into(),
        });
        if let Some(r) = req.ratio {
            args.push("--ratio".into());
            args.push(r.to_string());
        }
        if let Some(c) = &req.cwd {
            args.push("--cwd".into());
            args.push(c.to_string_lossy().into_owned());
        }
        args.push(if req.focus { "--focus" } else { "--no-focus" }.into());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.field(
            &refs,
            req.from_pane.as_deref().unwrap_or("(current pane)"),
            "pane",
        )
    }

    fn run_command(&self, pane: &str, command: &str) -> Result<()> {
        self.call(&["pane", "run", pane, command], pane).map(|_| ())
    }

    fn send_text(&self, pane: &str, text: &str) -> Result<()> {
        self.call(&["pane", "send-text", pane, text], pane)
            .map(|_| ())
    }

    fn send_keys(&self, pane: &str, keys: &[&str]) -> Result<()> {
        let mut args = vec!["pane", "send-keys", pane];
        args.extend_from_slice(keys);
        self.call(&args, pane).map(|_| ())
    }
}

/// A failed text/Enter delivery to an `@pane:<id>` agent is a stale pane, with
/// the pane id in the message (Python's `deliver`). A timeout or a missing
/// herdr keep their own kind: they say nothing about the pane.
fn stale_pane(e: HerdrError, agent: &str, pane: &str) -> HerdrError {
    match e {
        HerdrError::TimedOut { .. } | HerdrError::Unavailable { .. } => e,
        e => HerdrError::StalePane {
            pane: pane.to_string(),
            message: format!(
                "{e} — pane {pane} for {agent} (stale pane id? re-check with `herdr pane list` and fix Workers/Reviewer in plan.md)"
            ),
        },
    }
}
