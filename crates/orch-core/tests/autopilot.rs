//! The event manager, the recovery reconciliation and the bounded poll against a real
//! store, with fakes for everything outside orch-core (AC10): the runtime view, the
//! deliveries, the milestones' work, the lead, the clock, the notifier and the poll.

use orch_core::events::{
    AgentState, HerdrPoll, PollOutcome, PollSource, PollTunables, RunEvent, RuntimeView,
};
use orch_core::lead_queue::LeadKind;
use orch_core::manager::{
    CheckOutcome, CommitOutcome, Deliveries, DeliveryOutcome, LoopTunables, Manager, NewDelivery,
    Recovered, ReviewOutcome, Work,
};
use orch_core::reconcile::{park_with_lost, reconcile};
use orch_core::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::store::{Counter, DeliveryKind, Store, StoreError, TransitionRequest};
use orch_core::task::State;
use orch_core::wake::{Clock, LeadPane, LeadRuntime, Notifier};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

// ---- fakes

struct FkClock(Rc<Cell<i64>>);

impl Clock for FkClock {
    fn now(&self) -> i64 {
        self.0.get()
    }
}

struct Notes(RefCell<Vec<String>>);

impl Notifier for Notes {
    fn notify(&self, m: &str) {
        self.0.borrow_mut().push(m.to_string());
    }
}

struct Runtime {
    st: RefCell<HashMap<String, AgentState>>,
    out: RefCell<HashMap<String, String>>,
}

impl Runtime {
    fn set(&self, agent: &str, s: AgentState) {
        self.st.borrow_mut().insert(agent.into(), s);
    }
    fn set_output(&self, agent: &str, text: &str) {
        self.out.borrow_mut().insert(agent.into(), text.into());
    }
}

impl RuntimeView for Runtime {
    fn status(&self, agent: &str) -> Option<AgentState> {
        self.st.borrow().get(agent).copied()
    }

    fn output(&self, agent: &str, _lines: u32) -> Option<String> {
        self.out.borrow().get(agent).cloned()
    }
}

thread_local! {
    /// What the fake Deliveries' recovery "sees" about each agent, set per test.
    static RECOVER_STATUS: RefCell<Option<AgentState>> = const { RefCell::new(None) };
}

/// The delivery mechanics, mirroring the real ones (record before send, confirm
/// after, error kept, failure streak) without a runtime.
struct Deliver {
    /// The next `fail_sends` sends and re-sends are refused.
    fail_sends: Cell<u32>,
    resends: Cell<u32>,
}

impl Deliveries for Deliver {
    fn send(
        &mut self,
        store: &mut Store,
        req: NewDelivery<'_>,
    ) -> Result<DeliveryOutcome, StoreError> {
        let id = store.record_delivery(req.agent, req.task, req.attempt, req.kind, req.prompt)?;
        self.outcome(store, id, req.agent)
    }

    fn resend(&mut self, store: &mut Store, id: i64) -> Result<DeliveryOutcome, StoreError> {
        self.resends.set(self.resends.get() + 1);
        let d = store.delivery(id)?;
        self.outcome(store, id, &d.agent)
    }

    fn failure_streak(&self, store: &Store, agent: &str) -> u32 {
        store.delivery_fails(agent).unwrap_or(0)
    }

    fn recover(
        &mut self,
        store: &mut Store,
        now: i64,
        grace_s: i64,
    ) -> Result<Vec<Recovered>, StoreError> {
        let mut out = Vec::new();
        for d in store.unconfirmed_deliveries()? {
            if now - d.recorded_at < grace_s {
                continue;
            }
            match RECOVER_STATUS.with(|c| *c.borrow()) {
                Some(AgentState::Working | AgentState::Blocked) => {
                    store.confirm_delivery(d.id)?;
                    out.push(Recovered::Confirmed { delivery: store.delivery(d.id)? });
                }
                Some(AgentState::Gone) => out.push(Recovered::Gone { delivery: d }),
                _ => out.push(Recovered::Unconfirmed { delivery: d }),
            }
        }
        Ok(out)
    }
}

impl Deliver {
    fn outcome(
        &self,
        store: &mut Store,
        id: i64,
        agent: &str,
    ) -> Result<DeliveryOutcome, StoreError> {
        if self.fail_sends.get() > 0 {
            self.fail_sends.set(self.fail_sends.get() - 1);
            store.record_delivery_error(id, "unavailable", "herdr down")?;
            store.bump_delivery_fails(agent)?;
            return Ok(DeliveryOutcome::Failed {
                delivery: store.delivery(id)?,
                error: "herdr down".into(),
            });
        }
        if !store.delivery(id)?.confirmed {
            store.confirm_delivery(id)?;
        }
        store.reset_delivery_fails(agent)?;
        Ok(DeliveryOutcome::Sent { delivery: store.delivery(id)? })
    }
}

/// The milestones' work (Phase 7/8's seam), scripted per test.
struct FkWork {
    check: std::result::Result<CheckOutcome, String>,
    review: ReviewOutcome,
    fix_rounds: Cell<u32>,
    save_calls: Cell<u32>,
}

impl Work for FkWork {
    fn prepare(&mut self, _task: &str, _worker: &str) -> std::result::Result<(), String> {
        Ok(())
    }

    fn write_task(&mut self, _task: &str) -> std::result::Result<(), String> {
        Ok(())
    }

