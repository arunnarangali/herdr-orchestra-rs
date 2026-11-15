//! Repeatable performance test: runs 50 milestones through the manager loop with simulated agents,
//! measures time per manager cycle, and measures REST API response times for /status and /events.
//!
//! To run this test:
//! cargo test -p orch --test performance -- --ignored --nocapture

use orch_api::{AppState, Token, bind, serve};
use orch_core::events::{
    AgentState, PollOutcome, PollSource, RunEvent, RuntimeView,
};
use orch_core::manager::{
    CheckOutcome, CommitOutcome, Deliveries, DeliveryOutcome, LoopTunables, Manager, NewDelivery,
    Recovered, ReviewOutcome, Work, Cycle,
};
use orch_core::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::store::{Store, StoreError};
use orch_core::task::State;
use orch_core::wake::{Clock, LeadPane, LeadRuntime, Notifier};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::rc::Rc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

struct FkClock(Rc<Cell<i64>>);
impl Clock for FkClock {
    fn now(&self) -> i64 {
        self.0.get()
    }
}

struct FkNotifier;
impl Notifier for FkNotifier {
    fn notify(&self, _m: &str) {}
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

struct FkRuntime {
    st: RefCell<HashMap<String, AgentState>>,
}

impl RuntimeView for FkRuntime {
    fn status(&self, agent: &str) -> Option<AgentState> {
        self.st.borrow().get(agent).copied().or(Some(AgentState::Working))
    }
    fn output(&self, _agent: &str, _lines: u32) -> Option<String> {
        None
    }
}

struct FkDeliveries;
impl Deliveries for FkDeliveries {
    fn send(
        &mut self,
        store: &mut Store,
        req: NewDelivery<'_>,
    ) -> Result<DeliveryOutcome, StoreError> {
        let id = store.record_delivery(req.agent, req.task, req.attempt, req.kind, req.prompt)?;
        store.confirm_delivery(id)?;
        let delivery = store.delivery(id)?;
        Ok(DeliveryOutcome::Sent { delivery })
    }

    fn resend(&mut self, store: &mut Store, id: i64) -> Result<DeliveryOutcome, StoreError> {
        let delivery = store.delivery(id)?;
        Ok(DeliveryOutcome::Sent { delivery })
    }

    fn failure_streak(&self, _store: &Store, _agent: &str) -> u32 {
        0
    }

