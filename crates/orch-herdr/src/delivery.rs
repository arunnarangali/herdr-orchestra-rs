//! Crash-safe prompt delivery (§4, §10, §15, §16): the prompt is recorded
//! durably in the store **before** anything is sent to the runtime, it is
//! confirmed when the runtime accepts the send, and an unconfirmed delivery
//! found after a restart is either confirmed from the agent's status or
//! re-sent to the **same** agent with the same prompt, task and attempt —
//! never handed to a second agent, never silently sent twice.
//!
//! This module only provides the operations and their reports (record/send,
//! recover, resend, the per-agent failure streak); Phase 6's event manager
//! calls them. There is no scheduler, retry loop, benching or lead wake-up
//! here.

use crate::adapter::RuntimeAdapter;
use crate::error::HerdrError;
use crate::status::AgentStatus;
use orch_config::tunables::Tunables;
use orch_core::store::{Delivery, DeliveryKind, Store, StoreError};

/// The fallback confirm grace when no tunable says otherwise (§15 recovery:
/// an unconfirmed wait older than this is resolved from the agent's status).
pub const DEFAULT_CONFIRM_GRACE_S: i64 = 30;

/// §15: bench an agent after this many consecutive failed deliveries. Reading
/// the counter is this module's job; benching is not.
pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// The confirm grace to use: at least `ORCH_CONFIRM_S` (the tunable, default
/// 30), and never below [`DEFAULT_CONFIRM_GRACE_S`].
pub fn confirm_grace(tunables: &Tunables) -> i64 {
    tunables.confirm_s.max(DEFAULT_CONFIRM_GRACE_S)
}

/// The short herdr word for a send error, kept in the delivery record.
fn error_kind(error: &HerdrError) -> &'static str {
    match error {
        HerdrError::AgentNotFound { .. } => "agent_not_found",
        HerdrError::StalePane { .. } => "stale_pane",
        HerdrError::AgentBlocked { .. } => "agent_blocked",
        HerdrError::TimedOut { .. } => "timed_out",
        HerdrError::Unavailable { .. } => "unavailable",
        HerdrError::Other { .. } => "other",
    }
}

/// A prompt to record and deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewDelivery<'a> {
    /// The one agent this may be delivered to.
    pub agent: &'a str,
    /// The task the prompt belongs to.
    pub task: &'a str,
    /// The attempt the prompt starts.
    pub attempt: u32,
    /// Coder or reviewer.
    pub kind: DeliveryKind,
    pub prompt: &'a str,
}

/// What one send (or re-send) did.
#[derive(Debug, Clone, PartialEq)]
pub enum SendOutcome {
    /// The runtime accepted the prompt: the delivery is confirmed and the
    /// agent's consecutive-failure count is reset.
    Sent { delivery: Delivery },
    /// The runtime refused it: the delivery stays unconfirmed with the error
    /// kind and message kept, and the agent's failure count went up.
    Failed {
        delivery: Delivery,
        error: HerdrError,
    },
}

/// What recovery decided about one unconfirmed delivery.
#[derive(Debug, Clone, PartialEq)]
pub enum Recovery {
    /// The agent is working or blocked: the prompt reached it. Confirmed
    /// without re-sending.
    Confirmed { delivery: Delivery },
    /// The agent is gone (herdr reports `agent_not_found`): reported, not
    /// re-sent, not confirmed.
    Gone { delivery: Delivery },
    /// Still unconfirmed and re-sendable — the agent is idle/done/unknown, or
    /// is not watchable, or the runtime could not be asked. Never re-sent
    /// automatically; [`Deliverer::resend`] does that on request.
    Unconfirmed { delivery: Delivery },
}

/// The delivery operations over one store and one runtime adapter. `watchable`
/// says whether herdr can report an agent's status (the registry's
/// `herdr_detect` for the agent's kind); unwatchable agents are never judged
/// by herdr (§4).
pub struct Deliverer<'a> {
    store: &'a mut Store,
    adapter: &'a dyn RuntimeAdapter,
    watchable: &'a dyn Fn(&str) -> bool,
}