    fn fix_body(
        &mut self,
        task: &str,
        round: u32,
        _tail: &str,
    ) -> std::result::Result<String, String> {
        self.fix_rounds.set(self.fix_rounds.get() + 1);
        Ok(format!("# fix {task} round {round}"))
    }

    fn check(&mut self, _task: &str) -> std::result::Result<CheckOutcome, String> {
        self.check.clone()
    }

    fn save_work(&mut self, _task: &str) -> std::result::Result<(), String> {
        self.save_calls.set(self.save_calls.get() + 1);
        Ok(())
    }

    fn review_body(&mut self, task: &str, round: u32) -> std::result::Result<String, String> {
        Ok(format!("# review {task} round {round}"))
    }

    fn review_verdict(
        &mut self,
        _task: &str,
        _round: u32,
    ) -> std::result::Result<ReviewOutcome, String> {
        Ok(self.review.clone())
    }

    fn commit(&mut self, _task: &str) -> std::result::Result<CommitOutcome, String> {
        Ok(CommitOutcome::Merged)
    }
}

struct FkPoll {
    batches: RefCell<Vec<Vec<RunEvent>>>,
}

impl PollSource for FkPoll {
    fn poll(&mut self, _store: &mut Store, _secs: u64) -> PollOutcome {
        let mut b = self.batches.borrow_mut();
        if b.is_empty() {
            PollOutcome::Waiting { nothing_running: true }
        } else {
            PollOutcome::Events(b.remove(0))
        }
    }
}

struct FkLead;

impl LeadRuntime for FkLead {
    fn pane(&self, _lead: &str) -> LeadPane {
        LeadPane::Idle
    }

    fn send(&self, _lead: &str, _text: &str) -> Result<(), String> {
        Ok(())
    }
}

// ---- rig

fn tun() -> LoopTunables {
    LoopTunables {
        idle_grace_s: 120,
        confirm_s: 30,
        coder_budget_s: 2700,
        review_budget_s: 900,
        send_retry_s: 120,
        unreachable_min: 30,
        cooldown_min: 60,
        gone_cooldown_min: 10,
        max_lost: 4,
        lead_wait_s: 900,
        lead_idle_max_s: 43200,
        lead_confirm_s: 20,
        lead_confirm_wakes: 2,
        lead_rewake_s: 300,
        lead_max_wakes: 3,
    }
}

fn watch_all(_: &str) -> bool {
    true
}

fn pusher(log: &Rc<RefCell<Vec<String>>>) -> impl FnMut(&str) + '_ {
    |m: &str| log.borrow_mut().push(m.to_string())
}

fn pusher2(log: &Rc<RefCell<Vec<String>>>) -> impl FnMut(&str, bool) + '_ {
    |m: &str, _: bool| log.borrow_mut().push(m.to_string())
}

fn joined(log: &Rc<RefCell<Vec<String>>>) -> String {
    log.borrow().join("\n")
}

fn settings(lead: bool) -> RunSettings {
    RunSettings {
        branch: "feature/x".into(),
        base: None,
        title: "X".into(),
        commits: "per-milestone".into(),
        isolation: "worktree".into(),
        max_parallel: 2,
        setup_inputs: vec![],
        scope: "warn".into(),
        risk_hold: true,
        budget_s: None,
        check_command: "cargo test".into(),
        setup_command: "none".into(),
        lead_mode: if lead { "events".into() } else { "off".into() },
        reviewer: Some("rev".into()),
    }
}

fn plan_task(id: &str, tier: Option<&str>) -> PlanTask {
    PlanTask {
        id: id.into(),
        title: format!("task {id}"),
        deps: vec![],
        enabled: true,
        open_questions: false,
        tier: tier.map(String::from),
        needs: vec![],
        model_why: String::new(),
        prefer: "any".into(),
        review: false,
        files: vec![format!("{id}.rs")],
        parallel: true,
    }
}

fn agents(names: &[&str], lead: bool) -> Vec<RunAgent> {
    let mut out: Vec<RunAgent> = names
        .iter()
        .map(|n| RunAgent {
            role: AgentRole::Worker,
            name: (*n).into(),
            kind: "claude".into(),
            model: None,
            tier: Some("medium".into()),
            detect: true,
            pane: None,
        })
        .collect();
    if lead {
        out.push(RunAgent {
            role: AgentRole::Lead,
            name: "lead".into(),
            kind: "claude".into(),
            model: None,
            tier: None,
            detect: true,
            pane: None,
        });
    }
    out.push(RunAgent {
        role: AgentRole::Reviewer,
        name: "rev".into(),
        kind: "claude".into(),
        model: None,
        tier: None,
        detect: true,
        pane: None,
    });
    out
}

fn sync(s: &mut Store, lead: bool, tasks: &[PlanTask]) {
    s.apply_sync(&SyncInput {
        settings: settings(lead),
        agents: agents(&["w1", "w2"], lead),
        tasks: tasks.to_vec(),
    })
    .unwrap();
}

/// One run with one new milestone, lead off, everything scripted to pass.
struct One {
    dir: tempfile::TempDir,
    store: Store,
    clock: FkClock,
    runtime: Runtime,
    deliver: Deliver,
    poll: FkPoll,
    work: FkWork,
    notes: Notes,
    say_log: Rc<RefCell<Vec<String>>>,
}

