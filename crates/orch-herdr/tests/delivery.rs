//! Crash-safe delivery against the in-memory fake (§4, §15, §16): record
//! before send, confirm on success, failure streaks, and every recovery
//! branch after a simulated stop, with the store reopened like after a real
//! crash.

use orch_config::tunables::Tunables;
use orch_core::store::{DeliveryKind, NewTask, Store};
use orch_herdr::delivery::{
    Deliverer, NewDelivery, Recovery, SendOutcome, DEFAULT_CONFIRM_GRACE_S,
    MAX_CONSECUTIVE_FAILURES, confirm_grace,
};
use orch_herdr::model::{AgentInfo, PaneInfo};
use orch_herdr::status::AgentStatus;
use orch_herdr::{CallKind, FakeHerdr, HerdrError, RuntimeAdapter, Sent};

const GRACE: i64 = DEFAULT_CONFIRM_GRACE_S;

fn watchable(agent: &str) -> bool {
    agent != "hermes-1"
}

fn tempdir() -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "orch-herdr-delivery-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn setup() -> (std::path::PathBuf, Store, FakeHerdr) {
    let dir = tempdir();
    let store = Store::open_in(&dir.join(".orchestrate")).unwrap();
    (dir, store, FakeHerdr::new())
}

fn agent(rt: &FakeHerdr, name: &str, pane: &str, status: AgentStatus) {
    rt.add_pane(PaneInfo::new(pane, "w1", "w1:t1"));
    rt.add_agent(AgentInfo::new(name, "claude", pane, status));
}

fn req<'a>(agent: &'a str, task: &'a str) -> NewDelivery<'a> {
    NewDelivery {
        agent,
        task,
        attempt: 1,
        kind: DeliveryKind::Coder,
        prompt: "do it",
    }
}

fn add_task(store: &mut Store, id: &str) {
    store
        .add_task(&NewTask {
            id: id.into(),
            title: format!("task {id}"),
            deps: vec![],
        })
        .unwrap();
}

fn one_prompt(agent: &str, text: &str) -> Vec<Sent> {
    vec![Sent::Prompt {
        agent: agent.into(),
        text: text.into(),
    }]
}

#[test]
fn send_records_then_confirms_on_success() {
    let (_dir, mut store, rt) = setup();
    add_task(&mut store, "M1");
    agent(&rt, "claude-1", "w1:p1", AgentStatus::Idle);
    let watch = |a: &str| watchable(a);
    let mut d = Deliverer::new(&mut store, &rt, &watch);
    let SendOutcome::Sent { delivery } = d.send(req("claude-1", "M1")).unwrap()
    else {
        panic!()
    };
    assert_eq!(delivery.agent, "claude-1");
    assert_eq!(delivery.task, "M1");
    assert_eq!(delivery.attempt, 1);
    assert_eq!(delivery.kind, DeliveryKind::Coder);
    assert_eq!(delivery.prompt, "do it");
    assert!(delivery.confirmed && delivery.confirmed_at.is_some());
    assert!(delivery.recorded_at > 0);
    assert_eq!(d.failure_streak("claude-1").unwrap(), 0);
    assert_eq!(rt.sent(), one_prompt("claude-1", "do it"));
}

