//! An agent's status as herdr reports it, extended with what orch adds on top
//! (§4): an agent herdr cannot see any more is *gone*, and an agent whose kind
//! has `herdr_detect = false` in the registry is *unwatchable*.
//!
//! herdr 0.9.5 itself only knows `idle`, `working`, `blocked`, `done` and
//! `unknown` (`herdr api schema --json`, the `AgentStatus` enum); "not found"
//! comes back as an error with code `agent_not_found`, which the adapter turns
//! into [`AgentStatus::Gone`].

/// The lifecycle state of one agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// herdr: `idle` — ready for input.
    Idle,
    /// herdr: `working` — the agent is busy.
    Working,
    /// herdr: `blocked` — herdr recognised an approval or question UI.
    Blocked,
    /// herdr: `done` — finished its turn, ready for input.
    Done,
    /// herdr: `unknown` — an agent is present but herdr cannot classify it.
    Unknown,
    /// orch: `herdr agent get` answered `agent_not_found`. Never reported for
    /// an unwatchable agent (§4).
    Gone,
    /// orch: the agent's kind has `herdr_detect = false`; herdr cannot watch
    /// it. Such an agent is never reported as gone, blocked or idle (§4); it is
    /// judged only by its done file and time budget.
    Unwatchable,
}

impl AgentStatus {
    /// Whether herdr can watch an agent with this status. Only
    /// [`AgentStatus::Unwatchable`] is not watchable.
    pub fn watchable(&self) -> bool {
        !matches!(self, AgentStatus::Unwatchable)
    }

    /// Parse the status string herdr prints (`idle`, `working`, `blocked`,
    /// `done`, `unknown`); `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "idle" => AgentStatus::Idle,
            "working" => AgentStatus::Working,
            "blocked" => AgentStatus::Blocked,
            "done" => AgentStatus::Done,
            "unknown" => AgentStatus::Unknown,
            _ => return None,
        })
    }
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AgentStatus::Idle => "idle",
            AgentStatus::Working => "working",
            AgentStatus::Blocked => "blocked",
            AgentStatus::Done => "done",
            AgentStatus::Unknown => "unknown",
            AgentStatus::Gone => "gone",
            AgentStatus::Unwatchable => "unwatchable",
        })
    }
}

impl std::str::FromStr for AgentStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AgentStatus::parse(s).ok_or_else(|| format!("unknown agent status {s:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_herdr_statuses() {
        for (text, want) in [
            ("idle", AgentStatus::Idle),
            ("working", AgentStatus::Working),
            ("blocked", AgentStatus::Blocked),
            ("done", AgentStatus::Done),
            ("unknown", AgentStatus::Unknown),
        ] {
            assert_eq!(AgentStatus::parse(text), Some(want), "{text}");
            assert_eq!(text.parse::<AgentStatus>(), Ok(want), "{text}");
        }
        assert_eq!(AgentStatus::parse("running"), None);
        assert!("running".parse::<AgentStatus>().is_err());
    }

    #[test]
    fn statuses_round_trip_through_serde_and_display() {
        for s in [
            AgentStatus::Idle,
            AgentStatus::Working,
            AgentStatus::Blocked,
            AgentStatus::Done,
            AgentStatus::Unknown,
            AgentStatus::Gone,
            AgentStatus::Unwatchable,
        ] {
            let text = format!("\"{s}\"");
            assert_eq!(serde_json::to_string(&s).unwrap(), text, "{s:?}");
            assert_eq!(serde_json::from_str::<AgentStatus>(&text).unwrap(), s);
        }
    }

    // §4: an unwatchable agent is never reported as gone, blocked or idle.
    #[test]
    fn unwatchable_is_never_gone_blocked_or_idle() {
        assert_ne!(AgentStatus::Unwatchable, AgentStatus::Gone);
        assert_ne!(AgentStatus::Unwatchable, AgentStatus::Blocked);
        assert_ne!(AgentStatus::Unwatchable, AgentStatus::Idle);
        assert!(!AgentStatus::Unwatchable.watchable());
        for s in [
            AgentStatus::Idle,
            AgentStatus::Working,
            AgentStatus::Blocked,
            AgentStatus::Done,
            AgentStatus::Unknown,
            AgentStatus::Gone,
        ] {
            assert!(s.watchable(), "{s:?}");
        }
    }
}
