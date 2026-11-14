//! Waking the lead with confirmed delivery (§9 *Wake-up delivery*, §15A item 5).
//!
//! The event manager is the only component that wakes the lead. A wake-up is not done until
//! the lead reads its events (`orch events` marks them read: the acknowledgement):
//!
//! * the lead is typed into only while its pane is idle, once per batch of unread entries;
//!   a wake-up for a pane that is working or focused with input in progress is held, and sent
//!   once the pane is idle;
//! * after a wake-up a short confirm window (`ORCH_LEAD_CONFIRM_S`) ends in an acknowledgement
//!   or in a re-wake while the pane is idle, up to `ORCH_LEAD_CONFIRM_WAKES` quick re-wakes per
//!   round; rounds repeat after `ORCH_LEAD_REWAKE_S`, up to `ORCH_LEAD_MAX_WAKES`;
//! * when the limit is reached with entries still unread, the developer gets one desktop
//!   notification ("the lead has N unread events since HH:MM") per batch;
//! * the log says `wake sent`, then `wake acknowledged` or `wake unacknowledged after Ns`.
//!
//! [`decide`] is the pure decision. [`wake_tick`] runs one decision against a store, a
//! [`LeadRuntime`] (herdr, or a fake), a [`Notifier`] and a [`Clock`], so the whole thing is
//! testable without herdr. The wake state is saved BEFORE the prompt is typed, so a crash
//! between "send" and "acknowledge" neither loses the batch nor repeats it as a new one.

use crate::store::{Res, Store, now};
use rusqlite::params;

/// The wake-up timing, from the `ORCH_LEAD_*` tunables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WakeTunables {
    /// `ORCH_LEAD_CONFIRM_S`: how long to wait for the acknowledgement before a quick re-wake.
    pub confirm_s: i64,
    /// `ORCH_LEAD_CONFIRM_WAKES`: quick re-wakes per round.
    pub confirm_max: i64,
    /// `ORCH_LEAD_REWAKE_S`: how long a round lasts before the next round.
    pub rewake_s: i64,
    /// `ORCH_LEAD_MAX_WAKES`: rounds before the developer is told.
    pub max_wakes: i64,
}

impl Default for WakeTunables {
    fn default() -> Self {
        Self {
            confirm_s: 20,
            confirm_max: 2,
            rewake_s: 300,
            max_wakes: 3,
        }
    }
}

/// What the lead's pane looks like right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeadPane {
    /// Idle: safe to type into.
    Idle,
    /// Working (or blocked on a prompt of its own): never type into it.
    Working,
    /// The developer is in the pane (focused): never type over their input.
    FocusedInput,
    /// herdr cannot find the lead.
    Gone,
}

/// Why a wake-up is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldReason {
    Working,
    FocusedInput,
    Gone,
}

impl HoldReason {
    fn of(pane: LeadPane) -> HoldReason {
        match pane {
            LeadPane::Working => HoldReason::Working,
            LeadPane::FocusedInput => HoldReason::FocusedInput,
            _ => HoldReason::Gone,
        }
    }
}

/// What [`decide`] says to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeAction {
    /// Nothing to do (nothing unread, or still inside a wait).
    Nothing,
    /// Wake the lead about a new batch.
    Send,
    /// The wake-up was not acknowledged in time and the pane is idle: wake it again.
    /// `confirm` is a quick re-wake inside the round, else the next round.
    ReWake { confirm: bool },
    /// A wake-up is due but the pane is not idle: keep the entries, send when it is.
    Hold(HoldReason),
    /// The limit is reached with entries unread: tell the developer (once per batch).
    Notify,
}

/// The saved wake-up state (table `lead_wake`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WakeState {
    /// Newest queue id the last wake-up covers.
    pub upto: i64,
    /// When the oldest entry of the batch was queued.
    pub since: i64,
    /// When this round of wake-ups started.
    pub round_at: i64,
    /// When the last wake-up was typed.
    pub sent_at: i64,
    /// Rounds sent for this batch.
    pub sends: i64,
    /// Quick re-wakes in this round.
    pub confirm_n: i64,
    /// Sent and not yet seen acknowledged.
    pub awaiting: bool,
    /// The developer was told about this batch.
    pub notified: bool,
    /// The "held, pane busy" line was logged.
    pub held: bool,
    /// The "lead is not in herdr" report was made.
    pub gone_told: bool,
}