    fn recover(&mut self, _store: &mut Store, _now: i64, _grace_s: i64) -> Result<Vec<Recovered>, StoreError> {
        Ok(Vec::new())
    }
}

struct FkWork;
impl Work for FkWork {
    fn prepare(&mut self, _task: &str, _worker: &str) -> Result<(), String> {
        Ok(())
    }
    fn write_task(&mut self, _task: &str) -> Result<(), String> {
        Ok(())
    }
    fn fix_body(&mut self, _task: &str, _round: u32, _tail: &str) -> Result<String, String> {
        Ok("fix".into())
    }
    fn check(&mut self, _desc: &str) -> Result<CheckOutcome, String> {
        Ok(CheckOutcome::Pass)
    }
    fn save_work(&mut self, _task: &str) -> Result<(), String> {
        Ok(())
    }
    fn review_body(&mut self, _task: &str, _round: u32) -> Result<String, String> {
        Ok("review".into())
    }
    fn review_verdict(&mut self, _task: &str, _round: u32) -> Result<ReviewOutcome, String> {
        Ok(ReviewOutcome::Approved)
    }
    fn commit(&mut self, _task: &str) -> Result<CommitOutcome, String> {
        Ok(CommitOutcome::Merged)
    }
}

struct FkPoll;
impl PollSource for FkPoll {
    fn poll(&mut self, store: &mut Store, _secs: u64) -> PollOutcome {
        let tasks = store.list_tasks().unwrap_or_default();
        let mut events = Vec::new();
        for t in &tasks {
            if t.status == State::Coding {
                if let Some(w) = &t.worker {
                    events.push(RunEvent::Done {
                        task: t.id.clone(),
                        agent: w.clone(),
                    });
                }
            } else if t.status == State::Reviewing {
                events.push(RunEvent::Reviewed {
                    task: t.id.clone(),
                    agent: "rev".into(),
                });
            }
        }
        if events.is_empty() {
            PollOutcome::Waiting { nothing_running: true }
        } else {
            PollOutcome::Events(events)
        }
    }
}

fn watch_all(_: &str) -> bool {
    true
}

fn build_50_plan() -> Vec<PlanTask> {
    let mut tasks = Vec::with_capacity(50);
    // M1: root foundation
    tasks.push(PlanTask {
        id: "M1".into(),
        title: "Foundation".into(),
        deps: vec![],
        enabled: true,
        open_questions: false,
        tier: Some("small".into()),
        needs: vec![],
        model_why: "root".into(),
        prefer: "any".into(),
        review: true,
        files: vec!["src/lib.rs".into()],
        parallel: true,
    });
    // M2..M10: depend on M1
    for i in 2..=10 {
        tasks.push(PlanTask {
            id: format!("M{i}"),
            title: format!("Module {i}"),
            deps: vec!["M1".into()],
            enabled: true,
            open_questions: false,
            tier: Some("small".into()),
            needs: vec![],
            model_why: format!("tier 1 - M{i}"),
            prefer: "any".into(),
            review: true,
            files: vec![format!("src/mod_{i}.rs")],
            parallel: true,
        });
    }
    // M11..M25: depend on earlier M2..M10
    for i in 11..=25 {
        let parent = (i % 9) + 2;
        tasks.push(PlanTask {
            id: format!("M{i}"),
            title: format!("Submodule {i}"),
            deps: vec![format!("M{parent}")],
            enabled: true,
            open_questions: false,
            tier: Some("medium".into()),
            needs: vec![],
            model_why: format!("tier 2 - M{i}"),
            prefer: "any".into(),
            review: true,
            files: vec![format!("src/sub_{i}.rs")],
            parallel: true,
        });
    }
    // M26..M45: depend on M11..M25
    for i in 26..=45 {
        let parent = (i % 15) + 11;
        tasks.push(PlanTask {
            id: format!("M{i}"),
            title: format!("Component {i}"),
            deps: vec![format!("M{parent}")],
            enabled: true,
            open_questions: false,
            tier: Some("medium".into()),
            needs: vec![],
            model_why: format!("tier 3 - M{i}"),
            prefer: "any".into(),
            review: true,
            files: vec![format!("src/comp_{i}.rs")],
            parallel: true,
        });
    }
    // M46..M50: final pipeline, sequential
    for i in 46..=50 {
        let parent = i - 1;
        tasks.push(PlanTask {
            id: format!("M{i}"),
            title: format!("Final {i}"),
            deps: vec![format!("M{parent}")],
            enabled: true,
            open_questions: false,
            tier: Some("high".into()),
            needs: vec![],
            model_why: format!("tier 4 - M{i}"),
            prefer: "any".into(),
            review: true,
            files: vec![format!("src/final_{i}.rs")],
            parallel: false,
        });
    }
    assert_eq!(tasks.len(), 50);
    tasks
}

fn percentile(sorted: &[Duration], pct: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let idx = ((sorted.len() as f64 * pct) / 100.0).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn format_stats(name: &str, mut samples: Vec<Duration>) -> String {
    if samples.is_empty() {
        return format!("{name}: no samples");
    }
    samples.sort();
    let count = samples.len();
    let sum: Duration = samples.iter().sum();
    let mean = sum / count as u32;
    let min = samples[0];
    let max = samples[count - 1];
    let p50 = percentile(&samples, 50.0);
    let p95 = percentile(&samples, 95.0);
    let p99 = percentile(&samples, 99.0);
    format!(
        "{name:<22} | n={count:<4} | avg={mean:<9.2?} | p50={p50:<9.2?} | p95={p95:<9.2?} | p99={p99:<9.2?} | min={min:<8.2?} | max={max:<9.2?}",
    )
}

async fn raw_request(addr: SocketAddr, path: &str, token: &str) -> (u16, String) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status_line = head.lines().next().unwrap_or("");
    let code: u16 = status_line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
    (code, body.to_string())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_performance_50_milestones() {
    let dir = tempfile::tempdir().unwrap();
    let orch_dir = dir.path().join(".orchestrate");
    std::fs::create_dir_all(&orch_dir).unwrap();

    let mut store = Store::open_in(&orch_dir).unwrap();

    let workers = vec![
        RunAgent {
            role: AgentRole::Worker,
            name: "w1".into(),
            kind: "claude".into(),
            model: Some("opus".into()),
            tier: Some("high".into()),
            detect: true,
            pane: None,
        },
        RunAgent {
            role: AgentRole::Worker,
            name: "w2".into(),
            kind: "claude".into(),
            model: Some("sonnet".into()),
            tier: Some("medium".into()),
            detect: true,
            pane: None,
        },
        RunAgent {
            role: AgentRole::Worker,
            name: "w3".into(),
            kind: "codex".into(),
            model: Some("gpt-5".into()),
            tier: Some("medium".into()),
            detect: true,
            pane: None,
        },
        RunAgent {
            role: AgentRole::Worker,
            name: "w4".into(),
            kind: "codex".into(),
            model: Some("gpt-4".into()),
            tier: Some("small".into()),
            detect: true,
            pane: None,
        },
        RunAgent {
            role: AgentRole::Reviewer,
            name: "rev".into(),
            kind: "claude".into(),
            model: Some("opus".into()),
            tier: Some("high".into()),
            detect: true,
            pane: None,
        },
    ];

    let plan_tasks = build_50_plan();

    store
        .apply_sync(&SyncInput {
            settings: RunSettings {
                branch: "feature/perf-50".into(),
                base: Some("main".into()),
                title: "50-Milestone Performance Benchmark".into(),
                commits: "per-milestone".into(),
                isolation: "worktree".into(),
                max_parallel: 4,
                setup_inputs: vec![],
                scope: "warn".into(),
                risk_hold: false,
                budget_s: None,
                check_command: "cargo check".into(),
                setup_command: String::new(),
                lead_mode: "off".into(),
                reviewer: Some("rev".into()),
            },
            agents: workers,
            tasks: plan_tasks,
        })
        .unwrap();

    let clock = FkClock(Rc::new(Cell::new(1_700_000_000)));
    let runtime = FkRuntime {
        st: RefCell::new(HashMap::new()),
    };
    let mut deliveries = FkDeliveries;
    let mut work = FkWork;
    let lead = FkLead;
    let notifier = FkNotifier;
    let mut poll = FkPoll;
    let mut say_log = Vec::new();
    let mut say = |msg: &str, _imp: bool| {
        say_log.push(msg.to_string());
    };

    let tun = LoopTunables {
        idle_grace_s: 120,
        confirm_s: 30,
        coder_budget_s: 1800,
        review_budget_s: 600,
        send_retry_s: 5,
        unreachable_min: 5,
        cooldown_min: 5,
        gone_cooldown_min: 2,
        max_lost: 3,
        lead_wait_s: 300,
        lead_idle_max_s: 300,
        lead_confirm_s: 60,
        lead_confirm_wakes: 3,
        lead_rewake_s: 60,
        lead_max_wakes: 10,
    };

    let mut manager = Manager::new(
        dir.path().to_path_buf(),
        &mut store,
        &runtime,
        &watch_all,
        &mut poll,
        &mut work,
        &lead,
        &notifier,
        &clock,
        &mut deliveries,
        tun,
        &mut say,
    )
    .unwrap();

    let mut cycle_times = Vec::new();
    let manager_start = Instant::now();
    let mut cycles = 0;

    loop {
        cycles += 1;
        clock.0.set(clock.0.get() + 5);

        let t0 = Instant::now();
        let cycle_outcome = manager.cycle();
        cycle_times.push(t0.elapsed());

        match cycle_outcome {
            Cycle::Exit(_code) => {
                break;
            }
            Cycle::Continue => {}
        }

        // Poll events and apply to manager
        let evs = match manager.poll.poll(manager.store, 0) {
            PollOutcome::Events(e) => e,
            _ => Vec::new(),
        };
        for ev in evs {
            let _ = manager.apply_event(&ev);
        }

        if cycles > 2000 {
            panic!("Manager run exceeded 2000 cycles without finishing");
        }
    }
    let manager_total = manager_start.elapsed();

    // Verify all 50 milestones reached Merged status
    let final_tasks = store.list_tasks().unwrap();
    assert_eq!(final_tasks.len(), 50);
    for t in &final_tasks {
        assert_eq!(t.status, State::Merged, "Task {} was not merged: {:?}", t.id, t.status);
    }

    // Now measure API performance on the resulting 50-task database
    let token_secret = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let token = Token::new(token_secret);
    let app_state = AppState::new(dir.path().to_path_buf(), token);

    let listener = bind("127.0.0.1", 0).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let srv = tokio::spawn(serve(listener, app_state, async {
        let _ = shutdown_rx.await;
    }));

    // Warmup request
    let (code, _) = raw_request(addr, "/api/v1/health", token_secret).await;
    assert_eq!(code, 200);

    // Measure /api/v1/status (100 requests)
    let mut status_times = Vec::with_capacity(100);
    for _ in 0..100 {
        let t0 = Instant::now();
        let (code, body) = raw_request(addr, "/api/v1/status", token_secret).await;
        status_times.push(t0.elapsed());
        assert_eq!(code, 200, "status endpoint returned {code}: {body}");
    }

    // Measure /api/v1/events (100 requests)
    let mut events_times = Vec::with_capacity(100);
    for _ in 0..100 {
        let t0 = Instant::now();
        let (code, body) = raw_request(addr, "/api/v1/events", token_secret).await;
        events_times.push(t0.elapsed());
        assert_eq!(code, 200, "events endpoint returned {code}: {body}");
    }

    let _ = shutdown_tx.send(());
    let _ = srv.await;

    println!("\n=================== PERFORMANCE TEST RESULTS (50 Milestones) ===================");
    println!("Total milestones:      50 (all successfully completed and merged)");
    println!("Total manager cycles:  {cycles}");
    println!("Total run duration:    {manager_total:.2?}");
    println!("----------------------------------------------------------------------------------");
    println!("{}", format_stats("Manager Cycle", cycle_times));
    println!("{}", format_stats("API /api/v1/status", status_times));
    println!("{}", format_stats("API /api/v1/events", events_times));
    println!("==================================================================================\n");
}
