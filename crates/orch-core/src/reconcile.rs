//! Recovery reconciliation (§15, §24 Phase 6, AC8): on start, the event manager brings
//! whatever a crash or restart left half-done back to a consistent state — unconfirmed
//! prompt deliveries, tasks in `coding` whose agent vanished, a half-finished wake-up —
//! without ever marking incomplete work as completed. (The half-done *merge* journal is
//! Phase 7's `Work::commit` business, not this module's.)
//!
//! Parking one task never touches the others: every state change is one store
//! transaction.

use crate::events::{AgentState, RuntimeView};
use crate::manager::{Deliveries, DeliveryOutcome, LoopTunables, Recovered, Work};
use crate::store::{Counter, Delivery, DeliveryKind, Res, Store, TransitionRequest};
use crate::task::State;

/// What start-up reconciliation found and did, one line per action (the loop logs
/// each through `say`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    pub notes: Vec<String>,
}

/// Park a milestone: free its coder (keeping `last_worker`), keep its work behind the
/// [`Work`] trait when one is given (best effort — parking must succeed even if the
/// ref update fails), and count a lost attempt when asked. At `max_lost` lost attempts
/// the milestone fails with its history instead of going back to the queue. Returns
/// the status the milestone ended in.
pub fn park_with_lost(
    store: &mut Store,
    work: Option<&mut dyn Work>,
    max_lost: i64,
    task: &str,
    status: State,
    reason: &str,
    lost: bool,
) -> Res<State> {
    let lost_n = if lost {
        store.bump(task, Counter::Lost)?
    } else {
        store.get_task(task)?.lost
    };
    let (final_status, why) = if lost && (lost_n as i64) >= max_lost.max(1) {
        (
            State::Failed,
            format!("lost {lost_n} attempts (last: {reason})"),
        )
    } else {
        (status, reason.to_string())
    };
    if let Some(w) = work {
        let _ = w.save_work(task);
    }
    store.release_claim(task).ok();
    store.transition(task, &TransitionRequest::auto(final_status, Some(&why)))?;
    Ok(final_status)
}

/// Bring a crashed or restarted run back to a consistent state (AC8). Called once,
/// before the loop starts; every finding becomes a note the caller logs.
#[allow(clippy::too_many_arguments)]
pub fn reconcile(
    store: &mut Store,
    runtime: &dyn RuntimeView,
    deliveries: &mut dyn Deliveries,
    watchable: &dyn Fn(&str) -> bool,
    work: &mut dyn Work,
    tun: &LoopTunables,
    now: i64,
    say: &mut dyn FnMut(&str),
) -> Res<ReconcileReport> {
    let mut report = ReconcileReport::default();

    // 1. Prompts recorded but never confirmed (a crash between record and send,
    //    §15): confirm from the agent's status, report a gone agent, or re-send to
    //    the *same* agent — never a second coder (AC3).
    let recoveries = deliveries.recover(store, now, tun.confirm_grace_s())?;
    for r in recoveries {
        match r {
            Recovered::Confirmed { delivery } => report.notes.push(format!(
                "RECOVERED {}: the prompt to {} was confirmed from the agent's status",
                delivery.task, delivery.agent
            )),
            Recovered::Gone { delivery } => delivery_gone(store, &delivery, tun, now, &mut report),
            Recovered::Unconfirmed { delivery } => {
                resend_unconfirmed(store, deliveries, &delivery, &mut report)
            }
        }
    }

    // 2. Tasks in `coding` whose agent herdr no longer knows (or that has no agent
    //    at all): the work goes back to the queue for the next free coder (AC8).
    let team: Vec<String> = store
        .load_run()?
        .map(|r| {
            r.agents
                .iter()
                .filter(|a| a.role == crate::run_store::AgentRole::Worker)
                .map(|a| a.name.clone())
                .collect()
        })
        .unwrap_or_default();
    for t in store.list_tasks()? {
        if t.status != State::Coding {
            continue;
        }
        match t.worker.as_deref() {
            None => {
                park_with_lost(
                    store,
                    Some(&mut *work),
                    tun.max_lost,
                    &t.id,
                    State::Pending,
                    "its prompt never went out (interrupted run) — will be sent again",
                    true,
                )?;
                report
                    .notes
                    .push(format!("RECOVERED {}: no agent was prompted — it goes back to the queue", t.id));
            }
            Some(agent) if !team.iter().any(|w| w == agent) => {
                park_with_lost(
                    store,
                    Some(&mut *work),
                    tun.max_lost,
                    &t.id,
                    State::Pending,
                    &format!("{agent} is not on this run's team"),
                    true,
                )?;
                report.notes.push(format!(
                    "RECOVERED {}: its coder {agent} is not on the team — it goes back to the queue",
                    t.id
                ));
            }
            Some(agent)
                if (watchable)(agent) && runtime.status(agent) == Some(AgentState::Gone) =>
            {
                store.bench_agent(agent, "gone", now + tun.gone_cooldown_min * 60)?;
                park_with_lost(
                    store,
                    Some(&mut *work),
                    tun.max_lost,
                    &t.id,
                    State::Pending,
                    &format!("{agent} disappeared from herdr"),
                    true,
                )?;
                report.notes.push(format!(
                    "RECOVERED {}: {agent} vanished from herdr — it goes to the next free coder",
                    t.id
                ));
            }
            _ => {}
        }
    }

    // 3. A half-finished wake-up: the state says the lead was woken and is being
    //    waited for, but nothing is left for it to read — clear the wait so the
    //    wake logic starts fresh.
    let mut st = store.wake_state()?;
    if st.awaiting && store.unread_lead_events()?.is_empty() {
        st.awaiting = false;
        store.save_wake_state(&st)?;
        report
            .notes
            .push("RECOVERED wake: cleared a half-finished wake-up (nothing left for the lead to read)".to_string());
    }

    for n in &report.notes {
        say(n);
    }
    Ok(report)
}

