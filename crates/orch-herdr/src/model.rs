//! The typed values the runtime adapter returns: one agent, one pane, one
//! workspace. Field names follow herdr 0.9.5's JSON (`herdr agent list`,
//! `herdr pane get`, `herdr workspace list`, `herdr api schema --json`) so the
//! M2 CLI backend can deserialise herdr's replies straight into them; extra
//! fields herdr sends are ignored.

use crate::status::AgentStatus;

/// One agent as `herdr agent list` / `herdr agent get <target>` reports it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentInfo {
    /// The agent's unique live name (`name`).
    pub name: String,
    /// The agent kind, e.g. `claude` (herdr's field `agent`).
    #[serde(rename = "agent")]
    pub kind: String,
    /// The pane hosting the agent (`pane_id`, e.g. `w1:p1`).
    pub pane_id: String,
    /// The pane's tab (`tab_id`).
    pub tab_id: String,
    /// The pane's workspace (`workspace_id`).
    pub workspace_id: String,
    /// The folder the agent runs in (`cwd`), when herdr knows it.
    #[serde(default)]
    pub cwd: Option<String>,
    /// The lifecycle state (`agent_status`).
    #[serde(rename = "agent_status")]
    pub status: AgentStatus,
    /// Whether this agent's pane has focus (`focused`).
    #[serde(default)]
    pub focused: bool,
}

/// One pane as `herdr pane get` / `herdr pane list` reports it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PaneInfo {
    /// The pane's id (`pane_id`, e.g. `w1:p1`).
    pub pane_id: String,
    /// The pane's workspace (`workspace_id`).
    pub workspace_id: String,
    /// The pane's tab (`tab_id`).
    pub tab_id: String,
    /// The pane's label, set with `herdr pane rename` (`label`).
    #[serde(default)]
    pub label: Option<String>,
    /// The pane's working folder (`cwd`), when herdr knows it.
    #[serde(default)]
    pub cwd: Option<String>,
    /// The recognised agent in the pane, if any (`agent`).
    #[serde(default)]
    pub agent: Option<String>,
    /// The pane agent's state (`agent_status`).
    #[serde(rename = "agent_status")]
    pub status: AgentStatus,
    /// Whether this pane has focus (`focused`).
    #[serde(default)]
    pub focused: bool,
}

/// One workspace as `herdr workspace list` reports it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceInfo {
    /// The workspace's id (`workspace_id`, e.g. `w1`).
    pub workspace_id: String,
    /// The workspace's number (`number`).
    pub number: u32,
    /// The workspace's label (`label`).
    pub label: String,
    /// Whether this workspace has focus (`focused`).
    #[serde(default)]
    pub focused: bool,
    /// How many panes it holds (`pane_count`).
    #[serde(default)]
    pub pane_count: u32,
    /// How many tabs it holds (`tab_count`).
    #[serde(default)]
    pub tab_count: u32,
    /// The workspace's active tab (`active_tab_id`).
    #[serde(default)]
    pub active_tab_id: String,
}

impl PaneInfo {
    /// A minimal pane: no label, no cwd, no agent, not focused.
    pub fn new(pane_id: &str, workspace_id: &str, tab_id: &str) -> Self {
        PaneInfo {
            pane_id: pane_id.to_string(),
            workspace_id: workspace_id.to_string(),
            tab_id: tab_id.to_string(),
            label: None,
            cwd: None,
            agent: None,
            status: AgentStatus::Unknown,
            focused: false,
        }
    }
}

impl AgentInfo {
    /// A minimal agent record: unknown cwd, not focused.
    pub fn new(name: &str, kind: &str, pane_id: &str, status: AgentStatus) -> Self {
        AgentInfo {
            name: name.to_string(),
            kind: kind.to_string(),
            pane_id: pane_id.to_string(),
            tab_id: String::new(),
            workspace_id: String::new(),
            cwd: None,
            status,
            focused: false,
        }
    }
}

impl WorkspaceInfo {
    /// A minimal workspace: focused off, no counters, no active tab.
    pub fn new(workspace_id: &str, number: u32, label: &str) -> Self {
        WorkspaceInfo {
            workspace_id: workspace_id.to_string(),
            number,
            label: label.to_string(),
            focused: false,
            pane_count: 0,
            tab_count: 0,
            active_tab_id: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_info_deserialises_herdr_json_ignoring_extra_fields() {
        let json = r#"{
            "agent": "claude", "agent_status": "done", "cwd": "/repo",
            "focused": true, "foreground_cwd": "/repo", "name": "lead",
            "pane_id": "w1:p1", "revision": 14, "state_change_seq": 120,
            "tab_id": "w1:t1", "terminal_id": "term_x",
            "terminal_title": "t", "workspace_id": "w1"
        }"#;
        let a: AgentInfo = serde_json::from_str(json).unwrap();
        assert_eq!(a.name, "lead");
        assert_eq!(a.kind, "claude");
        assert_eq!(a.pane_id, "w1:p1");
        assert_eq!(a.status, AgentStatus::Done);
        assert!(a.focused);
        assert_eq!(a.cwd.as_deref(), Some("/repo"));
    }

    #[test]
    fn pane_info_deserialises_herdr_json() {
        let json = r#"{
            "agent": "codex", "agent_status": "working", "cwd": "/repo",
            "focused": false, "label": "codex-2", "pane_id": "w1:p3",
            "revision": 7, "tab_id": "w1:t1", "terminal_id": "term_y",
            "workspace_id": "w1"
        }"#;
        let p: PaneInfo = serde_json::from_str(json).unwrap();
        assert_eq!(p.pane_id, "w1:p3");
        assert_eq!(p.label.as_deref(), Some("codex-2"));
        assert_eq!(p.agent.as_deref(), Some("codex"));
        assert_eq!(p.status, AgentStatus::Working);
    }

    #[test]
    fn workspace_info_deserialises_herdr_json() {
        let json = r#"{
            "active_tab_id": "w1:t1", "agent_status": "idle", "focused": true,
            "label": "orchestra", "number": 1, "pane_count": 4,
            "tab_count": 1, "workspace_id": "w1"
        }"#;
        let w: WorkspaceInfo = serde_json::from_str(json).unwrap();
        assert_eq!(w.workspace_id, "w1");
        assert_eq!(w.number, 1);
        assert_eq!(w.pane_count, 4);
        assert!(w.focused);
    }
}
