//! `orch events [--peek]` (the lead reads what it was woken for; reading acknowledges it) and
//! the herdr side of waking the lead ([`HerdrLead`]). Output, wording and exit codes follow
//! Python v4.10.1's `cmd_events`; the queue and the wake-up logic live in `orch_core`
//! (`lead_queue`, `wake`).

use orch_core::lead_queue::{LeadEntry, LeadKind};
use orch_core::lease::{driver_alive, driver_info, driver_lock_path};
use orch_core::paths;
use orch_core::run_store::{AgentRole, Run};
use orch_core::store::{DB_FILE, Store};
use orch_core::wake::{LeadPane, LeadRuntime};
use orch_herdr::error::HerdrError;
use orch_herdr::{AgentStatus, RuntimeAdapter};
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

const NO_RUN: &str = "no run in .orchestrate/state.db — run `orch sync` after writing plan.md";
const NO_LEAD: &str =
    "This run has no lead events (plan header: `Lead: lead (<kind>)`, and not `Lead mode: off`).";
const NOT_RUNNING: &str = "NOT running — your next `orch dispatch` or `orch answer` starts it";
const CLOSING: &str = "Handle each event above, then STOP: you are woken again when something else happens. Never prompt a coder yourself and never poll.";

/// What the lead is told to do about each kind (Python's `EVENT_HELP`; `{t}` is the milestone).
fn event_help(kind: LeadKind) -> &'static str {
    match kind {
        LeadKind::Paused => {
            "a coder's question, or an agent that stopped. A question only the developer can settle: ask them, then `orch answer {t} \"<their answer>\"`. An agent problem: tell the developer what to look at, then `orch dispatch {t}`."
        }
        LeadKind::Failed => {
            "still failing after its rounds. Read the last report and check or review output, add the missing FACT or constraint to the plan (never a how-to), then `orch dispatch {t}` (add `--tier high` or `--to <worker>` to give it a stronger model)."
        }
        LeadKind::Held => {
            "risky changes wait for the developer: show them the reasons; they run `orch approve {t}`. Never approve it yourself."
        }
        LeadKind::NeedsAnswer => {
            "open questions in plan.md: ask the developer, write the answers under \"Notes from user\", set \"Open questions:\" to `- none`, `orch sync`, then `orch dispatch {t}`."
        }
        LeadKind::Merged => {
            "landed on the feature branch. Nothing to do unless it unlocked work listed under READY."
        }
        LeadKind::Ready => "milestones are ready for you: `orch dispatch <Mn>...`.",
        LeadKind::Escalated => "moved to a stronger model after its rounds ran out. Nothing to do.",
        LeadKind::AutoDispatched => {
            "started without you because you did not dispatch it in time. Nothing to do."
        }
        LeadKind::AllDone => {
            "every milestone landed. Finish .orchestrate/summary.md (see `orch lead run`, section ALL_DONE)."
        }
        LeadKind::Stopped => {
            "the event manager stopped. Tell the developer why; they run `orch autopilot` again."
        }
        LeadKind::AnswerProvided => {
            "an answer for {t} was given (by you or by the developer) and goes to its coder. Nothing to do unless it changes what you told the developer."
        }
    }
}

/// 0 for no tier, else 1..3 (Python `rank`).
fn rank(tier: Option<&str>) -> usize {
    tier.and_then(|t| orch_config::models::TIERS.iter().position(|x| *x == t))
        .map_or(0, |i| i + 1)
}

/// Workers allowed to take a milestone of `tier`: its tier or higher (unrated workers count as
/// any tier); when nobody reaches it, the strongest workers (Python `eligible_workers`).
fn eligible_workers(run: &Run, tier: Option<&str>) -> Vec<String> {
    let need = rank(tier);
    let workers: Vec<(&str, usize)> = run
        .agents
        .iter()
        .filter(|a| a.role == AgentRole::Worker)
        .map(|a| (a.name.as_str(), rank(a.tier.as_deref())))
        .collect();
    let ok: Vec<String> = workers
        .iter()
        .filter(|(_, r)| need == 0 || *r == 0 || *r >= need)
        .map(|(n, _)| n.to_string())
        .collect();
    if !ok.is_empty() {
        return ok;
    }
    let top = workers.iter().map(|(_, r)| *r).max().unwrap_or(0);
    workers
        .iter()
        .filter(|(_, r)| *r == top)
        .map(|(n, _)| n.to_string())
        .collect()
}

