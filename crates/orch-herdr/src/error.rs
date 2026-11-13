//! The error type of the runtime adapter. Callers distinguish every situation
//! the recovery rules (§15) need: agent not found, stale pane, blocked agent,
//! timeout, herdr unreachable, and anything else herdr failed with.

/// The result type of every adapter call: a value or a [`HerdrError`].
pub type Result<T> = std::result::Result<T, HerdrError>;

/// Everything that can go wrong talking to the herdr runtime.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HerdrError {
    /// herdr has no such agent (`agent_not_found`); the agent exited, was
    /// released, or the name is wrong.
    #[error("agent {agent:?} not found: {message}")]
    AgentNotFound {
        /// The agent (or agent target) that was asked for.
        agent: String,
        /// herdr's own message, kept for the log.
        message: String,
    },
    /// The pane id is stale or unknown (herdr's `STALE pane`); ids are not
    /// reused, so this means "re-check with `herdr pane list`".
    #[error("stale or unknown pane {pane:?}: {message}")]
    StalePane {
        /// The pane id that was asked for.
        pane: String,
        /// herdr's own message, kept for the log.
        message: String,
    },
    /// A prompt was refused because the agent is blocked (an approval or
    /// question UI is up and herdr won't deliver into it).
    #[error("agent {agent:?} is blocked, prompt refused: {message}")]
    AgentBlocked {
        /// The agent whose prompt was refused.
        agent: String,
        /// herdr's own message, kept for the log.
        message: String,
    },
    /// A herdr call ran longer than the configured timeout
    /// (`ORCH_HERDR_TIMEOUT_S`, default 30 s).
    #[error("herdr call timed out after {timeout_s}s: {message}")]
    TimedOut {
        /// The timeout that was exceeded, in seconds.
        timeout_s: u64,
        /// What timed out (the command or call).
        message: String,
    },
    /// `herdr` is not installed, not on PATH, or its server is not reachable.
    #[error("herdr is not installed or not reachable: {message}")]
    Unavailable {
        /// Why herdr is unusable (exec failure, socket refused, …).
        message: String,
    },
    /// Any other herdr failure, keeping herdr's message.
    #[error("herdr failed: {message}")]
    Other {
        /// herdr's message (or our own context for it).
        message: String,
    },
}

impl HerdrError {
    /// A [`HerdrError::Other`] with a plain message.
    pub fn other(message: impl Into<String>) -> Self {
        HerdrError::Other {
            message: message.into(),
        }
    }

    /// A [`HerdrError::TimedOut`] for one call.
    pub fn timed_out(timeout_s: u64, message: impl Into<String>) -> Self {
        HerdrError::TimedOut {
            timeout_s,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // §4/AC24: callers must be able to match on every kind of failure.
    #[test]
    fn agent_not_found_matches() {
        let e = HerdrError::AgentNotFound {
            agent: "claude-1".into(),
            message: "agent target claude-1 not found".into(),
        };
        assert!(matches!(e, HerdrError::AgentNotFound { ref agent, .. } if agent == "claude-1"));
        assert!(e.to_string().contains("claude-1"));
    }

    #[test]
    fn stale_pane_matches() {
        let e = HerdrError::StalePane {
            pane: "w1:p3".into(),
            message: "STALE pane w1:p3".into(),
        };
        assert!(matches!(e, HerdrError::StalePane { ref pane, .. } if pane == "w1:p3"));
        assert!(e.to_string().contains("stale or unknown pane"));
    }

    #[test]
    fn agent_blocked_matches() {
        let e = HerdrError::AgentBlocked {
            agent: "codex-2".into(),
            message: "waiting for approval".into(),
        };
        assert!(matches!(e, HerdrError::AgentBlocked { ref agent, .. } if agent == "codex-2"));
        assert!(e.to_string().contains("prompt refused"));
    }

    #[test]
    fn timed_out_matches() {
        let e = HerdrError::timed_out(30, "agent prompt timed out");
        assert!(matches!(e, HerdrError::TimedOut { timeout_s: 30, .. }));
        assert!(e.to_string().contains("timed out after 30s"));
    }

    #[test]
    fn unavailable_matches() {
        let e = HerdrError::Unavailable {
            message: "herdr: command not found".into(),
        };
        assert!(matches!(e, HerdrError::Unavailable { .. }));
        assert!(e.to_string().contains("not installed or not reachable"));
    }

    #[test]
    fn other_matches_and_keeps_the_message() {
        let e = HerdrError::other("herdr socket said: protocol mismatch");
        assert!(
            matches!(e, HerdrError::Other { ref message } if message.contains("protocol mismatch"))
        );
        assert!(e.to_string().contains("protocol mismatch"));
    }

    #[test]
    fn errors_compare_like_values_so_the_fake_can_inject_them() {
        assert_eq!(HerdrError::other("x"), HerdrError::other("x"));
        assert_ne!(HerdrError::other("x"), HerdrError::other("y"));
    }
}
