//! `orch autopilot`, the event manager (§9, §24 Phase 6): the single driver of a run.
//! It takes the driver lease, reconciles whatever a crash left half-done, then loops:
//! budget accounting, in-flight work, scheduler dispatch, lead wake-ups, and a bounded
//! poll of the runtime and the done files. Everything git, check, commit, merge and
//! review related goes through `orch_core::manager::Work` — Phase 7 and Phase 8
//! implement it; until then a stub answers with an error the manager parks the
//! milestone with, so the loop is testable end to end with fakes.
//!
//! herdr's adapter is wired onto orch-core's narrow [`RuntimeView`] and
//! [`Deliveries`] traits here; orch-core itself carries no herdr code.
//!
//! `args` are the words after the command: `--once` runs exactly one cycle (the
//! integration tests' lever).

use crate::events_cmd::HerdrLead;
use orch_config::tunables::Tunables;
use orch_core::events::{AgentState, HerdrPoll, PollTunables, RuntimeView};
use orch_core::lease::{driver_lock_path, hhmmss, take_lease};
use orch_core::manager::{
    CheckOutcome, CommitOutcome, Deliveries, DeliveryOutcome, LoopTunables, Manager, NewDelivery,
    Recovered, ReviewOutcome, Work,
};
use orch_core::paths::{ORCH_DIR, main_root};
use orch_core::store::{Store, StoreError};
use orch_core::Error;
use orch_core::wake::{Notifier, NotifySend, SystemClock};
use orch_herdr::adapter::RuntimeAdapter;
use orch_herdr::cli::CliHerdr;
use orch_herdr::delivery::{Deliverer, Recovery, SendOutcome};
use orch_herdr::status::AgentStatus;
use std::cell::Cell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

type DeliveryRes<T> = std::result::Result<T, StoreError>;

/// `orch autopilot [--once]`.
pub fn run(args: &[String]) -> i32 {
    let mut once = false;
    for a in args {
        match a.as_str() {
            "--once" => once = true,
            _ => {
                eprintln!("usage: orch autopilot [--once]");
                return 2;
            }
        }
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Ok(tun) = Tunables::from_env() else {
        eprintln!("ERROR: could not read the tunables from the environment");
        return 1;
    };
    let root = match main_root(&cwd) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    let orch = root.join(ORCH_DIR);
    let mut store = match Store::open_in(&orch) {
        Ok(s) => s,
        Err(e) => return fail(e.into()),
    };
    // AC1: the single driver. A second `orch autopilot` refuses with the holder line;
    // dropping the lease (or dying) frees the run.
    let _lease = match take_lease(&driver_lock_path(&orch)) {
        Ok(l) => l,
        Err(e) => return fail(e),
    };

    let herdr = CliHerdr::from_tunables(&tun);
    // The agents herdr can watch (the synced run's `detect` flags), read once.
    let watching: HashSet<String> = store
        .load_run()
        .ok()
        .flatten()
        .map(|r| {
            r.agents
                .iter()
                .filter(|a| a.detect)
                .map(|a| a.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let watchable = move |name: &str| watching.contains(name);

    let view = HerdrView { adapter: &herdr, watchable: &watchable };
    let mut deliveries = HerdrDeliveries { adapter: &herdr, watchable: &watchable };
    let lead = HerdrLead(&herdr);
    let notifier = &NotifySend;
    let clock = SystemClock;
    let mut poll = HerdrPoll {
        root: root.clone(),
        adapter: &view,
        watchable: &watchable,
        tun: PollTunables {
            step_s: tun.poll_step_s,
            idle_grace_s: tun.idle_grace_s,
            confirm_s: tun.confirm_s,
            coder_budget_s: tun.coder_budget_min * 60,
            review_budget_s: tun.review_budget_min * 60,
        },
    };
    let mut work = StubWork;

    let log_path = orch.join("autopilot.log");
    let mut say = move |msg: &str, important: bool| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
            .unwrap_or(0);
        let line = format!("[{}] {msg}", hhmmss(now));
        println!("{line}");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            use std::io::Write as _;
            let _ = writeln!(f, "{line}");
        }
        if important {
            notifier.notify(msg);
        }
    };

    let loop_tun = LoopTunables {
        idle_grace_s: tun.idle_grace_s,
        confirm_s: tun.confirm_s,
        coder_budget_s: tun.coder_budget_min * 60,
        review_budget_s: tun.review_budget_min * 60,
        send_retry_s: tun.send_retry_s,
        unreachable_min: tun.unreachable_min,
        cooldown_min: tun.cooldown_min,
        gone_cooldown_min: tun.gone_cooldown_min,
        max_lost: tun.max_lost,
        lead_wait_s: tun.lead_wait_s,
        lead_idle_max_s: tun.lead_idle_max_s,
        lead_confirm_s: tun.lead_confirm_s,
        lead_confirm_wakes: tun.lead_confirm_wakes,
        lead_rewake_s: tun.lead_rewake_s,
        lead_max_wakes: tun.lead_max_wakes,
    };

    let mut manager = match Manager::new(
        root,
        &mut store,
        &view,
        &watchable,
        &mut poll,
        &mut work,
        &lead,
        notifier,
        &clock,
        &mut deliveries,
        loop_tun,
        &mut say,
    ) {
        Ok(m) => m,
        Err(e) => return fail(e),
    };
    // the lease is held until the process ends (SIGTERM included); the OS frees it
    // `--once`: the closure is false for the first cycle's check, true afterwards, so
    // exactly one cycle runs.
    let flag = Cell::new(0u32);
    let stop = move || {
        if !once {
            return false;
        }
        let n = flag.get();
        flag.set(n + 1);
        n > 0
    };
    manager.run(&stop)
}

fn fail(e: Error) -> i32 {
    eprintln!("ERROR: {e}");
    e.exit_code()
}

/// herdr's adapter, reduced to what the loop asks (AC10: orch-core sees only the
/// narrow [`RuntimeView`]).
struct HerdrView<'a> {
    adapter: &'a dyn RuntimeAdapter,
    watchable: &'a dyn Fn(&str) -> bool,
}

impl RuntimeView for HerdrView<'_> {
    fn status(&self, agent: &str) -> Option<AgentState> {
        if !(self.watchable)(agent) {
            return None; // unwatchable: only the done file and the budget count
        }
        match self.adapter.agent_status(agent) {
            Ok(AgentStatus::Working) => Some(AgentState::Working),
            Ok(AgentStatus::Blocked) => Some(AgentState::Blocked),
            Ok(AgentStatus::Idle) => Some(AgentState::Idle),
            Ok(AgentStatus::Done) => Some(AgentState::Done),
            Ok(AgentStatus::Gone) => Some(AgentState::Gone),
            _ => None,
        }
    }

    fn output(&self, agent: &str, lines: u32) -> Option<String> {
        self.adapter.read_output(agent, lines).ok()
    }
}