// §16/AC10: the record is committed before anything is sent — a refused send
// still leaves the delivery behind, so the job is never lost.
#[test]
fn a_failed_send_leaves_the_record_and_keeps_the_error() {
    let (_dir, mut store, rt) = setup();
    add_task(&mut store, "M1");
    agent(&rt, "claude-1", "w1:p1", AgentStatus::Idle);
    rt.fail_next(
        CallKind::AgentPrompt,
        1,
        HerdrError::Unavailable {
            message: "herdr: command not found".into(),
        },
    );
    let watch = |a: &str| watchable(a);
    let mut d = Deliverer::new(&mut store, &rt, &watch);
    let SendOutcome::Failed { delivery, error } = d.send(req("claude-1", "M1")).unwrap()
    else {
        panic!()
    };
    assert!(!delivery.confirmed);
    assert_eq!(delivery.error_kind.as_deref(), Some("unavailable"));
    assert!(
        delivery
            .error_message
            .as_deref()
            .unwrap()
            .contains("command not found"),
        "{delivery:?}"
    );
    assert!(matches!(error, HerdrError::Unavailable { .. }));
    // The runtime saw nothing.
    assert!(rt.sent().is_empty());
    // §15: three in a row is where Phase 6 benches.
    assert_eq!(MAX_CONSECUTIVE_FAILURES, 3);
    rt.fail_next(
        CallKind::AgentPrompt,
        2,
        HerdrError::timed_out(30, "agent prompt timed out"),
    );
    let id = delivery.id;
    assert!(matches!(d.resend(id).unwrap(), SendOutcome::Failed { .. }));
    assert!(matches!(d.resend(id).unwrap(), SendOutcome::Failed { .. }));
    assert_eq!(d.failure_streak("claude-1").unwrap(), 3);
    // A confirmed send resets the count.
    let SendOutcome::Sent { delivery } = d.resend(id).unwrap() else { panic!() };
    assert!(delivery.confirmed && delivery.error_kind.is_none());
    assert_eq!(d.failure_streak("claude-1").unwrap(), 0);
    assert_eq!(rt.sent().len(), 1);
}

// §15/AC17: a re-send goes to the agent the delivery was recorded for, with
// the same prompt, task and attempt; no API exists to pick another agent,
// and a confirmed delivery cannot be re-sent at all.
#[test]
fn resend_goes_to_the_recorded_agent_only() {
    let (_dir, mut store, rt) = setup();
    add_task(&mut store, "M1");
    agent(&rt, "claude-1", "w1:p1", AgentStatus::Idle);
    agent(&rt, "codex-2", "w1:p2", AgentStatus::Idle);
    let watch = |a: &str| watchable(a);
    let mut d = Deliverer::new(&mut store, &rt, &watch);
    let SendOutcome::Sent { delivery } = d.send(req("codex-2", "M1")).unwrap() else {
        panic!()
    };
    assert_eq!(delivery.agent, "codex-2");
    assert_eq!(rt.sent(), one_prompt("codex-2", "do it"));
    let e = d.resend(delivery.id).unwrap_err();
    assert!(e.to_string().contains("already confirmed"), "{e}");
}

// AC19: a stop after record, before send. The store is reopened like after a
// real crash; recovery finds the delivery unconfirmed and re-sendable, and
// the re-send delivers the very same prompt.
#[test]
fn crash_after_record_before_send_is_recovered_and_resent() {
    let (dir, mut store, rt) = setup();
    add_task(&mut store, "M1");
    agent(&rt, "claude-1", "w1:p1", AgentStatus::Idle);
    let id = store
        .record_delivery("claude-1", "M1", 1, DeliveryKind::Coder, "do it")
        .unwrap();
    let recorded_at = store.delivery(id).unwrap().recorded_at;
    // "crash": reopen the store from disk.
    let path = dir.join(".orchestrate/state.db");
    drop(store);
    let mut store = Store::open(&path).unwrap();
    let watch = |a: &str| watchable(a);
    let mut d = Deliverer::new(&mut store, &rt, &watch);
    // Inside the grace window nothing is decided yet.
    assert!(d.recover(recorded_at, GRACE).unwrap().is_empty());
    let out = d.recover(recorded_at + GRACE + 1, GRACE).unwrap();
    let Recovery::Unconfirmed { delivery } = &out[0] else { panic!("{out:?}") };
    assert_eq!(delivery.id, id);
    assert_eq!(delivery.agent, "claude-1");
    assert_eq!(delivery.prompt, "do it");
    assert!(!delivery.confirmed);
    assert!(rt.sent().is_empty());
    let SendOutcome::Sent { delivery } = d.resend(id).unwrap() else { panic!() };
    assert!(delivery.confirmed);
    assert_eq!(rt.sent(), one_prompt("claude-1", "do it"));
}

