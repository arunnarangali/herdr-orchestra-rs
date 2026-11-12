//! The replaceable runtime adapter (§4): the one trait every later phase talks
//! to agents through. Milestone M2 implements it for the herdr CLI; the
//! in-memory [`FakeHerdr`](crate::fake::FakeHerdr) implements it for tests; any
//! future backend can too.
//!
//! The trait is synchronous and needs no async runtime (§25 rule 21). It is
//! object safe, so a backend can travel in a `Box<dyn RuntimeAdapter>` or a
//! `Arc<dyn RuntimeAdapter>`.
//!
//! Runtime events are deliberately **not** in the trait yet: in this phase
//! status comes from [`agent`](RuntimeAdapter::agent) / [`agent_status`]
//! (`herdr agent get`), and the socket subscription (`events.subscribe`,
//! `pane_agent_status_changed`, protocol 22) arrives in Phase 6 with the event
//! manager.
//!
//! [`agent_status`]: RuntimeAdapter::agent_status

use crate::error::{HerdrError, Result};
use crate::model::{AgentInfo, PaneInfo, WorkspaceInfo};
use crate::status::AgentStatus;

/// Whether orch runs inside a herdr-managed pane and whether herdr works at
/// all (§4, `orch lead check`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Availability {
    /// Are we inside a herdr pane (`HERDR_ENV=1`)? When false, orch cannot use
    /// `herdr pane current` or split from the calling pane.
    pub in_pane: bool,
    /// Is herdr installed and its server reachable (a plain `herdr status`
    /// succeeds)? When false, no other call will work.
    pub reachable: bool,
}

/// Which way `herdr pane split` cuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    /// `--direction right`.
    Right,
    /// `--direction down`.
    Down,
}

/// The request for [`RuntimeAdapter::split_pane`] (`herdr pane split`).
#[derive(Debug, Clone, PartialEq)]
pub struct SplitPane {
    /// The pane to split; `None` means the calling pane when in one, else the
    /// focused pane (herdr's own default).
    pub from_pane: Option<String>,
    /// Which way to cut.
    pub direction: SplitDirection,
    /// Share of the new pane, `--ratio FLOAT` (herdr's default when `None`).
    pub ratio: Option<f64>,
    /// The folder the new pane starts in, `--cwd PATH`.
    pub cwd: Option<std::path::PathBuf>,
    /// Whether the new pane gets focus (`--focus` / `--no-focus`).
    pub focus: bool,
}

impl SplitPane {
    /// Split `from_pane` right, in `cwd`, without moving focus.
    pub fn right(from_pane: &str, cwd: &std::path::Path) -> Self {
        SplitPane {
            from_pane: Some(from_pane.to_string()),
            direction: SplitDirection::Right,
            ratio: None,
            cwd: Some(cwd.to_path_buf()),
            focus: false,
        }
    }
}

/// The herdr runtime adapter contract. Every method is synchronous and maps to
/// one documented herdr CLI call (named in its doc comment); herdr's JSON
/// replies are already typed in [`crate::model`].
///
/// Error contract: agent-targeted methods can fail with
/// [`HerdrError::AgentNotFound`], pane-targeted methods with
/// [`HerdrError::StalePane`], [`send_prompt`](RuntimeAdapter::send_prompt) with
/// [`HerdrError::AgentBlocked`]; every method can fail with
/// [`HerdrError::TimedOut`], [`HerdrError::Unavailable`] and
/// [`HerdrError::Other`].
pub trait RuntimeAdapter {
    /// Runtime availability (§4): are we inside a herdr pane, and is herdr
    /// installed and reachable? Corresponds to checking `HERDR_ENV=1` and a
    /// plain `herdr status`.
    fn availability(&self) -> Result<Availability>;

    /// The pane the calling process is in. Corresponds to
    /// `herdr pane current --current` (the fallback when `HERDR_ENV` does not
    /// reach the agent's shell, e.g. Hermes).
    ///
    /// Fails with [`HerdrError::StalePane`] when there is no current pane.
    fn current_pane(&self) -> Result<PaneInfo>;

    /// All agents herdr recognises. Corresponds to `herdr agent list`.
    fn agents(&self) -> Result<Vec<AgentInfo>>;

    /// One agent by name or pane id. Corresponds to `herdr agent get <target>`.
    ///
    /// Fails with [`HerdrError::AgentNotFound`] when herdr has no such agent;
    /// status-only callers should prefer [`agent_status`](RuntimeAdapter::agent_status),
    /// which turns that into [`AgentStatus::Gone`].
    fn agent(&self, target: &str) -> Result<AgentInfo>;

    /// An agent's status. Corresponds to `herdr agent get <target>` (the
    /// `agent_status` field).
    ///
    /// An agent herdr no longer knows reports [`AgentStatus::Gone`], not an
    /// error; an unwatchable agent (kind with `herdr_detect = false`) is never
    /// reported gone, blocked or idle (§4).
    fn agent_status(&self, target: &str) -> Result<AgentStatus> {
        match self.agent(target) {
            Ok(a) => Ok(a.status),
            Err(HerdrError::AgentNotFound { .. }) => Ok(AgentStatus::Gone),
            Err(e) => Err(e),
        }
    }