impl<'a> Deliverer<'a> {
    pub fn new(
        store: &'a mut Store,
        adapter: &'a dyn RuntimeAdapter,
        watchable: &'a dyn Fn(&str) -> bool,
    ) -> Self {
        Deliverer {
            store,
            adapter,
            watchable,
        }
    }

    /// Record the delivery durably, then send it (§15: the record is committed
    /// before anything reaches the runtime, so a crash in between leaves the
    /// job recoverable, not lost).
    pub fn send(&mut self, req: NewDelivery<'_>) -> Result<SendOutcome, StoreError> {
        let id =
            self.store
                .record_delivery(req.agent, req.task, req.attempt, req.kind, req.prompt)?;
        self.deliver(id)
    }

    /// Re-send an unconfirmed delivery to the agent it was recorded for, with
    /// the same prompt, task and attempt (§15, `orch resend`'s engine). There
    /// is deliberately no way to re-send it to a different agent: the API
    /// takes only the delivery id.
    pub fn resend(&mut self, id: i64) -> Result<SendOutcome, StoreError> {
        let delivery = self.store.delivery(id)?;
        if delivery.confirmed {
            return Err(StoreError::DeliveryConfirmed(id));
        }
        self.deliver(id)
    }

    fn deliver(&mut self, id: i64) -> Result<SendOutcome, StoreError> {
        let delivery = self.store.delivery(id)?;
        match self.adapter.send_prompt(&delivery.agent, &delivery.prompt) {
            Ok(()) => {
                self.store.confirm_delivery(id)?;
                self.store.reset_delivery_fails(&delivery.agent)?;
                Ok(SendOutcome::Sent {
                    delivery: self.store.delivery(id)?,
                })
            }
            Err(error) => {
                self.store
                    .record_delivery_error(id, error_kind(&error), &error.to_string())?;
                self.store.bump_delivery_fails(&delivery.agent)?;
                Ok(SendOutcome::Failed {
                    delivery: self.store.delivery(id)?,
                    error,
                })
            }
        }
    }

    /// Resolve the unconfirmed deliveries a restart finds (§15: crash
    /// mid-step). A delivery younger than `grace_s` seconds is left alone
    /// (the send may still be in flight). For a watchable agent, an older
    /// one is confirmed when the agent is working or blocked, reported gone
    /// when herdr no longer knows the agent, and otherwise left unconfirmed
    /// and re-sendable. For an unwatchable agent it is always reported
    /// unconfirmed: herdr is not consulted, and nothing is re-sent or
    /// confirmed automatically (§4). `now` is the current unix time in
    /// seconds, passed in so callers (and tests) control the clock.
    pub fn recover(&mut self, now: i64, grace_s: i64) -> Result<Vec<Recovery>, StoreError> {
        let mut out = Vec::new();
        for d in self.store.unconfirmed_deliveries()? {
            if now - d.recorded_at < grace_s {
                continue;
            }
            if !(self.watchable)(&d.agent) {
                out.push(Recovery::Unconfirmed { delivery: d });
                continue;
            }
            // A send the runtime definitively refused never reached the agent, so a
            // busy agent says nothing about it: it must not be confirmed from status.
            let refused = matches!(
                d.error_kind.as_deref(),
                Some("agent_blocked" | "agent_not_found" | "stale_pane")
            );
            match self.adapter.agent_status(&d.agent) {
                Ok(AgentStatus::Working | AgentStatus::Blocked) if !refused => {
                    self.store.confirm_delivery(d.id)?;
                    out.push(Recovery::Confirmed {
                        delivery: self.store.delivery(d.id)?,
                    });
                }
                Ok(AgentStatus::Gone) => out.push(Recovery::Gone { delivery: d }),
                // idle/done/unknown, or the runtime could not be asked
                // (timed out, unreachable): cannot tell, so it stays
                // unconfirmed and re-sendable.
                _ => out.push(Recovery::Unconfirmed { delivery: d }),
            }
        }
        Ok(out)
    }

    /// The agent's consecutive delivery failures (§15); Phase 6 benches an
    /// agent once this reaches [`MAX_CONSECUTIVE_FAILURES`]. Reading only —
    /// benching itself is not implemented here.
    pub fn failure_streak(&self, agent: &str) -> Result<u32, StoreError> {
        self.store.delivery_fails(agent)
    }
}