// AC20/AC13: a stop after send, before confirm. Recovery confirms from the
// agent's working/blocked status without sending anything again, and never
// confirms twice (AC21).
#[test]
fn crash_after_send_before_confirm_is_confirmed_without_resending() {
    for status in [AgentStatus::Working, AgentStatus::Blocked] {
        let (dir, mut store, rt) = setup();
        add_task(&mut store, "M1");
        agent(&rt, "claude-1", "w1:p1", AgentStatus::Idle);
        let id = store
            .record_delivery("claude-1", "M1", 1, DeliveryKind::Coder, "do it")
            .unwrap();
        let recorded_at = store.delivery(id).unwrap().recorded_at;
        // The send went out, the confirm never happened.
        rt.send_prompt("claude-1", "do it").unwrap();
        rt.set_status("claude-1", status);
        let path = dir.join(".orchestrate/state.db");
        drop(store);
        let mut store = Store::open(&path).unwrap();
        let watch = |a: &str| watchable(a);
        let mut d = Deliverer::new(&mut store, &rt, &watch);
        let out = d.recover(recorded_at + GRACE + 1, GRACE).unwrap();
        let Recovery::Confirmed { delivery } = &out[0] else { panic!("{out:?}") };
        assert!(delivery.confirmed);
        assert_eq!(rt.sent().len(), 1, "recovery must not re-send");
        // Running recovery again confirms nothing twice.
        assert!(
            d.recover(recorded_at + GRACE + 1, GRACE)
                .unwrap()
                .is_empty()
        );
        assert!(store.unconfirmed_deliveries().unwrap().is_empty());
    }
}

// AC14: herdr no longer knows the agent — reported gone, not re-sent.
#[test]
fn recovery_reports_a_gone_agent_without_resending() {
    let (_dir, mut store, rt) = setup();
    add_task(&mut store, "M1");
    agent(&rt, "claude-1", "w1:p1", AgentStatus::Working);
    let id = store
        .record_delivery("claude-1", "M1", 1, DeliveryKind::Coder, "do it")
        .unwrap();
    let recorded_at = store.delivery(id).unwrap().recorded_at;
    rt.remove_agent("claude-1");
    let watch = |a: &str| watchable(a);
    let mut d = Deliverer::new(&mut store, &rt, &watch);
    let out = d.recover(recorded_at + GRACE + 1, GRACE).unwrap();
    let Recovery::Gone { delivery } = &out[0] else { panic!("{out:?}") };
    assert_eq!(delivery.id, id);
    assert!(!delivery.confirmed);
    assert!(rt.sent().is_empty());
}

// AC15: idle/done/unknown (and a runtime error) stay unconfirmed and can be
// re-sent on request.
#[test]
fn recovery_leaves_the_rest_unconfirmed_but_resendable() {
    for status in [AgentStatus::Idle, AgentStatus::Done, AgentStatus::Unknown] {
        let (_dir, mut store, rt) = setup();
        add_task(&mut store, "M1");
        agent(&rt, "claude-1", "w1:p1", AgentStatus::Working);
        let id = store
            .record_delivery("claude-1", "M1", 1, DeliveryKind::Coder, "do it")
            .unwrap();
        let recorded_at = store.delivery(id).unwrap().recorded_at;
        rt.set_status("claude-1", status);
        let watch = |a: &str| watchable(a);
        let mut d = Deliverer::new(&mut store, &rt, &watch);
        let out = d.recover(recorded_at + GRACE + 1, GRACE).unwrap();
        assert!(
            matches!(&out[0], Recovery::Unconfirmed { .. }),
            "{status:?}: {out:?}"
        );
        assert!(rt.sent().is_empty());
        d.resend(id).unwrap();
        assert_eq!(rt.sent().len(), 1);
    }
    // A runtime error while asking the status is not "gone".
    let (_dir, mut store, rt) = setup();
    add_task(&mut store, "M1");
    let id = store
        .record_delivery("claude-1", "M1", 1, DeliveryKind::Coder, "do it")
        .unwrap();
    let recorded_at = store.delivery(id).unwrap().recorded_at;
    rt.fail_next(
        CallKind::AgentGet,
        1,
        HerdrError::timed_out(30, "herdr hung"),
    );
    let watch = |a: &str| watchable(a);
    let mut d = Deliverer::new(&mut store, &rt, &watch);
    let out = d.recover(recorded_at + GRACE + 1, GRACE).unwrap();
    assert!(matches!(&out[0], Recovery::Unconfirmed { .. }), "{out:?}");
}