impl One {
    fn new() -> One {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
        sync(&mut store, false, &[plan_task("M1", Some("medium"))]);
        One {
            dir,
            store,
            clock: FkClock(Rc::new(Cell::new(1_000_000))),
            runtime: Runtime { st: RefCell::new(HashMap::new()), out: RefCell::new(HashMap::new()) },
            deliver: Deliver { fail_sends: Cell::new(0), resends: Cell::new(0) },
            poll: FkPoll { batches: RefCell::new(vec![]) },
            work: FkWork {
                check: Ok(CheckOutcome::Pass),
                review: ReviewOutcome::Approved,
                fix_rounds: Cell::new(0),
                save_calls: Cell::new(0),
            },
            notes: Notes(RefCell::new(vec![])),
            say_log: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Run one closure with a manager over the rig's pieces; `t` is the shared
    /// time handle so tests can advance the clock while the manager is alive.
    fn with_manager<R>(&mut self, f: impl FnOnce(&mut Manager<'_>, &Rc<Cell<i64>>) -> R) -> R {
        let One { dir, store, clock, runtime, deliver, poll, work, notes, say_log, .. } = self;
        let t = clock.0.clone();
        let mut say = pusher2(say_log);
        let mut m = Manager::new(
            dir.path().to_path_buf(),
            store,
            runtime,
            &watch_all,
            poll,
            work,
            &FkLead,
            notes,
            clock,
            deliver,
            tun(),
            &mut say,
        )
        .unwrap();
        f(&mut m, &t)
    }

    fn cycle(&mut self) -> orch_core::manager::Cycle {
        self.with_manager(|m, _| m.cycle())
    }
}

// ---- AC2: event validation, transition, dispatch

#[test]
fn an_event_is_applied_then_the_scheduler_claims_and_sends() {
    let mut one = One::new();
    assert_eq!(one.cycle(), orch_core::manager::Cycle::Continue);
    let t = one.store.get_task("M1").unwrap();
    assert_eq!(t.status, State::Coding);
    assert_eq!(t.worker.as_deref(), Some("w1"));
    assert_eq!(t.attempt, 1);
    // the prompt was recorded and confirmed (AC3)
    let d = one.store.latest_delivery("M1", 1, DeliveryKind::Coder).unwrap().unwrap();
    assert!(d.confirmed);
    assert!(d.prompt.contains(".orch-task/M1/task.md"));
    assert!(d.prompt.contains("result-1.json"));
    assert!(joined(&one.say_log).contains("M1 -> w1"));

    // a done file event moves it on through the validated transition
    one.with_manager(|m, _| {
        m.apply_event(&RunEvent::Done { task: "M1".into(), agent: "w1".into() }).unwrap()
    });
    assert_eq!(one.store.get_task("M1").unwrap().status, State::Verifying);
}

#[test]
fn a_replayed_event_changes_nothing_and_is_logged_as_a_duplicate() {
    let mut one = One::new();
    one.cycle();
    let ev = RunEvent::Done { task: "M1".into(), agent: "w1".into() };
    one.with_manager(|m, _| m.apply_event(&ev).unwrap());
    assert_eq!(one.store.get_task("M1").unwrap().status, State::Verifying);
    let n = one.say_log.borrow().len();
    // replay: same idempotency key, no state change, logged
    one.with_manager(|m, _| m.apply_event(&ev).unwrap());
    assert_eq!(one.store.get_task("M1").unwrap().status, State::Verifying);
    let text = joined(&one.say_log);
    assert!(
        text.len() > n && text[n..].contains("duplicate"),
        "the replay was not logged as a duplicate: {text}"
    );
    // an unknown milestone is dropped, not an error
    one.with_manager(|m, _| {
        m.apply_event(&RunEvent::Done { task: "M9".into(), agent: "w1".into() }).unwrap()
    });
    assert!(joined(&one.say_log).contains("unknown milestone M9"));
}

#[test]
fn a_final_milestone_ignores_further_events() {
    let mut one = One::new();
    one.cycle();
    let _ = one.store
        .transition("M1", &TransitionRequest::auto(State::Verifying, None));
    let _ = one.store
        .transition("M1", &TransitionRequest::auto(State::Merged, None));
    // a DONE event for a merged milestone: stale event, logged, nothing changes
    one.with_manager(|m, _| {
        m.apply_event(&RunEvent::Done { task: "M1".into(), agent: "w1".into() }).unwrap()
    });
    let text = joined(&one.say_log);
    assert!(text.contains("duplicate or illegal") && text.contains("merged is final"), "{text}");
    assert_eq!(one.store.get_task("M1").unwrap().status, State::Merged);
}

// ---- AC3: crash-safe delivery, record before send, confirm after

#[test]
fn a_failed_send_leaves_the_delivery_unconfirmed_with_the_error_kept() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    sync(&mut store, false, &[plan_task("M1", None)]);
    let mut del = Deliver { fail_sends: Cell::new(1), resends: Cell::new(0) };
    let outcome = del
        .send(
            &mut store,
            NewDelivery {
                agent: "w1",
                task: "M1",
                attempt: 1,
                kind: DeliveryKind::Coder,
                prompt: "work M1",
            },
        )
        .unwrap();
    let d = match outcome {
        DeliveryOutcome::Failed { delivery, .. } => delivery,
        _ => panic!("expected a failed send"),
    };
    assert!(!d.confirmed);
    assert_eq!(d.agent, "w1");
    assert_eq!(d.error_kind.as_deref(), Some("unavailable"));
    assert_eq!(store.delivery_fails("w1").unwrap(), 1);
}

#[test]
fn recovery_confirms_from_a_working_agent_and_resends_to_an_idle_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    sync(&mut store, false, &[plan_task("M1", None)]);
    store.claim("M1", "w1", None).unwrap();
    let mut del = Deliver { fail_sends: Cell::new(0), resends: Cell::new(0) };
    let id = match del
        .send(
            &mut store,
            NewDelivery { agent: "w1", task: "M1", attempt: 1, kind: DeliveryKind::Coder, prompt: "p" },
        )
        .unwrap()
    {
        DeliveryOutcome::Sent { delivery } => delivery.id,
        _ => panic!(),
    };
    // crash: un-confirm the delivery (the send went out but the confirm was lost)
    unconfirm(dir.path(), id);
    RECOVER_STATUS.with(|c| *c.borrow_mut() = Some(AgentState::Working));
    let late = now() + 3600; // the delivery is fresh only for ORCH_CONFIRM_S
    let say_log = Rc::new(RefCell::new(Vec::new()));
    let runtime = Runtime { st: RefCell::new(HashMap::new()), out: RefCell::new(HashMap::new()) };
    let mut work = FkWork {
        check: Ok(CheckOutcome::Pass),
        review: ReviewOutcome::Approved,
        fix_rounds: Cell::new(0),
        save_calls: Cell::new(0),
    };
    let mut say = pusher(&say_log);
    let report = reconcile(
        &mut store,
        &runtime,
        &mut del,
        &watch_all,
        &mut work,
        &tun(),
        late,
        &mut say,
    )
    .unwrap();
    assert!(report.notes.iter().any(|n| n.contains("confirmed from the agent's status")), "{report:?}");
    assert!(store.delivery(id).unwrap().confirmed);

    // idle agent: the same delivery goes out again, to the same agent
    unconfirm(dir.path(), id);
    RECOVER_STATUS.with(|c| *c.borrow_mut() = Some(AgentState::Idle));
    let mut say = pusher(&say_log);
    let report = reconcile(
        &mut store,
        &runtime,
        &mut del,
        &watch_all,
        &mut work,
        &tun(),
        late,
        &mut say,
    )
    .unwrap();
    assert!(report.notes.iter().any(|n| n.contains("re-sent the unconfirmed prompt")), "{report:?}");
    assert!(store.delivery(id).unwrap().confirmed);
    assert_eq!(del.resends.get(), 1);
    let d = store.latest_delivery("M1", 1, DeliveryKind::Coder).unwrap().unwrap();
    assert_eq!(d.agent, "w1");
}

// ---- AC4: a prompt that cannot reach its agent

#[test]
fn a_failed_prompt_requeues_the_task_and_benches_the_agent_for_the_retry() {
    let mut one = One::new();
    one.deliver.fail_sends.set(1);
    assert_eq!(one.cycle(), orch_core::manager::Cycle::Continue);
    // the task went back to the queue with a lost attempt
    let t = one.store.get_task("M1").unwrap();
    assert_eq!(t.status, State::Pending);
    assert_eq!(t.lost, 1);
    // the agent is benched for ORCH_SEND_RETRY_S
    let snap = one.store.scheduler_snapshot().unwrap().unwrap();
    let bench = snap.benches.iter().find(|b| b.agent == "w1").unwrap();
    assert_eq!(bench.reason, "unreachable");
    assert_eq!(bench.until, one.clock.0.get() + 120);
    // a successful send resets the failure streak (AC3)
    assert_eq!(one.deliver.failure_streak(&one.store, "w1"), 1);
}

#[test]
fn three_consecutive_failed_sends_bench_the_agent_and_tell_the_lead() {
    let mut one = One::new();
    one.deliver.fail_sends.set(3);
    one.cycle(); // t = 1000000
    one.clock.0.set(1_000_121);
    one.cycle();
    one.clock.0.set(1_000_242);
    one.cycle(); // the third failure in a row
    let snap = one.store.scheduler_snapshot().unwrap().unwrap();
    let bench = snap.benches.iter().find(|b| b.agent == "w1").unwrap();
    assert_eq!(bench.reason, "unreachable");
    assert_eq!(bench.until, 1_000_242 + 30 * 60);
    assert!(joined(&one.say_log).contains("BENCHED"), "{}", joined(&one.say_log));
}

// ---- AC5: lost attempts, rate limits, idle nudges, gone agents

#[test]
fn a_milestone_fails_at_max_lost_with_its_history() {
    let mut one = One::new();
    one.deliver.fail_sends.set(u32::MAX);
    for _ in 0..4 {
        one.cycle();
        let n = one.clock.0.get();
        one.clock.0.set(n + 121);
    }
    let t = one.store.get_task("M1").unwrap();
    assert_eq!(t.status, State::Failed);
    assert!(t.reason.contains("lost 4 attempts"), "{}", t.reason);
}

#[test]
fn a_rate_limited_agent_is_benched_with_the_python_cooldown() {
    let mut one = One::new();
    one.cycle(); // coding, prompt sent
    let now = one.clock.0.get();
    one.runtime.set("w1", AgentState::Idle);
    one.runtime.set_output("w1", "Claude usage limit reached. Try again at 14:00");
    one.with_manager(|m, _| {
        m.apply_event(&RunEvent::Idle { task: "M1".into(), agent: "w1".into(), after_s: 200 })
            .unwrap()
    });
    let t = one.store.get_task("M1").unwrap();
    assert_eq!(t.status, State::Pending);
    let snap = one.store.scheduler_snapshot().unwrap().unwrap();
    let bench = snap.benches.iter().find(|b| b.agent == "w1").unwrap();
    assert_eq!(bench.reason, "rate-limit");
    assert_eq!(bench.until, now + 60 * 60);
}

#[test]
fn an_idle_agent_is_nudged_once_then_parked() {
    let mut one = One::new();
    one.cycle(); // coding, prompt sent
    one.runtime.set("w1", AgentState::Idle);
    one.with_manager(|m, _| {
        m.apply_event(&RunEvent::Idle { task: "M1".into(), agent: "w1".into(), after_s: 200 })
            .unwrap();
        // nudged once: the same delivery went out again (to the same agent, AC3)
        assert_eq!(m.store.get_task("M1").unwrap().status, State::Coding);
        // a second idle without finishing parks it
        m.apply_event(&RunEvent::Idle { task: "M1".into(), agent: "w1".into(), after_s: 400 })
            .unwrap();
    });
    assert_eq!(one.deliver.resends.get(), 1);
    let t = one.store.get_task("M1").unwrap();
    assert_eq!(t.status, State::Paused);
    assert!(t.reason.contains("stopped twice without finishing"), "{}", t.reason);
}

#[test]
fn a_gone_coder_benches_and_the_work_moves_on() {
    let mut one = One::new();
    one.cycle(); // coding
    let now = one.clock.0.get();
    one.with_manager(|m, _| {
        m.apply_event(&RunEvent::Gone { task: "M1".into(), agent: "w1".into() }).unwrap()
    });
    let t = one.store.get_task("M1").unwrap();
    assert_eq!(t.status, State::Pending);
    assert_eq!(t.lost, 1);
    let snap = one.store.scheduler_snapshot().unwrap().unwrap();
    let bench = snap.benches.iter().find(|b| b.agent == "w1").unwrap();
    assert_eq!(bench.reason, "gone");
    assert_eq!(bench.until, now + 10 * 60);
}

// ---- AC6: lead mode

#[test]
fn the_lead_waits_then_the_milestone_is_auto_dispatched() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    sync(&mut store, true, &[plan_task("M1", Some("medium"))]);
    let mut one = One {
        dir,
        store,
        clock: FkClock(Rc::new(Cell::new(1_000_000))),
        runtime: Runtime { st: RefCell::new(HashMap::new()), out: RefCell::new(HashMap::new()) },
        deliver: Deliver { fail_sends: Cell::new(0), resends: Cell::new(0) },
        poll: FkPoll { batches: RefCell::new(vec![]) },
        work: FkWork {
            check: Ok(CheckOutcome::Pass),
            review: ReviewOutcome::Approved,
            fix_rounds: Cell::new(0),
            save_calls: Cell::new(0),
        },
        notes: Notes(RefCell::new(vec![])),
        say_log: Rc::new(RefCell::new(Vec::new())),
    };
    // cycle 1: the lead has not dispatched; the manager tells it and waits
    let c = one.with_manager(|m, t| {
        m.cycle();
        assert_eq!(m.store.get_task("M1").unwrap().status, State::Pending);
        // after ORCH_LEAD_WAIT_MIN it is started without the lead
        t.set(t.get() + 901);
        m.cycle(); // the wait is noticed, the go intent is set
        m.cycle(); // the scheduler now dispatches it
        m.cycle()
    });
    assert_eq!(c, orch_core::manager::Cycle::Continue);
    let entries = one.store.lead_entries().unwrap();
    assert!(entries.iter().any(|e| e.kind == LeadKind::Ready), "{entries:?}");
    assert!(entries.iter().any(|e| e.kind == LeadKind::AutoDispatched), "{entries:?}");
    let t = one.store.get_task("M1").unwrap();
    assert_eq!(t.status, State::Coding);
}