/// A `GONE` recovery for one delivery, by the delivery's kind (the loop's gone
/// handling, from the store's state rather than a live event).
fn delivery_gone(
    store: &mut Store,
    d: &Delivery,
    tun: &LoopTunables,
    now: i64,
    report: &mut ReconcileReport,
) {
    let _ = store.bench_agent(&d.agent, "gone", now + tun.gone_cooldown_min * 60);
    match d.kind {
        DeliveryKind::Reviewer => {
            let moved = store
                .get_task(&d.task)
                .ok()
                .is_some_and(|t| t.status == State::Reviewing)
                && store
                    .transition(
                        &d.task,
                        &TransitionRequest::auto(State::AwaitingReview, Some("the reviewer is gone")),
                    )
                    .is_ok();
            if moved {
                report.notes.push(format!(
                    "RECOVERED {}: the reviewer {} is gone — it waits for review again",
                    d.task, d.agent
                ));
            } else {
                report.notes.push(format!(
                    "GONE {} agent={} (the prompt to it was never confirmed)",
                    d.task, d.agent
                ));
            }
        }
        DeliveryKind::Coder => {
            let parked = store
                .get_task(&d.task)
                .ok()
                .is_some_and(|t| t.status == State::Coding)
                && park_with_lost(
                    store,
                    None,
                    tun.max_lost,
                    &d.task,
                    State::Pending,
                    &format!("{} disappeared from herdr", d.agent),
                    true,
                )
                .is_ok();
            if parked {
                report.notes.push(format!(
                    "RECOVERED {}: {} vanished from herdr — it goes to the next free coder",
                    d.task, d.agent
                ));
            } else {
                report.notes.push(format!(
                    "GONE {} agent={} (the prompt to it was never confirmed)",
                    d.task, d.agent
                ));
            }
        }
    }
}

/// An `UNCONFIRMED` recovery: re-send to the same agent, but only while the task is
/// still in the state the prompt serves — a parked or re-queued task gets a fresh
/// prompt from the scheduler instead (never silently sent twice).
fn resend_unconfirmed(
    store: &mut Store,
    deliveries: &mut dyn Deliveries,
    d: &Delivery,
    report: &mut ReconcileReport,
) {
    let wanted = match d.kind {
        DeliveryKind::Coder => State::Coding,
        DeliveryKind::Reviewer => State::Reviewing,
    };
    let in_state = store
        .get_task(&d.task)
        .map(|t| t.status == wanted)
        .unwrap_or(false);
    if !in_state {
        report.notes.push(format!(
            "UNCONFIRMED {} agent={} — the task is not {} any more; it gets a fresh prompt when dispatched",
            d.task, d.agent, wanted
        ));
        return;
    }
    match deliveries.resend(store, d.id) {
        Ok(DeliveryOutcome::Sent { .. }) => report.notes.push(format!(
            "RECOVERED {}: re-sent the unconfirmed prompt to the same agent {}",
            d.task, d.agent
        )),
        Ok(DeliveryOutcome::Failed { error, .. }) => report.notes.push(format!(
            "{}: the re-send to {} failed ({error}) — it is retried on the next pass",
            d.task, d.agent
        )),
        Err(e) => report.notes.push(format!("{}: {e}", d.task)),
    }
}
