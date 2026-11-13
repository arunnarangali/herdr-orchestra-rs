//! The in-memory fake backend: a scripted [`RuntimeAdapter`] so the whole test
//! suite runs without a real Herdr (§4, the Rust twin of Python's
//! `tests/sim/herdr`). Tests add and remove agents and panes, set statuses,
//! inject failures for the next N calls of a kind, and read back everything
//! that was sent.

use crate::adapter::{Availability, RuntimeAdapter, SplitPane};
use crate::error::{HerdrError, Result};
use crate::model::{AgentInfo, PaneInfo, WorkspaceInfo};
use crate::status::AgentStatus;
use std::collections::BTreeMap;
use std::sync::Mutex;

/// Which trait call a failure injection applies to. One variant per trait
/// method that can fail in a way tests care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CallKind {
    /// [`RuntimeAdapter::availability`].
    Availability,
    /// [`RuntimeAdapter::current_pane`].
    PaneCurrent,
    /// [`RuntimeAdapter::agents`].
    AgentList,
    /// [`RuntimeAdapter::agent_kinds`].
    AgentKinds,
    /// [`RuntimeAdapter::agent`] (and the default
    /// [`RuntimeAdapter::agent_status`], which goes through it).
    AgentGet,
    /// [`RuntimeAdapter::read_output`].
    AgentRead,
    /// [`RuntimeAdapter::rename_agent`].
    AgentRename,
    /// [`RuntimeAdapter::rename_pane`].
    PaneRename,
    /// [`RuntimeAdapter::pane`].
    PaneGet,
    /// [`RuntimeAdapter::panes`].
    PaneList,
    /// [`RuntimeAdapter::workspaces`].
    WorkspaceList,
    /// [`RuntimeAdapter::split_pane`].
    PaneSplit,
    /// [`RuntimeAdapter::run_command`].
    PaneRun,
    /// [`RuntimeAdapter::send_prompt`].
    AgentPrompt,
    /// [`RuntimeAdapter::send_text`].
    PaneSendText,
    /// [`RuntimeAdapter::send_keys`].
    PaneSendKeys,
}

/// One thing the fake was asked to send, in the order it was sent (§4: read
/// back every prompt and text, in order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    /// `herdr agent prompt <agent> <text>`.
    Prompt { agent: String, text: String },
    /// `herdr pane send-text <pane> <text>`.
    Text { pane: String, text: String },
    /// `herdr pane send-keys <pane> <key> ...`.
    Keys { pane: String, keys: Vec<String> },
    /// `herdr pane run <pane> <command>`.
    Run { pane: String, command: String },
}

struct Failure {
    remaining: u32,
    error: HerdrError,
}

struct State {
    in_pane: bool,
    reachable: bool,
    current_pane: Option<String>,
    panes: BTreeMap<String, PaneInfo>,
    agents: BTreeMap<String, AgentInfo>,
    agent_kinds: Vec<String>,
    workspaces: BTreeMap<String, WorkspaceInfo>,
    outputs: BTreeMap<String, String>,
    sent: Vec<Sent>,
    failures: BTreeMap<CallKind, Failure>,
    next_pane: u32,
}

/// A scriptable in-memory herdr. All methods take `&self` so the fake can be
/// shared; the state is behind a mutex like any real backend's.
pub struct FakeHerdr {
    state: Mutex<State>,
}

impl FakeHerdr {
    /// A fake with one focused workspace `w1`, in-pane and reachable, but no
    /// panes, agents or failures.
    pub fn new() -> Self {
        let mut w = WorkspaceInfo::new("w1", 1, "orchestra");
        w.focused = true;
        FakeHerdr {
            state: Mutex::new(State {
                in_pane: true,
                reachable: true,
                current_pane: None,
                panes: BTreeMap::new(),
                agents: BTreeMap::new(),
                agent_kinds: ["pi", "claude", "codex", "gemini", "agy", "opencode", "amp", "hermes"]
                    .map(String::from)
                    .to_vec(),
                workspaces: BTreeMap::from([("w1".to_string(), w)]),
                outputs: BTreeMap::new(),
                sent: Vec::new(),
                failures: BTreeMap::new(),
                next_pane: 1,
            }),
        }
    }