/// The delivery mechanics over herdr's [`Deliverer`] — the record-before-send,
/// confirm-after, re-send-to-the-same-agent code — behind the manager's [`Deliveries`].
struct HerdrDeliveries<'a> {
    adapter: &'a dyn RuntimeAdapter,
    watchable: &'a dyn Fn(&str) -> bool,
}

impl Deliveries for HerdrDeliveries<'_> {
    fn send(&mut self, store: &mut Store, req: NewDelivery<'_>) -> DeliveryRes<DeliveryOutcome> {
        let req = orch_herdr::delivery::NewDelivery {
            agent: req.agent,
            task: req.task,
            attempt: req.attempt,
            kind: req.kind,
            prompt: req.prompt,
        };
        match Deliverer::new(store, self.adapter, self.watchable).send(req) {
            Ok(SendOutcome::Sent { delivery }) => Ok(DeliveryOutcome::Sent { delivery }),
            Ok(SendOutcome::Failed { delivery, error }) => {
                Ok(DeliveryOutcome::Failed { delivery, error: error.to_string() })
            }
            Err(e) => Err(e),
        }
    }

    fn resend(&mut self, store: &mut Store, id: i64) -> DeliveryRes<DeliveryOutcome> {
        match Deliverer::new(store, self.adapter, self.watchable).resend(id) {
            Ok(SendOutcome::Sent { delivery }) => Ok(DeliveryOutcome::Sent { delivery }),
            Ok(SendOutcome::Failed { delivery, error }) => {
                Ok(DeliveryOutcome::Failed { delivery, error: error.to_string() })
            }
            Err(e) => Err(e),
        }
    }

    fn failure_streak(&self, store: &Store, agent: &str) -> u32 {
        store.delivery_fails(agent).unwrap_or(0)
    }

    fn recover(
        &mut self,
        store: &mut Store,
        now: i64,
        grace_s: i64,
    ) -> DeliveryRes<Vec<Recovered>> {
        let out = Deliverer::new(store, self.adapter, self.watchable)
            .recover(now, grace_s)?
            .into_iter()
            .map(|r| match r {
                Recovery::Confirmed { delivery } => Recovered::Confirmed { delivery },
                Recovery::Gone { delivery } => Recovered::Gone { delivery },
                Recovery::Unconfirmed { delivery } => Recovered::Unconfirmed { delivery },
            })
            .collect();
        Ok(out)
    }
}

/// The milestones' git/check/review work, Phase 7 and Phase 8's trait: until then
/// every step answers with an error, which the manager parks the milestone with.
/// The event loop itself is built and tested against this narrow seam.
struct StubWork;

impl Work for StubWork {
    fn prepare(&mut self, _task: &str, _worker: &str) -> std::result::Result<(), String> {
        Err("milestone work (worktrees, setup) is not built yet — Phase 7".into())
    }

    fn write_task(&mut self, _task: &str) -> std::result::Result<(), String> {
        Err("writing task files is not built yet — Phase 7".into())
    }

    fn fix_body(&mut self, _task: &str, _round: u32, _tail: &str) -> std::result::Result<String, String> {
        Err("fix prompts are not built yet — Phase 8".into())
    }

    fn check(&mut self, _task: &str) -> std::result::Result<CheckOutcome, String> {
        Err("running the check command is not built yet — Phase 8".into())
    }

    fn save_work(&mut self, _task: &str) -> std::result::Result<(), String> {
        Err("saving work to the hidden ref is not built yet — Phase 7".into())
    }

    fn review_body(&mut self, _task: &str, _round: u32) -> std::result::Result<String, String> {
        Err("review prompts are not built yet — Phase 7/8".into())
    }

    fn review_verdict(&mut self, _task: &str, _round: u32) -> std::result::Result<ReviewOutcome, String> {
        Err("reading review verdicts is not built yet — Phase 8".into())
    }

    fn commit(&mut self, _task: &str) -> std::result::Result<CommitOutcome, String> {
        Err("commit and merge are not built yet — Phase 7".into())
    }
}