/// What [`decide`] looks at.
#[derive(Debug, Clone, Copy)]
pub struct WakeInput {
    /// How many unread, open entries there are.
    pub unread: usize,
    /// Is one of them covered by the last wake-up (the same batch)?
    pub covered: bool,
    pub state: WakeState,
    pub pane: LeadPane,
    pub now: i64,
    pub tun: WakeTunables,
}

/// The pure wake-up decision.
pub fn decide(i: &WakeInput) -> WakeAction {
    if i.unread == 0 {
        return WakeAction::Nothing;
    }
    let st = &i.state;
    let due = |idle_action: WakeAction| {
        if i.pane == LeadPane::Idle {
            idle_action
        } else {
            WakeAction::Hold(HoldReason::of(i.pane))
        }
    };
    // a wake-up the lead has not acted on covers newer entries too: it reads the whole queue
    if st.sends == 0 || !i.covered {
        return due(WakeAction::Send);
    }
    if st.confirm_n < i.tun.confirm_max {
        if i.now - st.sent_at >= i.tun.confirm_s {
            return due(WakeAction::ReWake { confirm: true });
        }
        return WakeAction::Nothing;
    }
    if i.now - st.round_at < i.tun.rewake_s {
        return WakeAction::Nothing;
    }
    if st.sends >= i.tun.max_wakes {
        return if st.notified {
            WakeAction::Nothing
        } else {
            WakeAction::Notify
        };
    }
    due(WakeAction::ReWake { confirm: false })
}

/// The state after a wake-up of this kind was decided at `now`, covering entries up to
/// `newest`; `since` is when the oldest unread entry was queued.
pub fn after_send(
    old: &WakeState,
    action: WakeAction,
    newest: i64,
    since: i64,
    now: i64,
) -> WakeState {
    match action {
        WakeAction::ReWake { confirm: true } => WakeState {
            upto: newest,
            sent_at: now,
            confirm_n: old.confirm_n + 1,
            awaiting: true,
            held: false,
            gone_told: false,
            ..*old
        },
        WakeAction::ReWake { confirm: false } => WakeState {
            upto: newest,
            round_at: now,
            sent_at: now,
            sends: old.sends + 1,
            confirm_n: 0,
            awaiting: true,
            held: false,
            gone_told: false,
            ..*old
        },
        _ => WakeState {
            upto: newest,
            since,
            round_at: now,
            sent_at: now,
            sends: 1,
            confirm_n: 0,
            awaiting: true,
            notified: false,
            held: false,
            gone_told: false,
        },
    }
}

/// The prompt typed into the lead's pane (Python's wording: it tells the lead to run
/// `orch events`).
pub fn wake_prompt(unread: usize, branch: &str) -> String {
    format!(
        "orch: {unread} new event(s) in the run on {branch} need the lead. Run `orch events` and handle what it prints, then stop."
    )
}

/// How the lead's pane is looked at and typed into. The `orch` binary implements it over the
/// herdr [`RuntimeAdapter`](https://docs.rs) (`orch-herdr`); tests use a scripted fake.
pub trait LeadRuntime {
    /// The lead's pane right now.
    fn pane(&self, lead: &str) -> LeadPane;
    /// Type the wake-up prompt into the lead. `Err` carries the delivery failure, for example
    /// "agent target lead not found".
    fn send(&self, lead: &str, text: &str) -> Result<(), String>;
}

/// A desktop notification for things that need the developer (`notify-send`, as Python's `say`).
pub trait Notifier {
    fn notify(&self, message: &str);
}

/// Time, injectable so tests need no sleeping.
pub trait Clock {
    fn now(&self) -> i64;
}

/// The wall clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        now()
    }
}

/// `notify-send "orch autopilot" <message>`, when `notify-send` exists; failures are ignored.
pub struct NotifySend;