fn show(out: &mut String, store: &Store, e: &LeadEntry) -> Result<(), String> {
    let mut head = format!("#{} {}", e.id, e.kind.heading());
    if let Some(t) = &e.task {
        let title = store.get_task(t).map(|t| t.title).unwrap_or_default();
        let _ = write!(head, " {t} \"{title}\"");
    }
    if !e.text.is_empty() {
        let _ = write!(head, ": {}", e.text);
    }
    let help = event_help(e.kind).replace("{t}", e.task.as_deref().unwrap_or("<Mn>"));
    let _ = writeln!(out, "{head}\n    -> {help}");
    Ok(())
}

/// The text `orch events` prints for a run, and the entries that were new (to acknowledge).
/// `driver` is the event manager's line.
pub fn render(store: &Store, run: &Run, driver: &str) -> Result<(String, Vec<LeadEntry>), String> {
    let e = |x: orch_core::store::StoreError| x.to_string();
    let new = store.unread_lead_events().map_err(e)?;
    let old = store.still_open_lead_events().map_err(e)?;
    let mut out = format!(
        "EVENTS for the lead — run {}: {} new, {} still open\n",
        run.settings.branch,
        new.len(),
        old.len()
    );
    for x in &new {
        show(&mut out, store, x)?;
    }
    if !old.is_empty() {
        out.push_str("STILL OPEN (you have seen these; they close when the milestone moves on):\n");
        for x in &old {
            show(&mut out, store, x)?;
        }
    }
    let hold = store.undispatched().map_err(e)?;
    if !hold.is_empty() {
        out.push_str("READY — waiting for your dispatch:\n");
        for tid in &hold {
            let t = store.get_task(tid).map_err(e)?;
            let tier = run
                .plan
                .iter()
                .find(|p| &p.id == tid)
                .and_then(|p| p.tier.clone());
            let _ = writeln!(
                out,
                "  {tid} \"{}\"{} -> {}:  orch dispatch {tid}",
                t.title,
                tier.as_ref()
                    .map_or(String::new(), |x| format!(" tier {x}")),
                eligible_workers(run, tier.as_deref()).join(" or ")
            );
        }
    }
    let tasks = store.list_tasks().map_err(e)?;
    let workers: Vec<String> = run
        .agents
        .iter()
        .filter(|a| a.role == AgentRole::Worker)
        .map(|w| {
            let model = w.model.as_ref().map_or(String::new(), |m| {
                format!(": {m}, {}", w.tier.as_deref().unwrap_or("unrated"))
            });
            let state = match tasks
                .iter()
                .find(|t| t.worker.as_deref() == Some(w.name.as_str()))
            {
                Some(t) => format!(" on {}", t.id),
                None => " free".to_string(),
            };
            format!("{} ({}{model}){state}", w.name, w.kind)
        })
        .collect();
    let _ = writeln!(out, "WORKERS: {}", workers.join(", "));
    let _ = writeln!(out, "event manager: {driver}");
    if new.is_empty() && old.is_empty() && hold.is_empty() {
        out.push_str("Nothing needs you.\n");
    }
    let _ = writeln!(out, "{CLOSING}");
    Ok((out, new))
}

/// `orch events [--peek]` for the repo around `start`. Exit 0, also when the run has no lead.
pub fn run_events(start: &Path, args: &[String], out: &mut dyn Write) -> i32 {
    let peek = args.iter().any(|a| a == "--peek");
    let die = |m: String| {
        eprintln!("ERROR: {m}");
        2
    };
    let root = match paths::main_root(start) {
        Ok(r) => r,
        Err(e) => return die(e.to_string()),
    };
    let dir = root.join(paths::ORCH_DIR);
    if !dir.join(DB_FILE).exists() {
        return die(NO_RUN.into());
    }
    let mut store = match Store::open_in(&dir) {
        Ok(s) => s,
        Err(e) => return die(e.to_string()),
    };
    let run = match store.load_run() {
        Ok(Some(r)) => r,
        Ok(None) => return die(NO_RUN.into()),
        Err(e) => return die(e.to_string()),
    };
    match store.lead_on() {
        Ok(true) => {}
        Ok(false) => {
            let _ = writeln!(out, "{NO_LEAD}");
            return 0;
        }
        Err(e) => return die(e.to_string()),
    }
    let lock = driver_lock_path(&dir);
    let driver = if driver_alive(&lock) {
        driver_info(&lock).holder_line()
    } else {
        NOT_RUNNING.to_string()
    };
    let (text, new) = match render(&store, &run, &driver) {
        Ok(x) => x,
        Err(e) => return die(e),
    };
    let _ = write!(out, "{text}");
    let _ = out.flush();
    // Reading is the acknowledgement. Only what was shown is marked: an entry queued while we
    // printed stays unread and is woken for.
    if !peek && !new.is_empty() {
        let ids: Vec<i64> = new.iter().map(|e| e.id).collect();
        if let Err(e) = store.mark_lead_read(&ids) {
            return die(e.to_string());
        }
    }
    0
}