// AC16: an unwatchable agent is never judged by herdr and never re-sent or
// confirmed automatically — even when the runtime would answer. A re-send to
// it still works on request (the caller types into its pane, §4).
#[test]
fn unwatchable_agents_are_never_judged_or_resent_automatically() {
    let (_dir, mut store, rt) = setup();
    add_task(&mut store, "M1");
    agent(&rt, "hermes-1", "w1:p1", AgentStatus::Unwatchable);
    // If recovery asked the runtime anyway, these failures would surface.
    rt.fail_next(
        CallKind::AgentGet,
        5,
        HerdrError::other("the adapter must not be asked"),
    );
    let watch = |a: &str| watchable(a);
    let id = store
        .record_delivery("hermes-1", "M1", 2, DeliveryKind::Coder, "again")
        .unwrap();
    let recorded_at = store.delivery(id).unwrap().recorded_at;
    {
        let mut d = Deliverer::new(&mut store, &rt, &watch);
        let out = d.send(req("hermes-1", "M1")).unwrap();
        let SendOutcome::Sent { delivery } = out else { panic!("{out:?}") };
        assert!(delivery.confirmed);
        let out = d.recover(recorded_at + GRACE + 1, GRACE).unwrap();
        assert!(matches!(&out[0], Recovery::Unconfirmed { .. }), "{out:?}");
        let SendOutcome::Sent { delivery } = d.resend(id).unwrap() else { panic!() };
        assert!(delivery.confirmed);
    }
    // The only sends were the two explicit ones; nothing automatic.
    assert_eq!(rt.sent().len(), 2);
    assert!(store.unconfirmed_deliveries().unwrap().is_empty());
}

// The grace comes from ORCH_CONFIRM_S but never below the documented constant.
#[test]
fn confirm_grace_follows_the_tunable_with_a_floor() {
    let t = Tunables::from_lookup(|_| None).unwrap();
    assert_eq!(t.confirm_s, 30);
    assert_eq!(confirm_grace(&t), 30);
    let t =
        Tunables::from_lookup(|name| (name == "ORCH_CONFIRM_S").then(|| "5".to_string())).unwrap();
    assert_eq!(confirm_grace(&t), DEFAULT_CONFIRM_GRACE_S);
    let t = Tunables::from_lookup(|name| (name == "ORCH_CONFIRM_S").then(|| "120".to_string()))
        .unwrap();
    assert_eq!(confirm_grace(&t), 120);
}

// A prompt the runtime refused (agent blocked) never arrived: a blocked status
// must not turn it into a confirmed delivery during recovery.
#[test]
fn a_refused_prompt_is_not_confirmed_by_a_blocked_status() {
    let (_dir, mut store, rt) = setup();
    add_task(&mut store, "M1");
    agent(&rt, "claude-1", "w1:p1", AgentStatus::Blocked);
    rt.fail_next(
        CallKind::AgentPrompt,
        1,
        HerdrError::AgentBlocked { agent: "claude-1".into(), message: "approval dialog".into() },
    );
    let watch = |a: &str| watchable(a);
    let mut d = Deliverer::new(&mut store, &rt, &watch);
    assert!(matches!(d.send(req("claude-1", "M1")).unwrap(), SendOutcome::Failed { .. }));
    let r = d.recover(i64::MAX / 2, 30).unwrap();
    assert!(matches!(r.as_slice(), [Recovery::Unconfirmed { .. }]), "{r:?}");
    assert!(rt.sent().is_empty());
}