    /// Set the `HERDR_ENV=1` part of [`Availability`].
    pub fn set_in_pane(&self, in_pane: bool) {
        self.state.lock().unwrap().in_pane = in_pane;
    }

    /// Set whether herdr is installed and reachable.
    pub fn set_reachable(&self, reachable: bool) {
        self.state.lock().unwrap().reachable = reachable;
    }

    /// Set which pane `herdr pane current` reports (`None` = none).
    pub fn set_current_pane(&self, pane: Option<&str>) {
        self.state.lock().unwrap().current_pane = pane.map(str::to_string);
    }

    /// Put a pane into the fake. A focused pane becomes the current pane
    /// unless one was set already.
    pub fn add_pane(&self, pane: PaneInfo) {
        let mut st = self.state.lock().unwrap();
        if pane.focused && st.current_pane.is_none() {
            st.current_pane = Some(pane.pane_id.clone());
        }
        st.panes.insert(pane.pane_id.clone(), pane);
    }

    /// Take a pane out. Agents hosted there disappear with it.
    pub fn remove_pane(&self, pane: &str) {
        let mut st = self.state.lock().unwrap();
        st.panes.remove(pane);
        st.agents.retain(|_, a| a.pane_id != pane);
        if st.current_pane.as_deref() == Some(pane) {
            st.current_pane = None;
        }
    }

    /// Put an agent into the fake, filling its pane's agent and status in.
    pub fn add_agent(&self, agent: AgentInfo) {
        let mut st = self.state.lock().unwrap();
        if let Some(pane) = st.panes.get_mut(&agent.pane_id) {
            if pane.agent.is_none() {
                pane.agent = Some(agent.name.clone());
                pane.status = agent.status;
            }
            if agent.workspace_id.is_empty() {
                let (ws, tab, cwd) = (
                    pane.workspace_id.clone(),
                    pane.tab_id.clone(),
                    pane.cwd.clone(),
                );
                let mut a = agent;
                a.workspace_id = ws;
                a.tab_id = tab;
                a.cwd = a.cwd.or(cwd);
                st.agents.insert(a.name.clone(), a);
                return;
            }
        }
        st.agents.insert(agent.name.clone(), agent);
    }

    /// Take an agent out, clearing its pane's agent and status in.
    pub fn remove_agent(&self, name: &str) {
        let mut st = self.state.lock().unwrap();
        if let Some(a) = st.agents.remove(name)
            && let Some(pane) = st.panes.get_mut(&a.pane_id)
            && pane.agent.as_deref() == Some(name)
        {
            pane.agent = None;
            pane.status = AgentStatus::Unknown;
        }
    }

    /// Script an agent's status; its pane's status follows.
    pub fn set_status(&self, name: &str, status: AgentStatus) {
        let mut st = self.state.lock().unwrap();
        let pane_id = st
            .agents
            .get_mut(name)
            .unwrap_or_else(|| panic!("fake has no agent {name:?}"))
            .pane_id
            .clone();
        st.agents.get_mut(name).unwrap().status = status;
        if let Some(pane) = st.panes.get_mut(&pane_id)
            && pane.agent.as_deref() == Some(name)
        {
            pane.status = status;
        }
    }

    /// Script what `herdr agent read <name>` returns.
    pub fn set_output(&self, name: &str, output: impl Into<String>) {
        self.state
            .lock()
            .unwrap()
            .outputs
            .insert(name.to_string(), output.into());
    }

    /// Script the agent kinds herdr can detect (`herdr agent`'s `kinds:` line).
    pub fn set_agent_kinds(&self, kinds: &[&str]) {
        self.state.lock().unwrap().agent_kinds = kinds.iter().map(|k| k.to_string()).collect();
    }

    /// Put a workspace into the fake.
    pub fn add_workspace(&self, workspace: WorkspaceInfo) {
        self.state
            .lock()
            .unwrap()
            .workspaces
            .insert(workspace.workspace_id.clone(), workspace);
    }