    /// Send a prompt to a named agent. Corresponds to
    /// `herdr agent prompt <target> <text>`.
    ///
    /// Fails with [`HerdrError::AgentBlocked`] when the agent is blocked and
    /// the prompt is refused, [`HerdrError::AgentNotFound`] when the agent is
    /// gone. For agents herdr cannot recognise (`@pane:<id>`, kind with
    /// `herdr_detect = false`) use [`send_text`](RuntimeAdapter::send_text)
    /// plus `send_keys(.., ["enter"])` on the agent's pane instead (§4).
    fn send_prompt(&self, agent: &str, text: &str) -> Result<()>;

    /// Read an agent's recent terminal output. Corresponds to
    /// `herdr agent read <target> --source recent-unwrapped --lines N`.
    fn read_output(&self, agent: &str, lines: u32) -> Result<String>;

    /// Name an agent. Corresponds to `herdr agent rename <target> <name>`.
    fn rename_agent(&self, target: &str, new_name: &str) -> Result<()>;

    /// Label a pane. Corresponds to `herdr pane rename <pane_id> <label>`.
    ///
    /// Fails with [`HerdrError::StalePane`] when the pane id is stale.
    fn rename_pane(&self, pane: &str, label: &str) -> Result<()>;

    /// One pane's details. Corresponds to `herdr pane get <pane_id>`.
    ///
    /// Fails with [`HerdrError::StalePane`] when the pane id is stale.
    fn pane(&self, pane: &str) -> Result<PaneInfo>;

    /// All panes, optionally of one workspace. Corresponds to
    /// `herdr pane list [--workspace <workspace_id>]`.
    fn panes(&self, workspace: Option<&str>) -> Result<Vec<PaneInfo>>;

    /// All workspaces. Corresponds to `herdr workspace list`.
    fn workspaces(&self) -> Result<Vec<WorkspaceInfo>>;

    /// Split a pane, in a given folder, to open a new agent pane (`orch team
    /// up`). Corresponds to `herdr pane split [<pane_id>] --direction
    /// right|down [--ratio FLOAT] [--cwd PATH] [--focus]`. Returns the new
    /// pane (herdr's `.result.pane`).
    ///
    /// Fails with [`HerdrError::StalePane`] when the source pane id is stale.
    fn split_pane(&self, req: &SplitPane) -> Result<PaneInfo>;

    /// Run a command in a pane (start an agent with the registry's launch
    /// command). Corresponds to `herdr pane run <pane_id> <command>`.
    fn run_command(&self, pane: &str, command: &str) -> Result<()>;

    /// Type text into a pane. Corresponds to
    /// `herdr pane send-text <pane_id> <text>`.
    fn send_text(&self, pane: &str, text: &str) -> Result<()>;

    /// Press keys in a pane (e.g. `enter` after
    /// [`send_text`](RuntimeAdapter::send_text) for `@pane:<id>` agents, §4).
    /// Corresponds to `herdr pane send-keys <pane_id> <key> [key ...]`.
    fn send_keys(&self, pane: &str, keys: &[&str]) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{CallKind, FakeHerdr};

    // §4: the adapter is replaceable — any backend fits behind a boxed trait
    // object, so callers need to know nothing about herdr.
    #[test]
    fn the_trait_is_object_safe_and_serves_a_boxed_backend() {
        let fake = FakeHerdr::new();
        fake.add_agent(crate::model::AgentInfo::new("claude-1", "claude", "w1:p1", AgentStatus::Idle));
        let backend: Box<dyn RuntimeAdapter> = Box::new(fake);
        assert_eq!(backend.agent_status("claude-1").unwrap(), AgentStatus::Idle);
        assert!(backend.agent_status("nobody").unwrap() == AgentStatus::Gone);
    }

    // The default agent_status turns agent_not_found into Gone.
    #[test]
    fn agent_status_reports_gone_for_a_missing_agent() {
        let fake = FakeHerdr::new();
        fake.fail_next(CallKind::AgentGet, 1, HerdrError::AgentNotFound { agent: "x".into(), message: "gone".into() });
        assert_eq!(fake.agent_status("x").unwrap(), AgentStatus::Gone);
    }

    // An unwatchable agent keeps its status; §4 forbids gone/blocked/idle.
    #[test]
    fn agent_status_never_reports_an_unwatchable_agent_as_gone() {
        let fake = FakeHerdr::new();
        fake.add_agent(crate::model::AgentInfo::new(
            "hermes-1",
            "hermes",
            "w1:p2",
            AgentStatus::Unwatchable,
        ));
        assert_eq!(fake.agent_status("hermes-1").unwrap(), AgentStatus::Unwatchable);
    }
}