impl Notifier for NotifySend {
    fn notify(&self, message: &str) {
        let _ = std::process::Command::new("notify-send")
            .args(["orch autopilot", message])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Everything [`wake_tick`] talks to.
pub struct WakeEnv<'a> {
    pub runtime: &'a dyn LeadRuntime,
    pub notifier: &'a dyn Notifier,
    pub clock: &'a dyn Clock,
    pub tun: WakeTunables,
}

impl Store {
    /// The saved wake-up state (all zero before the first wake-up).
    pub fn wake_state(&self) -> Res<WakeState> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row(
                "SELECT upto, since, round_at, sent_at, sends, confirm_n, awaiting, notified, held, gone_told
                 FROM lead_wake WHERE id=1",
                [],
                |r| {
                    Ok(WakeState {
                        upto: r.get(0)?,
                        since: r.get(1)?,
                        round_at: r.get(2)?,
                        sent_at: r.get(3)?,
                        sends: r.get(4)?,
                        confirm_n: r.get(5)?,
                        awaiting: r.get(6)?,
                        notified: r.get(7)?,
                        held: r.get(8)?,
                        gone_told: r.get(9)?,
                    })
                },
            )
            .optional()?
            .unwrap_or_default())
    }

    /// Save the wake-up state.
    pub fn save_wake_state(&mut self, s: &WakeState) -> Res<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO lead_wake
               (id, upto, since, round_at, sent_at, sends, confirm_n, awaiting, notified, held, gone_told)
             VALUES (1,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                s.upto, s.since, s.round_at, s.sent_at, s.sends, s.confirm_n, s.awaiting,
                s.notified, s.held, s.gone_told
            ],
        )?;
        Ok(())
    }

    /// The lead's agent name (the plan's `Lead:` line), when the run has one.
    pub fn lead_name(&self) -> Res<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row(
                "SELECT name FROM run_agents WHERE role='lead' ORDER BY position LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }
}

/// Local `HH:MM` of an epoch value, as the notification prints it.
pub fn hhmm(epoch: i64) -> String {
    let full = crate::lease::hhmmss(epoch);
    full.get(..5).unwrap_or(&full).to_string()
}