#[test]
fn without_a_lead_the_manager_dispatches_by_itself() {
    let mut one = One::new(); // lead off
    one.cycle();
    assert_eq!(one.store.get_task("M1").unwrap().status, State::Coding);
    assert!(one.store.undispatched().unwrap().is_empty());
}

#[test]
fn the_budget_used_up_stops_the_run_with_a_stopped_entry() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    let mut s = settings(false);
    s.budget_s = Some(60);
    store
        .apply_sync(&SyncInput {
            settings: s,
            agents: agents(&["w1"], false),
            tasks: vec![plan_task("M1", Some("medium"))],
        })
        .unwrap();
    let mut one = One {
        dir,
        store,
        clock: FkClock(Rc::new(Cell::new(1_000_000))),
        runtime: Runtime { st: RefCell::new(HashMap::new()), out: RefCell::new(HashMap::new()) },
        deliver: Deliver { fail_sends: Cell::new(0), resends: Cell::new(0) },
        poll: FkPoll { batches: RefCell::new(vec![]) },
        work: FkWork {
            check: Ok(CheckOutcome::Pass),
            review: ReviewOutcome::Approved,
            fix_rounds: Cell::new(0),
            save_calls: Cell::new(0),
        },
        notes: Notes(RefCell::new(vec![])),
        say_log: Rc::new(RefCell::new(Vec::new())),
    };
    let c = one.with_manager(|m, t| {
        m.cycle(); // 1 s of driving
        t.set(t.get() + 61);
        m.cycle()
    });
    assert_eq!(c, orch_core::manager::Cycle::Exit(3));
    assert!(
        joined(&one.say_log).contains("budget") && joined(&one.say_log).contains("used up"),
        "{}",
        joined(&one.say_log)
    );
}