    /// Make the next `count` calls of `kind` fail with `error` (a later
    /// `fail_next` for the same kind replaces the script).
    pub fn fail_next(&self, kind: CallKind, count: u32, error: HerdrError) {
        self.state.lock().unwrap().failures.insert(
            kind,
            Failure {
                remaining: count,
                error,
            },
        );
    }

    /// Everything the fake was asked to send, oldest first.
    pub fn sent(&self) -> Vec<Sent> {
        self.state.lock().unwrap().sent.clone()
    }

    fn failing(&self, st: &mut State, kind: CallKind) -> Result<()> {
        if let Some(f) = st.failures.get_mut(&kind)
            && f.remaining > 0
        {
            f.remaining -= 1;
            return Err(f.error.clone());
        }
        Ok(())
    }
}

impl Default for FakeHerdr {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeAdapter for FakeHerdr {
    fn availability(&self) -> Result<Availability> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::Availability)?;
        Ok(Availability {
            in_pane: st.in_pane,
            reachable: st.reachable,
        })
    }

    fn current_pane(&self) -> Result<PaneInfo> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::PaneCurrent)?;
        let id = st
            .current_pane
            .clone()
            .ok_or_else(|| HerdrError::StalePane {
                pane: "(current)".into(),
                message: "not inside a herdr pane".into(),
            })?;
        st.panes.get(&id).cloned().ok_or_else(|| HerdrError::StalePane {
            pane: id,
            message: "current pane is gone".into(),
        })
    }

    fn agents(&self) -> Result<Vec<AgentInfo>> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::AgentList)?;
        Ok(st.agents.values().cloned().collect())
    }

    fn agent_kinds(&self) -> Result<Vec<String>> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::AgentKinds)?;
        Ok(st.agent_kinds.clone())
    }

    fn agent(&self, target: &str) -> Result<AgentInfo> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::AgentGet)?;
        st.agents.get(target).cloned().ok_or_else(|| HerdrError::AgentNotFound {
            agent: target.to_string(),
            message: format!("agent target {target} not found"),
        })
    }

    fn send_prompt(&self, agent: &str, text: &str) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::AgentPrompt)?;
        let a = st.agents.get(agent).ok_or_else(|| HerdrError::AgentNotFound {
            agent: agent.to_string(),
            message: format!("agent target {agent} not found"),
        })?;
        if a.status == AgentStatus::Blocked {
            return Err(HerdrError::AgentBlocked {
                agent: agent.to_string(),
                message: "agent is blocked, prompt refused".into(),
            });
        }
        let pane = a.pane_id.clone();
        st.sent.push(Sent::Prompt {
            agent: agent.to_string(),
            text: text.to_string(),
        });
        // A delivered prompt starts a turn — but only for an agent herdr
        // watches (§4: unwatchable agents are judged by their done file).
        let a = st.agents.get_mut(agent).expect("checked above");
        if a.status.watchable() {
            a.status = AgentStatus::Working;
            if let Some(pane_info) = st.panes.get_mut(&pane)
                && pane_info.agent.as_deref() == Some(agent)
            {
                pane_info.status = AgentStatus::Working;
            }
        }
        Ok(())
    }

    fn read_output(&self, agent: &str, _lines: u32) -> Result<String> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::AgentRead)?;
        if !st.agents.contains_key(agent) {
            return Err(HerdrError::AgentNotFound {
                agent: agent.to_string(),
                message: format!("agent target {agent} not found"),
            });
        }
        Ok(st.outputs.get(agent).cloned().unwrap_or_default())
    }

    fn rename_agent(&self, target: &str, new_name: &str) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::AgentRename)?;
        let a =
            st.agents
                .get(target)
                .cloned()
                .ok_or_else(|| HerdrError::AgentNotFound {
                    agent: target.to_string(),
                    message: format!("agent target {target} not found"),
                })?;
        if st.agents.contains_key(new_name) && new_name != target {
            return Err(HerdrError::other(format!(
                "agent name {new_name:?} is already in use"
            )));
        }
        st.agents.remove(&a.name);
        let mut a = a;
        a.name = new_name.to_string();
        st.agents.insert(new_name.to_string(), a.clone());
        if let Some(pane) = st.panes.get_mut(&a.pane_id)
            && pane.agent.as_deref() == Some(target)
        {
            pane.agent = Some(new_name.to_string());
        }
        Ok(())
    }

    fn rename_pane(&self, pane: &str, label: &str) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::PaneRename)?;
        st.panes
            .get_mut(pane)
            .ok_or_else(|| HerdrError::StalePane {
                pane: pane.to_string(),
                message: format!("STALE pane {pane}"),
            })?
            .label = Some(label.to_string());
        Ok(())
    }

    fn pane(&self, pane: &str) -> Result<PaneInfo> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::PaneGet)?;
        st.panes.get(pane).cloned().ok_or_else(|| HerdrError::StalePane {
            pane: pane.to_string(),
            message: format!("STALE pane {pane}"),
        })
    }

    fn panes(&self, workspace: Option<&str>) -> Result<Vec<PaneInfo>> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::PaneList)?;
        Ok(st
            .panes
            .values()
            .filter(|p| workspace.is_none_or(|w| p.workspace_id == w))
            .cloned()
            .collect())
    }

    fn workspaces(&self) -> Result<Vec<WorkspaceInfo>> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::WorkspaceList)?;
        Ok(st.workspaces.values().cloned().collect())
    }

    fn split_pane(&self, req: &SplitPane) -> Result<PaneInfo> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::PaneSplit)?;
        let source = match &req.from_pane {
            Some(id) => Some(
                st.panes
                    .get(id)
                    .cloned()
                    .ok_or_else(|| HerdrError::StalePane {
                        pane: id.clone(),
                        message: format!("STALE pane {id}"),
                    })?,
            ),
            None => st
                .current_pane
                .as_ref()
                .and_then(|id| st.panes.get(id))
                .cloned(),
        };
        let (workspace, tab, cwd) = source
            .as_ref()
            .map_or(("w1".to_string(), "w1:t1".to_string(), None), |p| {
                (
                    p.workspace_id.clone(),
                    p.tab_id.clone(),
                    p.cwd.clone(),
                )
            });
        let n = st.next_pane;
        st.next_pane = n + 1;
        // herdr never reuses closed pane ids; the fake does not either.
        let mut pane_id = format!("{workspace}:p{n:x}");
        while st.panes.contains_key(&pane_id) {
            st.next_pane += 1;
            pane_id = format!("{workspace}:p{:x}", st.next_pane);
        }
        let mut pane = PaneInfo::new(&pane_id, &workspace, &tab);
        pane.cwd = req
            .cwd
            .as_ref()
            .map(|p| p.display().to_string())
            .or(cwd);
        if req.focus {
            pane.focused = true;
            if let Some(p) = source.as_ref().and_then(|s| st.panes.get_mut(&s.pane_id)) {
                p.focused = false;
            }
        }
        let out = pane.clone();
        st.panes.insert(pane_id, pane);
        Ok(out)
    }

    fn run_command(&self, pane: &str, command: &str) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::PaneRun)?;
        if !st.panes.contains_key(pane) {
            return Err(HerdrError::StalePane {
                pane: pane.to_string(),
                message: format!("STALE pane {pane}"),
            });
        }
        st.sent.push(Sent::Run {
            pane: pane.to_string(),
            command: command.to_string(),
        });
        Ok(())
    }

    fn send_text(&self, pane: &str, text: &str) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::PaneSendText)?;
        if !st.panes.contains_key(pane) {
            return Err(HerdrError::StalePane {
                pane: pane.to_string(),
                message: format!("STALE pane {pane}"),
            });
        }
        st.sent.push(Sent::Text {
            pane: pane.to_string(),
            text: text.to_string(),
        });
        Ok(())
    }

    fn send_keys(&self, pane: &str, keys: &[&str]) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        self.failing(&mut st, CallKind::PaneSendKeys)?;
        if !st.panes.contains_key(pane) {
            return Err(HerdrError::StalePane {
                pane: pane.to_string(),
                message: format!("STALE pane {pane}"),
            });
        }
        st.sent.push(Sent::Keys {
            pane: pane.to_string(),
            keys: keys.iter().map(|k| k.to_string()).collect(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::SplitPane;

    fn agent(name: &str, pane: &str, status: AgentStatus) -> AgentInfo {
        AgentInfo::new(name, "claude", pane, status)
    }

    #[test]
    fn add_and_remove_agents_and_panes() {
        let f = FakeHerdr::new();
        f.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        f.add_agent(agent("claude-1", "w1:p1", AgentStatus::Idle));
        assert_eq!(
            f.agent("claude-1").unwrap().pane_id,
            "w1:p1"
        );
        assert_eq!(f.pane("w1:p1").unwrap().agent.as_deref(), Some("claude-1"));
        f.remove_agent("claude-1");
        assert_eq!(f.agent_status("claude-1").unwrap(), AgentStatus::Gone);
        assert_eq!(f.pane("w1:p1").unwrap().agent, None);
        f.remove_pane("w1:p1");
        assert!(matches!(
            f.pane("w1:p1"),
            Err(HerdrError::StalePane { .. })
        ));
    }

    #[test]
    fn statuses_can_be_scripted_and_are_visible_through_both_views() {
        let f = FakeHerdr::new();
        f.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        f.add_agent(agent("claude-1", "w1:p1", AgentStatus::Idle));
        f.set_status("claude-1", AgentStatus::Working);
        assert_eq!(f.agent_status("claude-1").unwrap(), AgentStatus::Working);
        assert_eq!(f.pane("w1:p1").unwrap().status, AgentStatus::Working);
        // An unwatchable agent stays unwatchable, never gone/blocked/idle (§4).
        f.set_status("claude-1", AgentStatus::Unwatchable);
        assert_eq!(f.agent_status("claude-1").unwrap(), AgentStatus::Unwatchable);
    }

    #[test]
    fn the_next_n_calls_of_a_kind_fail_then_it_recovers() {
        let f = FakeHerdr::new();
        f.fail_next(CallKind::AgentGet, 2, HerdrError::timed_out(30, "hung"));
        for _ in 0..2 {
            assert!(matches!(f.agent("x"), Err(HerdrError::TimedOut { timeout_s: 30, .. })));
        }
        assert!(matches!(f.agent("x"), Err(HerdrError::AgentNotFound { .. })));
        // A fresh script replaces the old one.
        f.fail_next(CallKind::AgentGet, 1, HerdrError::Unavailable { message: "down".into() });
        assert!(matches!(f.agent("x"), Err(HerdrError::Unavailable { .. })));
    }

    #[test]
    fn prompts_and_texts_are_read_back_in_order() {
        let f = FakeHerdr::new();
        f.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        f.add_pane(PaneInfo::new("w1:p2", "w1", "w1:t1"));
        f.add_agent(agent("claude-1", "w1:p1", AgentStatus::Idle));
        f.send_prompt("claude-1", "first").unwrap();
        f.send_text("w1:p2", "@pane task").unwrap();
        f.send_keys("w1:p2", &["enter"]).unwrap();
        f.run_command("w1:p2", "true").unwrap();
        assert_eq!(
            f.sent(),
            vec![
                Sent::Prompt { agent: "claude-1".into(), text: "first".into() },
                Sent::Text { pane: "w1:p2".into(), text: "@pane task".into() },
                Sent::Keys { pane: "w1:p2".into(), keys: vec!["enter".into()] },
                Sent::Run { pane: "w1:p2".into(), command: "true".into() },
            ]
        );
    }

    #[test]
    fn a_delivered_prompt_starts_a_turn_for_a_watched_agent() {
        let f = FakeHerdr::new();
        f.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        f.add_agent(agent("claude-1", "w1:p1", AgentStatus::Idle));
        f.send_prompt("claude-1", "go").unwrap();
        assert_eq!(f.agent_status("claude-1").unwrap(), AgentStatus::Working);
    }

    #[test]
    fn a_prompt_to_a_blocked_agent_is_refused() {
        let f = FakeHerdr::new();
        f.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        f.add_agent(agent("claude-1", "w1:p1", AgentStatus::Blocked));
        assert!(matches!(
            f.send_prompt("claude-1", "go"),
            Err(HerdrError::AgentBlocked { agent, .. }) if agent == "claude-1"
        ));
        assert!(f.sent().is_empty());
    }

    #[test]
    fn availability_follows_the_script() {
        let f = FakeHerdr::new();
        assert_eq!(
            f.availability().unwrap(),
            Availability { in_pane: true, reachable: true }
        );
        f.set_in_pane(false);
        f.set_reachable(false);
        assert_eq!(
            f.availability().unwrap(),
            Availability { in_pane: false, reachable: false }
        );
    }

    #[test]
    fn current_pane_reports_the_focused_or_set_pane() {
        let f = FakeHerdr::new();
        assert!(matches!(f.current_pane(), Err(HerdrError::StalePane { .. })));
        let mut p = PaneInfo::new("w1:p1", "w1", "w1:t1");
        p.focused = true;
        f.add_pane(p);
        assert_eq!(f.current_pane().unwrap().pane_id, "w1:p1");
        f.set_current_pane(Some("w1:p1"));
        f.remove_pane("w1:p1");
        assert!(matches!(f.current_pane(), Err(HerdrError::StalePane { .. })));
    }

    #[test]
    fn splitting_creates_a_new_pane_in_the_requested_folder() {
        let f = FakeHerdr::new();
        f.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        let out = f
            .split_pane(&SplitPane::right("w1:p1", std::path::Path::new("/repo/wt")))
            .unwrap();
        assert_ne!(out.pane_id, "w1:p1");
        assert_eq!(out.workspace_id, "w1");
        assert_eq!(out.cwd.as_deref(), Some("/repo/wt"));
        assert_eq!(f.pane(&out.pane_id).unwrap(), out);
        // A stale source pane is refused.
        assert!(matches!(
            f.split_pane(&SplitPane::right("w1:pX", std::path::Path::new("/r"))),
            Err(HerdrError::StalePane { .. })
        ));
    }

    #[test]
    fn renames_reach_the_agent_and_the_pane() {
        let f = FakeHerdr::new();
        f.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        f.add_agent(agent("claude-1", "w1:p1", AgentStatus::Idle));
        f.rename_agent("claude-1", "reviewer-2").unwrap();
        assert!(f.agent("claude-1").is_err());
        assert_eq!(f.agent("reviewer-2").unwrap().pane_id, "w1:p1");
        assert_eq!(
            f.pane("w1:p1").unwrap().agent.as_deref(),
            Some("reviewer-2")
        );
        f.rename_pane("w1:p1", "reviewer").unwrap();
        assert_eq!(f.pane("w1:p1").unwrap().label.as_deref(), Some("reviewer"));
        assert!(matches!(
            f.rename_pane("w1:pX", "x"),
            Err(HerdrError::StalePane { .. })
        ));
    }

    #[test]
    fn panes_filter_by_workspace_and_workspaces_are_listed() {
        let f = FakeHerdr::new();
        f.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        f.add_pane(PaneInfo::new("w2:p1", "w2", "w2:t1"));
        assert_eq!(f.panes(None).unwrap().len(), 2);
        assert_eq!(f.panes(Some("w2")).unwrap().len(), 1);
        let ws = f.workspaces().unwrap();
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].workspace_id, "w1");
    }

    #[test]
    fn pane_text_and_keys_refuse_stale_panes() {
        let f = FakeHerdr::new();
        for r in [
            f.send_text("w1:pX", "t"),
            f.send_keys("w1:pX", &["enter"]),
            f.run_command("w1:pX", "true"),
            f.pane("w1:pX").map(|_| ()),
        ] {
            assert!(matches!(r, Err(HerdrError::StalePane { .. })));
        }
    }

    #[test]
    fn read_output_serves_the_scripted_text() {
        let f = FakeHerdr::new();
        f.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        f.add_agent(agent("claude-1", "w1:p1", AgentStatus::Working));
        assert_eq!(f.read_output("claude-1", 50).unwrap(), "");
        f.set_output("claude-1", "task done");
        assert_eq!(f.read_output("claude-1", 50).unwrap(), "task done");
        assert!(matches!(
            f.read_output("nobody", 50),
            Err(HerdrError::AgentNotFound { .. })
        ));
    }
}