/// One pass of the wake-up logic, run by the event manager each loop. Logs through `say`
/// (the autopilot log line) and returns what it did. Does nothing for a run without a lead
/// or without unread entries.
pub fn wake_tick(
    store: &mut Store,
    env: &WakeEnv<'_>,
    say: &mut dyn FnMut(&str),
) -> Res<WakeAction> {
    let Some(lead) = store
        .lead_name()?
        .filter(|_| store.lead_on().unwrap_or(false))
    else {
        return Ok(WakeAction::Nothing);
    };
    let now = env.clock.now();
    let unread = store.unread_lead_events()?;
    let mut st = store.wake_state()?;
    let covered = unread.iter().any(|e| e.id <= st.upto);

    // The acknowledgement: everything the last wake-up covered was read.
    if st.awaiting && !covered {
        let late = if st.notified {
            " (late, after the developer was told)"
        } else {
            ""
        };
        say(&format!(
            "wake acknowledged after {}s{late}",
            now - st.sent_at
        ));
        st.awaiting = false;
        store.save_wake_state(&st)?;
    }
    if unread.is_empty() {
        return Ok(WakeAction::Nothing);
    }

    let pane = env.runtime.pane(&lead);
    let action = decide(&WakeInput {
        unread: unread.len(),
        covered: covered && st.sends > 0,
        state: st,
        pane,
        now,
        tun: env.tun,
    });
    match action {
        WakeAction::Nothing => {}
        WakeAction::Hold(HoldReason::Gone) => {
            if !st.gone_told {
                let msg = format!(
                    "the lead ({lead}) is not in herdr — start it and name it `{lead}`; {} event(s) wait for it (`orch events --peek` shows them)",
                    unread.len()
                );
                say(&msg);
                env.notifier.notify(&msg);
                st.gone_told = true;
                store.save_wake_state(&st)?;
            }
        }
        WakeAction::Hold(why) => {
            if !st.held {
                let what = match why {
                    HoldReason::FocusedInput => "is focused with input in progress",
                    _ => "is working",
                };
                say(&format!(
                    "wake held: the lead ({lead}) {what}; {} event(s) wait, one wake-up is sent once it is idle",
                    unread.len()
                ));
                st.held = true;
                store.save_wake_state(&st)?;
            }
        }
        WakeAction::Notify => {
            say(&format!("wake unacknowledged after {}s", now - st.sent_at));
            let since = unread.iter().map(|e| e.ts).min().unwrap_or(st.since);
            say(&format!(
                "the lead ({lead}) has not read its events after {} wake-ups — look at its pane, or run `orch events --peek` yourself",
                st.sends
            ));
            env.notifier.notify(&format!(
                "the lead has {} unread events since {}",
                unread.len(),
                hhmm(since)
            ));
            st.notified = true;
            store.save_wake_state(&st)?;
        }
        WakeAction::Send | WakeAction::ReWake { .. } => {
            if matches!(action, WakeAction::ReWake { .. }) {
                say(&format!("wake unacknowledged after {}s", now - st.sent_at));
            }
            let newest = unread.iter().map(|e| e.id).max().unwrap_or(st.upto);
            let since = unread.iter().map(|e| e.ts).min().unwrap_or(now);
            // saved BEFORE typing: a crash from here on re-wakes on schedule, never as a new batch
            let next = after_send(&st, action, newest, since, now);
            store.save_wake_state(&next)?;
            let branch = store.run_settings()?.map(|s| s.branch).unwrap_or_default();
            let text = wake_prompt(unread.len(), &branch);
            match env.runtime.send(&lead, &text) {
                Ok(()) => {
                    let kinds: Vec<String> = unread
                        .iter()
                        .take(6)
                        .map(|e| match &e.task {
                            Some(t) => format!("{} {t}", e.kind.as_str()),
                            None => e.kind.as_str().to_string(),
                        })
                        .collect();
                    say(&format!(
                        "wake sent to the lead ({lead}): {} event(s) — {}",
                        unread.len(),
                        kinds.join(", ")
                    ));
                }
                Err(e) => {
                    let msg = format!(
                        "could not wake the lead ({lead}): {e} — the events are kept; the wake-up is retried on schedule"
                    );
                    say(&msg);
                    if !next.notified {
                        env.notifier.notify(&msg);
                        store.save_wake_state(&WakeState {
                            notified: true,
                            ..next
                        })?;
                    }
                }
            }
        }
    }
    tracing::debug!(?action, unread = unread.len(), "wake tick");
    Ok(action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lead_queue::LeadKind;
    use crate::lead_queue::tests::store_with_run;
    use crate::store::TransitionRequest;
    use crate::task::State;
    use std::cell::{Cell, RefCell};

    struct FakeLead {
        pane: Cell<LeadPane>,
        sent: RefCell<Vec<String>>,
        fail: RefCell<Option<String>>,
    }

    impl FakeLead {
        fn new(pane: LeadPane) -> Self {
            Self {
                pane: Cell::new(pane),
                sent: RefCell::new(vec![]),
                fail: RefCell::new(None),
            }
        }
    }

    impl LeadRuntime for FakeLead {
        fn pane(&self, _lead: &str) -> LeadPane {
            self.pane.get()
        }
        fn send(&self, _lead: &str, text: &str) -> Result<(), String> {
            if let Some(e) = self.fail.borrow().clone() {
                return Err(e);
            }
            self.sent.borrow_mut().push(text.to_string());
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeNotifier(RefCell<Vec<String>>);
    impl Notifier for FakeNotifier {
        fn notify(&self, m: &str) {
            self.0.borrow_mut().push(m.to_string());
        }
    }

    struct FakeClock(Cell<i64>);
    impl Clock for FakeClock {
        fn now(&self) -> i64 {
            self.0.get()
        }
    }

    struct Rig {
        _d: tempfile::TempDir,
        store: Store,
        lead: FakeLead,
        notifier: FakeNotifier,
        clock: FakeClock,
        log: Vec<String>,
    }

    impl Rig {
        fn new(pane: LeadPane) -> Rig {
            let (d, store) = store_with_run(true);
            Rig {
                _d: d,
                store,
                lead: FakeLead::new(pane),
                notifier: FakeNotifier::default(),
                clock: FakeClock(Cell::new(10_000)),
                log: vec![],
            }
        }
        fn event(&mut self, text: &str) {
            self.store
                .enqueue_lead_event(LeadKind::Merged, Some("M1"), text)
                .unwrap();
        }
        fn tick(&mut self) -> WakeAction {
            let env = WakeEnv {
                runtime: &self.lead,
                notifier: &self.notifier,
                clock: &self.clock,
                tun: WakeTunables::default(),
            };
            let log = &mut self.log;
            wake_tick(&mut self.store, &env, &mut |l| log.push(l.to_string())).unwrap()
        }
        fn advance(&self, s: i64) {
            self.clock.0.set(self.clock.0.get() + s);
        }
        fn read_all(&mut self) {
            let ids: Vec<i64> = self
                .store
                .unread_lead_events()
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect();
            self.store.mark_lead_read(&ids).unwrap();
        }
        fn logged(&self, needle: &str) -> usize {
            self.log.iter().filter(|l| l.contains(needle)).count()
        }
    }

    fn input(state: WakeState, pane: LeadPane, now: i64, covered: bool) -> WakeInput {
        WakeInput {
            unread: 1,
            covered,
            state,
            pane,
            now,
            tun: WakeTunables::default(),
        }
    }

    // AC6, AC12: the idle case
    #[test]
    fn an_idle_lead_gets_one_wake_for_the_batch() {
        let mut r = Rig::new(LeadPane::Idle);
        assert_eq!(r.tick(), WakeAction::Nothing, "nothing unread");
        r.event("one");
        r.event("two");
        assert_eq!(r.tick(), WakeAction::Send);
        assert_eq!(r.lead.sent.borrow().len(), 1, "several events, one prompt");
        let text = r.lead.sent.borrow()[0].clone();
        assert_eq!(
            text,
            "orch: 2 new event(s) in the run on feature/x need the lead. Run `orch events` and handle what it prints, then stop."
        );
        assert_eq!(r.tick(), WakeAction::Nothing, "inside the confirm window");
        assert_eq!(r.lead.sent.borrow().len(), 1);
        assert_eq!(r.logged("wake sent"), 1);
    }

    // AC6, AC12: the working case, and the focused-with-input case
    #[test]
    fn a_working_or_focused_lead_is_never_typed_into_and_gets_the_wake_when_idle() {
        for busy in [LeadPane::Working, LeadPane::FocusedInput] {
            let mut r = Rig::new(busy);
            r.event("one");
            assert!(matches!(r.tick(), WakeAction::Hold(_)));
            r.advance(1000);
            assert!(matches!(r.tick(), WakeAction::Hold(_)));
            assert!(r.lead.sent.borrow().is_empty(), "{busy:?}");
            assert_eq!(r.logged("wake held"), 1, "logged once, not every tick");
            r.event("two");
            r.lead.pane.set(LeadPane::Idle);
            assert_eq!(r.tick(), WakeAction::Send);
            assert_eq!(r.lead.sent.borrow().len(), 1, "one wake for both events");
            assert!(r.lead.sent.borrow()[0].contains("2 new event(s)"));
        }
    }

    // AC7, AC9, AC12: never acknowledged
    #[test]
    fn an_unread_wake_is_repeated_then_reported_once() {
        let mut r = Rig::new(LeadPane::Idle);
        r.event("one");
        assert_eq!(r.tick(), WakeAction::Send);
        r.advance(20);
        assert_eq!(r.tick(), WakeAction::ReWake { confirm: true });
        r.advance(20);
        assert_eq!(r.tick(), WakeAction::ReWake { confirm: true });
        r.advance(20);
        assert_eq!(
            r.tick(),
            WakeAction::Nothing,
            "quick re-wakes used up, the round runs on"
        );
        r.advance(300);
        assert_eq!(r.tick(), WakeAction::ReWake { confirm: false });
        assert_eq!(r.store.wake_state().unwrap().sends, 2);
        r.advance(20);
        assert_eq!(
            r.tick(),
            WakeAction::ReWake { confirm: true },
            "a new round has its own quick re-wakes"
        );
    }

    #[test]
    fn the_limit_notifies_once_per_batch_and_is_logged() {
        let mut r = Rig::new(LeadPane::Idle);
        r.event("one");
        let mut notified = 0;
        for _ in 0..400 {
            if r.tick() == WakeAction::Notify {
                notified += 1;
            }
            r.advance(10);
        }
        assert_eq!(notified, 1);
        let sent = r.lead.sent.borrow().len();
        assert_eq!(
            sent as i64,
            WakeTunables::default().max_wakes * 3,
            "3 rounds of a wake and 2 quick re-wakes"
        );
        let n = r.notifier.0.borrow().clone();
        assert_eq!(n.len(), 1);
        assert!(
            n[0].starts_with("the lead has 1 unread events since "),
            "{n:?}"
        );
        assert_eq!(r.logged("wake sent"), sent);
        assert!(r.logged("wake unacknowledged after") >= 1);
        assert_eq!(r.logged("wake acknowledged"), 0);
    }

    // AC9, AC12: acknowledged
    #[test]
    fn reading_the_events_is_logged_as_acknowledged_and_ends_the_batch() {
        let mut r = Rig::new(LeadPane::Idle);
        r.event("one");
        r.tick();
        r.advance(5);
        r.read_all();
        assert_eq!(r.tick(), WakeAction::Nothing);
        assert_eq!(r.logged("wake acknowledged after 5s"), 1);
        r.advance(1000);
        assert_eq!(
            r.tick(),
            WakeAction::Nothing,
            "no re-wake after the acknowledgement"
        );
        assert_eq!(r.lead.sent.borrow().len(), 1);
        // a later event is a NEW batch: woken at once, with the count starting again
        r.event("two");
        assert_eq!(r.tick(), WakeAction::Send);
        assert_eq!(r.store.wake_state().unwrap().sends, 1);
    }

    // AC12: acknowledged late, after the developer was told
    #[test]
    fn a_late_acknowledgement_after_the_notification_is_logged() {
        let mut r = Rig::new(LeadPane::Idle);
        r.event("one");
        for _ in 0..400 {
            r.tick();
            r.advance(10);
        }
        assert_eq!(r.notifier.0.borrow().len(), 1);
        r.read_all();
        r.tick();
        assert_eq!(r.logged("wake acknowledged"), 1);
        assert!(r.log.last().unwrap().contains("late"));
        let sent = r.lead.sent.borrow().len();
        r.advance(5000);
        r.tick();
        assert_eq!(r.lead.sent.borrow().len(), sent);
    }

    #[test]
    fn an_unacknowledged_wake_is_not_repeated_into_a_busy_pane() {
        let mut r = Rig::new(LeadPane::Idle);
        r.event("one");
        r.tick();
        r.lead.pane.set(LeadPane::Working);
        r.advance(60);
        assert!(matches!(r.tick(), WakeAction::Hold(_)));
        assert_eq!(r.lead.sent.borrow().len(), 1);
        r.lead.pane.set(LeadPane::Idle);
        assert_eq!(r.tick(), WakeAction::ReWake { confirm: true });
        assert_eq!(r.lead.sent.borrow().len(), 2);
    }

    // AC10
    #[test]
    fn a_delivery_failure_is_reported_and_not_retried_endlessly() {
        let mut r = Rig::new(LeadPane::Idle);
        *r.lead.fail.borrow_mut() = Some("agent target lead not found".into());
        r.event("one");
        for _ in 0..400 {
            r.tick();
            r.advance(10);
        }
        assert!(r.logged("could not wake the lead (lead): agent target lead not found") >= 1);
        assert_eq!(
            r.notifier.0.borrow().len(),
            1,
            "reported once, not every retry"
        );
        assert_eq!(
            r.logged("not read its events"),
            0,
            "already told: not told twice"
        );
        // bounded: the same limit as any wake-up
        assert!(r.logged("could not wake") as i64 <= WakeTunables::default().max_wakes * 3);
    }

    #[test]
    fn a_lead_that_is_not_in_herdr_is_reported_once() {
        let mut r = Rig::new(LeadPane::Gone);
        r.event("one");
        r.tick();
        r.advance(1000);
        r.tick();
        assert_eq!(r.logged("is not in herdr"), 1);
        assert_eq!(r.notifier.0.borrow().len(), 1);
        assert!(r.lead.sent.borrow().is_empty());
        r.lead.pane.set(LeadPane::Idle);
        assert_eq!(r.tick(), WakeAction::Send);
    }

    // AC11
    #[test]
    fn the_wake_state_survives_a_restart_without_a_second_new_batch() {
        let mut r = Rig::new(LeadPane::Idle);
        r.event("one");
        assert_eq!(r.tick(), WakeAction::Send);
        let path = r.store.path().to_path_buf();
        let saved = r.store.wake_state().unwrap();
        assert_eq!(
            (saved.sends, saved.sent_at, saved.awaiting, saved.notified),
            (1, 10_000, true, false)
        );
        // the process dies after the send and before any acknowledgement
        drop(std::mem::replace(&mut r.store, Store::open(&path).unwrap()));
        r.advance(5);
        assert_eq!(
            r.tick(),
            WakeAction::Nothing,
            "not sent again as if it were new"
        );
        assert_eq!(r.lead.sent.borrow().len(), 1);
        r.advance(20);
        assert_eq!(
            r.tick(),
            WakeAction::ReWake { confirm: true },
            "re-woken on schedule: the batch was not lost"
        );
        assert_eq!(
            r.store.wake_state().unwrap().sends,
            1,
            "still the same round"
        );
        // restart after the developer was notified: still notified once
        let mut r = Rig::new(LeadPane::Idle);
        r.event("one");
        for _ in 0..400 {
            r.tick();
            r.advance(10);
        }
        let path = r.store.path().to_path_buf();
        r.store = Store::open(&path).unwrap();
        assert!(r.store.wake_state().unwrap().notified);
        for _ in 0..100 {
            r.tick();
            r.advance(10);
        }
        assert_eq!(r.notifier.0.borrow().len(), 1);
    }

    #[test]
    fn a_crash_between_saving_and_typing_re_wakes_instead_of_losing_the_batch() {
        let mut r = Rig::new(LeadPane::Idle);
        r.event("one");
        // simulate: the state was saved for a wake-up that was never typed
        let st = after_send(&WakeState::default(), WakeAction::Send, 1, 10_000, 10_000);
        r.store.save_wake_state(&st).unwrap();
        r.advance(25);
        assert_eq!(r.tick(), WakeAction::ReWake { confirm: true });
        assert_eq!(r.lead.sent.borrow().len(), 1);
    }

    #[test]
    fn a_run_without_a_lead_never_wakes() {
        let (_d, mut s) = store_with_run(false);
        let lead = FakeLead::new(LeadPane::Idle);
        let n = FakeNotifier::default();
        let c = FakeClock(Cell::new(1));
        let env = WakeEnv {
            runtime: &lead,
            notifier: &n,
            clock: &c,
            tun: WakeTunables::default(),
        };
        s.transition("M1", &TransitionRequest::auto(State::Paused, Some("x")))
            .unwrap();
        assert_eq!(
            wake_tick(&mut s, &env, &mut |_| {}).unwrap(),
            WakeAction::Nothing
        );
        assert!(lead.sent.borrow().is_empty());
    }

    #[test]
    fn a_blocking_event_that_closed_before_the_wake_is_not_sent() {
        let mut r = Rig::new(LeadPane::Working);
        r.store
            .transition("M1", &TransitionRequest::auto(State::Paused, Some("x")))
            .unwrap();
        r.tick();
        r.store
            .transition("M1", &TransitionRequest::auto(State::Pending, None))
            .unwrap();
        r.lead.pane.set(LeadPane::Idle);
        assert_eq!(r.tick(), WakeAction::Nothing);
        assert!(r.lead.sent.borrow().is_empty());
    }

    #[test]
    fn decide_is_pure_and_total() {
        let fresh = WakeState::default();
        assert_eq!(
            decide(&WakeInput {
                unread: 0,
                ..input(fresh, LeadPane::Idle, 5, false)
            }),
            WakeAction::Nothing
        );
        assert_eq!(
            decide(&input(fresh, LeadPane::Idle, 5, false)),
            WakeAction::Send
        );
        assert_eq!(
            decide(&input(fresh, LeadPane::Working, 5, false)),
            WakeAction::Hold(HoldReason::Working)
        );
        assert_eq!(
            decide(&input(fresh, LeadPane::FocusedInput, 5, false)),
            WakeAction::Hold(HoldReason::FocusedInput)
        );
        assert_eq!(
            decide(&input(fresh, LeadPane::Gone, 5, false)),
            WakeAction::Hold(HoldReason::Gone)
        );
        let sent = after_send(&fresh, WakeAction::Send, 3, 1, 100);
        assert_eq!(
            decide(&input(sent, LeadPane::Idle, 119, true)),
            WakeAction::Nothing
        );
        assert_eq!(
            decide(&input(sent, LeadPane::Idle, 120, true)),
            WakeAction::ReWake { confirm: true }
        );
        // newer entries only (the old batch was read): a new batch
        assert_eq!(
            decide(&input(sent, LeadPane::Idle, 101, false)),
            WakeAction::Send
        );
    }

    #[test]
    fn the_confirm_window_may_be_switched_off() {
        let tun = WakeTunables {
            confirm_max: 0,
            ..WakeTunables::default()
        };
        let sent = after_send(&WakeState::default(), WakeAction::Send, 3, 1, 100);
        let i = |now| WakeInput {
            tun,
            ..input(sent, LeadPane::Idle, now, true)
        };
        assert_eq!(decide(&i(399)), WakeAction::Nothing);
        assert_eq!(decide(&i(400)), WakeAction::ReWake { confirm: false });
    }

    #[test]
    fn hhmm_is_five_characters() {
        assert_eq!(hhmm(0).len(), 5);
        assert_eq!(hhmm(0).as_bytes()[2], b':');
    }
}