// ---- AC7: exits

#[test]
fn when_everything_is_merged_the_run_queues_all_done_and_exits_zero() {
    let mut one = One::new();
    one.cycle(); // coding
    let _ = one.store
        .transition("M1", &TransitionRequest::auto(State::Verifying, None));
    let _ = one.store
        .transition("M1", &TransitionRequest::auto(State::Merged, None));
    assert_eq!(one.cycle(), orch_core::manager::Cycle::Exit(0));
    assert!(joined(&one.say_log).contains("ALL_DONE"), "{}", joined(&one.say_log));
}

// ---- AC8: reconciliation

#[test]
fn reconcile_parks_a_coding_task_whose_agent_vanished() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    sync(&mut store, false, &[plan_task("M1", None)]);
    // a task mid-coding whose agent herdr no longer knows
    store.claim("M1", "w1", None).unwrap();
    let runtime = Runtime { st: RefCell::new(HashMap::new()), out: RefCell::new(HashMap::new()) };
    runtime.set("w1", AgentState::Gone);
    let mut del = Deliver { fail_sends: Cell::new(0), resends: Cell::new(0) };
    let mut work = FkWork {
        check: Ok(CheckOutcome::Pass),
        review: ReviewOutcome::Approved,
        fix_rounds: Cell::new(0),
        save_calls: Cell::new(0),
    };
    let say_log = Rc::new(RefCell::new(Vec::new()));
    let mut say = pusher(&say_log);
    let report = reconcile(
        &mut store,
        &runtime,
        &mut del,
        &watch_all,
        &mut work,
        &tun(),
        2_000_000,
        &mut say,
    )
    .unwrap();
    let t = store.get_task("M1").unwrap();
    assert_eq!(t.status, State::Pending);
    assert_eq!(t.lost, 1);
    assert!(report.notes.iter().any(|n| n.contains("vanished from herdr")), "{report:?}");
    // the agent is benched for the gone cooldown
    let snap = store.scheduler_snapshot().unwrap().unwrap();
    let bench = snap.benches.iter().find(|b| b.agent == "w1").unwrap();
    assert_eq!(bench.reason, "gone");
}