/// `orch events [--peek]`.
pub fn events(args: &[String]) -> Option<i32> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    Some(run_events(&cwd, args, &mut std::io::stdout()))
}

/// The lead's pane and prompt delivery, over the herdr runtime adapter.
#[allow(dead_code)] // the event manager (autopilot) milestone constructs it
pub struct HerdrLead<'a>(pub &'a dyn RuntimeAdapter);

impl LeadRuntime for HerdrLead<'_> {
    fn pane(&self, lead: &str) -> LeadPane {
        match self.0.agent(lead) {
            Ok(a) => match a.status {
                AgentStatus::Gone => LeadPane::Gone,
                AgentStatus::Working | AgentStatus::Blocked => LeadPane::Working,
                // idle, done, unknown, or a pane herdr does not watch: only the developer being
                // in the pane stops us typing
                _ if a.focused => LeadPane::FocusedInput,
                _ => LeadPane::Idle,
            },
            Err(HerdrError::AgentNotFound { .. }) => LeadPane::Gone,
            // herdr hiccup: do not type blind; look again next time
            Err(_) => LeadPane::Working,
        }
    }

    fn send(&self, lead: &str, text: &str) -> Result<(), String> {
        self.0.send_prompt(lead, text).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orch_core::wake::{Clock, Notifier, WakeAction, WakeEnv, WakeTunables, wake_tick};
    use orch_herdr::{AgentInfo, FakeHerdr};
    use std::cell::{Cell, RefCell};

    struct Clk(Cell<i64>);
    impl Clock for Clk {
        fn now(&self) -> i64 {
            self.0.get()
        }
    }
    #[derive(Default)]
    struct Notes(RefCell<Vec<String>>);
    impl Notifier for Notes {
        fn notify(&self, m: &str) {
            self.0.borrow_mut().push(m.into());
        }
    }

    fn rig() -> (tempfile::TempDir, Store, FakeHerdr) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
        store.apply_sync(&sync_input()).unwrap();
        let fake = FakeHerdr::new();
        fake.add_pane(orch_herdr::PaneInfo::new("w1:p1", "w1", "w1:t1"));
        fake.add_agent(AgentInfo::new("lead", "claude", "w1:p1", AgentStatus::Idle));
        (dir, store, fake)
    }

    fn sync_input() -> orch_core::run_store::SyncInput {
        use orch_core::run_store::*;
        let agent = |role, name: &str| RunAgent {
            role,
            name: name.into(),
            kind: "claude".into(),
            model: None,
            tier: None,
            detect: true,
            pane: None,
        };
        SyncInput {
            settings: RunSettings {
                branch: "feature/x".into(),
                base: None,
                title: String::new(),
                commits: "per-milestone".into(),
                isolation: "worktree".into(),
                max_parallel: 2,
                setup_inputs: vec![],
                scope: "warn".into(),
                risk_hold: true,
                budget_s: None,
                check_command: String::new(),
                setup_command: "none".into(),
                lead_mode: "events".into(),
                reviewer: None,
            },
            agents: vec![
                agent(AgentRole::Worker, "claude-1"),
                agent(AgentRole::Lead, "lead"),
            ],
            tasks: vec![PlanTask {
                id: "M1".into(),
                title: "One".into(),
                deps: vec![],
                enabled: true,
                open_questions: false,
                tier: None,
                needs: vec![],
                model_why: String::new(),
                prefer: "any".into(),
                review: false,
                files: vec![],
                parallel: false,
            }],
        }
    }

    fn focus(fake: &FakeHerdr, focused: bool) {
        let mut a = AgentInfo::new("lead", "claude", "w1:p1", AgentStatus::Idle);
        a.focused = focused;
        fake.add_agent(a);
    }

    fn tick(
        store: &mut Store,
        fake: &FakeHerdr,
        clock: &Clk,
        notes: &Notes,
    ) -> (WakeAction, Vec<String>) {
        let lead = HerdrLead(fake);
        let env = WakeEnv {
            runtime: &lead,
            notifier: notes,
            clock,
            tun: WakeTunables::default(),
        };
        let mut log = vec![];
        let a = wake_tick(store, &env, &mut |l| log.push(l.to_string())).unwrap();
        (a, log)
    }

    // AC12: the real adapter mapping, against the fake runtime and an injected clock
    #[test]
    fn the_fake_runtime_covers_idle_working_focused_and_gone() {
        let (_d, mut store, fake) = rig();
        store
            .enqueue_lead_event(LeadKind::Merged, Some("M1"), "landed")
            .unwrap();
        let (clock, notes) = (Clk(Cell::new(1000)), Notes::default());

        fake.set_status("lead", AgentStatus::Working);
        assert!(matches!(
            tick(&mut store, &fake, &clock, &notes).0,
            WakeAction::Hold(_)
        ));
        fake.set_status("lead", AgentStatus::Blocked);
        assert!(matches!(
            tick(&mut store, &fake, &clock, &notes).0,
            WakeAction::Hold(_)
        ));
        assert!(fake.sent().is_empty(), "never typed into a working pane");

        fake.set_status("lead", AgentStatus::Idle);
        focus(&fake, true);
        assert!(matches!(
            tick(&mut store, &fake, &clock, &notes).0,
            WakeAction::Hold(_)
        ));
        assert!(fake.sent().is_empty(), "never typed into a focused pane");

        focus(&fake, false);
        let (a, log) = tick(&mut store, &fake, &clock, &notes);
        assert_eq!(a, WakeAction::Send);
        assert!(log.iter().any(|l| l.starts_with("wake sent")), "{log:?}");
        let sent = fake.sent();
        assert_eq!(sent.len(), 1);
        assert!(matches!(&sent[0], orch_herdr::Sent::Prompt { agent, text }
            if agent == "lead" && text.contains("`orch events`")));
    }

    #[test]
    fn never_acknowledged_and_acknowledged_late_through_the_fake() {
        let (_d, mut store, fake) = rig();
        store
            .enqueue_lead_event(LeadKind::Merged, Some("M1"), "landed")
            .unwrap();
        let (clock, notes) = (Clk(Cell::new(1000)), Notes::default());
        assert_eq!(tick(&mut store, &fake, &clock, &notes).0, WakeAction::Send);
        // the fake marks the lead working after a prompt; it finishes without reading
        fake.set_status("lead", AgentStatus::Idle);
        clock.0.set(1020);
        let (a, log) = tick(&mut store, &fake, &clock, &notes);
        assert_eq!(a, WakeAction::ReWake { confirm: true });
        assert!(
            log.iter()
                .any(|l| l.contains("wake unacknowledged after 20s")),
            "{log:?}"
        );
        assert_eq!(fake.sent().len(), 2);
        // it reads late
        let ids: Vec<i64> = store
            .unread_lead_events()
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        store.mark_lead_read(&ids).unwrap();
        clock.0.set(1030);
        let (_, log) = tick(&mut store, &fake, &clock, &notes);
        assert!(
            log.iter()
                .any(|l| l.contains("wake acknowledged after 10s")),
            "{log:?}"
        );
    }

    #[test]
    fn a_missing_lead_agent_is_reported_not_retried() {
        let (_d, mut store, fake) = rig();
        fake.remove_agent("lead");
        store
            .enqueue_lead_event(LeadKind::Merged, Some("M1"), "landed")
            .unwrap();
        let (clock, notes) = (Clk(Cell::new(1000)), Notes::default());
        for i in 0..50 {
            clock.0.set(1000 + i * 30);
            tick(&mut store, &fake, &clock, &notes);
        }
        assert_eq!(notes.0.borrow().len(), 1);
        assert!(fake.sent().is_empty());
    }

    #[test]
    fn a_send_failure_through_the_fake_is_reported() {
        let (_d, mut store, fake) = rig();
        store
            .enqueue_lead_event(LeadKind::Merged, Some("M1"), "landed")
            .unwrap();
        fake.fail_next(
            orch_herdr::CallKind::AgentPrompt,
            100,
            HerdrError::AgentNotFound {
                agent: "lead".into(),
                message: "agent target lead not found".into(),
            },
        );
        let (clock, notes) = (Clk(Cell::new(1000)), Notes::default());
        let (_, log) = tick(&mut store, &fake, &clock, &notes);
        assert!(
            log.iter()
                .any(|l| l.contains("could not wake the lead (lead)")),
            "{log:?}"
        );
        assert_eq!(notes.0.borrow().len(), 1);
    }

    #[test]
    fn render_matches_the_python_layout() {
        let (_d, mut store, _f) = rig();
        store.mark_dispatched("M1").ok();
        let run = store.load_run().unwrap().unwrap();
        let (text, new) = render(&store, &run, NOT_RUNNING).unwrap();
        assert!(new.is_empty());
        assert_eq!(
            text,
            format!(
                "EVENTS for the lead — run feature/x: 0 new, 0 still open\nWORKERS: claude-1 (claude) free\nevent manager: {NOT_RUNNING}\nNothing needs you.\n{CLOSING}\n"
            )
        );
    }
}