#[test]
fn reconcile_clears_a_half_finished_wake_up() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    sync(&mut store, true, &[plan_task("M1", None)]);
    let mut st = store.wake_state().unwrap();
    st.awaiting = true;
    store.save_wake_state(&st).unwrap();
    let runtime = Runtime { st: RefCell::new(HashMap::new()), out: RefCell::new(HashMap::new()) };
    let mut del = Deliver { fail_sends: Cell::new(0), resends: Cell::new(0) };
    let mut work = FkWork {
        check: Ok(CheckOutcome::Pass),
        review: ReviewOutcome::Approved,
        fix_rounds: Cell::new(0),
        save_calls: Cell::new(0),
    };
    let say_log = Rc::new(RefCell::new(Vec::new()));
    let mut say = pusher(&say_log);
    reconcile(&mut store, &runtime, &mut del, &watch_all, &mut work, &tun(), 2_000_000, &mut say)
        .unwrap();
    assert!(!store.wake_state().unwrap().awaiting);
}

#[test]
fn reconcile_never_marks_incomplete_work_as_completed() {
    // a coding task with no worker at all goes back to the queue, not to merged
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    sync(&mut store, false, &[plan_task("M1", None)]);
    let _ = store
        .transition("M1", &TransitionRequest::auto(State::Coding, None))
        ;
    let runtime = Runtime { st: RefCell::new(HashMap::new()), out: RefCell::new(HashMap::new()) };
    let mut del = Deliver { fail_sends: Cell::new(0), resends: Cell::new(0) };
    let mut work = FkWork {
        check: Ok(CheckOutcome::Pass),
        review: ReviewOutcome::Approved,
        fix_rounds: Cell::new(0),
        save_calls: Cell::new(0),
    };
    let say_log = Rc::new(RefCell::new(Vec::new()));
    let mut say = pusher(&say_log);
    reconcile(&mut store, &runtime, &mut del, &watch_all, &mut work, &tun(), 2_000_000, &mut say)
        .unwrap();
    let t = store.get_task("M1").unwrap();
    assert_eq!(t.status, State::Pending);
}

#[test]
fn park_with_lost_fails_the_milestone_at_max_lost() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    sync(&mut store, false, &[plan_task("M1", None)]);
    let mut work = FkWork {
        check: Ok(CheckOutcome::Pass),
        review: ReviewOutcome::Approved,
        fix_rounds: Cell::new(0),
        save_calls: Cell::new(0),
    };
    for _ in 0..3 {
        let st = park_with_lost(
            &mut store,
            Some(&mut work),
            4,
            "M1",
            State::Pending,
            "agent vanished",
            true,
        )
        .unwrap();
        assert_eq!(st, State::Pending);
    }
    let st = park_with_lost(
        &mut store,
        Some(&mut work),
        4,
        "M1",
        State::Pending,
        "agent vanished",
        true,
    )
    .unwrap();
    assert_eq!(st, State::Failed);
    let t = store.get_task("M1").unwrap();
    assert!(t.reason.contains("lost 4 attempts"), "{}", t.reason);
    // a park without a lost attempt does not count towards the cap
    let dir2 = tempfile::tempdir().unwrap();
    let mut s2 = Store::open_in(&dir2.path().join(".orchestrate")).unwrap();
    sync(&mut s2, false, &[plan_task("M1", None)]);
    let st =
        park_with_lost(&mut s2, None, 4, "M1", State::Paused, "a question", false).unwrap();
    assert_eq!(st, State::Paused);
    assert_eq!(s2.get_task("M1").unwrap().lost, 0);
}

// ---- the driver state survives a restart; the per-wait state does not

#[test]
fn driver_state_survives_a_restart_and_prunes_the_wait_state() {
    use orch_core::manager::{DriverState, QueuedJob};
    let dir = tempfile::tempdir().unwrap();
    let orch = dir.path().join(".orchestrate");
    std::fs::create_dir_all(&orch).unwrap();
    let st = DriverState {
        driven_s: 42.5,
        waiting_since: Some(7),
        notified: vec!["M1:1:coder".into()],
        nudged: vec!["M1:1".into()],
        jobs: vec![QueuedJob { task: "M1".into(), file: "fix-1.md".into(), body: "fix it".into() }],
        ..DriverState::default()
    };
    st.save(&orch).unwrap();
    let mut loaded = DriverState::load(&orch);
    assert_eq!(loaded, st);
    loaded.prune_stale();
    assert_eq!(loaded.driven_s, 42.5);
    assert_eq!(loaded.jobs.len(), 1);
    assert!(loaded.notified.is_empty());
    assert!(loaded.nudged.is_empty());
    assert_eq!(loaded.waiting_since, None);
}

// ---- AC9: the bounded poll


#[test]
fn the_poll_reports_blocked_gone_idle_and_done() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    sync(&mut store, false, &[plan_task("M1", None)]);
    store.bump("M1", Counter::Attempt).unwrap();
    store.claim("M1", "w1", None).unwrap();
    let mut del = Deliver { fail_sends: Cell::new(0), resends: Cell::new(0) };
    del.send(
        &mut store,
        NewDelivery { agent: "w1", task: "M1", attempt: 1, kind: DeliveryKind::Coder, prompt: "p" },
    )
    .unwrap();
    let runtime = Runtime { st: RefCell::new(HashMap::new()), out: RefCell::new(HashMap::new()) };
    let poll = HerdrPoll {
        root: dir.path().to_path_buf(),
        adapter: &runtime,
        watchable: &watch_all,
        tun: PollTunables {
            step_s: 0.01,
            idle_grace_s: 120,
            confirm_s: 30,
            coder_budget_s: 2700,
            review_budget_s: 900,
        },
    };
    // working: nothing
    runtime.set("w1", AgentState::Working);
    assert!(poll.scan(&mut store).is_empty());
    // blocked
    runtime.set("w1", AgentState::Blocked);
    let evs = poll.scan(&mut store);
    assert!(matches!(evs.first(), Some(RunEvent::Blocked { agent, .. }) if agent == "w1"), "{evs:?}");
    // gone
    runtime.set("w1", AgentState::Gone);
    let evs = poll.scan(&mut store);
    assert!(matches!(evs.first(), Some(RunEvent::Gone { agent, .. }) if agent == "w1"), "{evs:?}");
    // idle only after the grace
    runtime.set("w1", AgentState::Idle);
    assert!(poll.scan(&mut store).is_empty(), "no idle before the grace");
    backdate(dir.path(), 1, now() - 200);
    let evs = poll.scan(&mut store);
    assert!(matches!(evs.first(), Some(RunEvent::Idle { agent, .. }) if agent == "w1"), "{evs:?}");
    // the done file: DONE
    let done = dir.path().join(".orch-task").join("M1").join("done-1");
    std::fs::create_dir_all(done.parent().unwrap()).unwrap();
    std::fs::write(&done, "").unwrap();
    let evs = poll.scan(&mut store);
    assert!(
        matches!(evs.first(), Some(RunEvent::Done { task, agent }) if task == "M1" && agent == "w1"),
        "{evs:?}"
    );
}

#[test]
fn a_fresh_unconfirmed_delivery_is_left_alone_and_an_old_one_reported() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    sync(&mut store, false, &[plan_task("M1", None)]);
    store.bump("M1", Counter::Attempt).unwrap();
    store.claim("M1", "w1", None).unwrap();
    let mut del = Deliver { fail_sends: Cell::new(1), resends: Cell::new(0) };
    let id = match del
        .send(
            &mut store,
            NewDelivery { agent: "w1", task: "M1", attempt: 1, kind: DeliveryKind::Coder, prompt: "p" },
        )
        .unwrap()
    {
        DeliveryOutcome::Failed { delivery, .. } => delivery.id,
        _ => panic!(),
    };
    let runtime = Runtime { st: RefCell::new(HashMap::new()), out: RefCell::new(HashMap::new()) };
    let poll = HerdrPoll {
        root: dir.path().to_path_buf(),
        adapter: &runtime,
        watchable: &watch_all,
        tun: PollTunables {
            step_s: 0.01,
            idle_grace_s: 120,
            confirm_s: 30,
            coder_budget_s: 2700,
            review_budget_s: 900,
        },
    };
    // fresh: the send may still be in flight
    assert!(poll.scan(&mut store).is_empty());
    // old and the agent is not working: an UNCONFIRMED event
    backdate(dir.path(), id, now() - 200);
    let evs = poll.scan(&mut store);
    assert!(
        matches!(evs.first(), Some(RunEvent::Unconfirmed { task, agent }) if task == "M1" && agent == "w1"),
        "{evs:?}"
    );
    // old and the agent is working after all: nothing (the prompt reached it)
    runtime.set("w1", AgentState::Working);
    assert!(poll.scan(&mut store).is_empty());
}

#[test]
fn a_coding_task_with_no_delivery_is_an_orphan() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    sync(&mut store, false, &[plan_task("M1", None)]);
    store.bump("M1", Counter::Attempt).unwrap();
    store.claim("M1", "w1", None).unwrap();
    let runtime = Runtime { st: RefCell::new(HashMap::new()), out: RefCell::new(HashMap::new()) };
    let poll = HerdrPoll {
        root: dir.path().to_path_buf(),
        adapter: &runtime,
        watchable: &watch_all,
        tun: PollTunables {
            step_s: 0.01,
            idle_grace_s: 120,
            confirm_s: 30,
            coder_budget_s: 2700,
            review_budget_s: 900,
        },
    };
    let evs = poll.scan(&mut store);
    assert!(
        matches!(evs.first(), Some(RunEvent::Orphan { task, status }) if task == "M1" && status == "coding"),
        "{evs:?}"
    );
}

// ---- the run loop with a stop request

#[test]
fn the_loop_reconciles_then_stops_on_request() {
    let mut one = One::new();
    let code = one.with_manager(|m, _| m.run(&|| true));
    assert_eq!(code, 0); // "stopped by request" exits 0 with the run kept
    assert!(joined(&one.say_log).contains("autopilot started"), "{}", joined(&one.say_log));
}

// ---- review flow through the Work seam (AC10)

#[test]
fn a_passing_check_with_a_plan_review_goes_to_review_then_merges() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    let mut plan = plan_task("M1", Some("medium"));
    plan.review = true;
    sync(&mut store, false, &[plan]);
    let mut one = One {
        dir,
        store,
        clock: FkClock(Rc::new(Cell::new(1_000_000))),
        runtime: Runtime { st: RefCell::new(HashMap::new()), out: RefCell::new(HashMap::new()) },
        deliver: Deliver { fail_sends: Cell::new(0), resends: Cell::new(0) },
        poll: FkPoll { batches: RefCell::new(vec![]) },
        work: FkWork {
            check: Ok(CheckOutcome::Pass),
            review: ReviewOutcome::Approved,
            fix_rounds: Cell::new(0),
            save_calls: Cell::new(0),
        },
        notes: Notes(RefCell::new(vec![])),
        say_log: Rc::new(RefCell::new(Vec::new())),
    };
    one.cycle(); // dispatched, coding
    one.with_manager(|m, _| {
        m.apply_event(&RunEvent::Done { task: "M1".into(), agent: "w1".into() }).unwrap()
    });
    // verifying: the check passes, the plan asks for a review -> saved, coder freed
    one.cycle();
    assert_eq!(one.store.get_task("M1").unwrap().status, State::AwaitingReview);
    one.cycle(); // the review prompt goes out to the reviewer
    let t = one.store.get_task("M1").unwrap();
    assert_eq!(t.status, State::Reviewing, "the review prompt went out");
    assert_eq!(one.work.save_calls.get(), 1);
    let d = one
        .store
        .latest_delivery("M1", 1, DeliveryKind::Reviewer)
        .unwrap()
        .expect("a review delivery");
    assert_eq!(d.agent, "rev");
    // the reviewer's done file: verdict -> approved -> merged
    one.with_manager(|m, _| {
        m.apply_event(&RunEvent::Reviewed { task: "M1".into(), agent: "rev".into() }).unwrap()
    });
    one.cycle();
    let t = one.store.get_task("M1").unwrap();
    assert_eq!(t.status, State::Merged);
}

// ---- helpers

/// Directly edit the delivery table: the tests simulate crashes and clock drift the
/// running manager cannot produce through its own API.
fn edit_delivery(root: &Path, stmt: &str, id: i64, ts: Option<i64>) {
    let conn =
        rusqlite::Connection::open(root.join(".orchestrate").join(orch_core::store::DB_FILE))
            .unwrap();
    if let Some(ts) = ts {
        conn.execute(stmt, rusqlite::params![ts, id]).unwrap();
    } else {
        conn.execute(stmt, rusqlite::params![id]).unwrap();
    }
}

fn unconfirm(root: &Path, id: i64) {
    edit_delivery(
        root,
        "UPDATE deliveries SET confirmed=0, confirmed_at=NULL WHERE id=?1",
        id,
        None,
    );
}

fn backdate(root: &Path, id: i64, recorded_at: i64) {
    edit_delivery(
        root,
        "UPDATE deliveries SET recorded_at=?1 WHERE id=?2",
        id,
        Some(recorded_at),
    );
}

fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}
