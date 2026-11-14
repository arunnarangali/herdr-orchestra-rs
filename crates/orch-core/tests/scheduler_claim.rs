//! The scheduler and atomic claiming against a real store (§10): the whole pass —
//! intent, benches, tiers, file serialization, claim — on one database file, plus the
//! concurrent-claim and review-pipelining cases a unit test with a hand-built snapshot
//! cannot cover.

use orch_core::claim::ClaimError;
use orch_core::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::scheduler::{self, Escalation, Hold, Intent, Job};
use orch_core::store::{NewTask, Store, TransitionRequest};
use orch_core::task::State;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    (dir, s)
}

fn settings() -> RunSettings {
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
        lead_mode: "events".into(),
        reviewer: None,
    }
}

fn plan_task(id: &str, tier: Option<&str>, files: &[&str], parallel: bool) -> PlanTask {
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
        files: files.iter().map(|f| f.to_string()).collect(),
        parallel,
    }
}

fn sync_run(
    s: &mut Store,
    workers: &[(&str, &str, Option<&str>)],
    tasks: Vec<PlanTask>,
    shared: bool,
) {
    let mut settings = settings();
    if shared {
        settings.isolation = "shared".into();
        settings.max_parallel = 1;
    }
    let agents = workers
        .iter()
        .map(|(name, kind, tier)| RunAgent {
            role: AgentRole::Worker,
            name: name.to_string(),
            kind: kind.to_string(),
            model: None,
            tier: tier.map(String::from),
            detect: true,
            pane: None,
        })
        .collect();
    s.apply_sync(&SyncInput {
        settings,
        agents,
        tasks,
    })
    .unwrap();
}

fn go(s: &mut Store, id: &str) {
    s.set_intent(
        id,
        &Intent {
            go: true,
            ..Intent::default()
        },
    )
    .unwrap();
}

fn assigned_to<'a>(out: &'a scheduler::Schedule, task: &str) -> Option<&'a str> {
    out.assignments
        .iter()
        .find(|a| a.task == task)
        .map(|a| a.worker.as_str())
}

fn hold_of<'a>(out: &'a scheduler::Schedule, task: &str) -> Option<&'a Hold> {
    out.unassigned
        .iter()
        .find(|u| u.task == task)
        .map(|u| &u.hold)
}

#[test]
fn a_full_pass_assigns_two_ready_milestones_to_two_free_coders() {
    let (_d, mut s) = store();
    sync_run(
        &mut s,
        &[
            ("w1", "claude", Some("medium")),
            ("w2", "codex", Some("medium")),
        ],
        vec![
            plan_task("M1", Some("medium"), &["src/a.rs"], false),
            plan_task("M2", Some("medium"), &["src/b.rs"], false),
        ],
        false,
    );
    // A lead is on: without a dispatch nothing starts.
    let out = scheduler::schedule(&s.scheduler_snapshot().unwrap().unwrap(), 10);
    assert!(out.assignments.is_empty());
    for id in ["M1", "M2"] {
        assert_eq!(hold_of(&out, id), Some(&Hold::LeadHold));
    }
    // The lead dispatches M1 to w2 explicitly.
    s.set_intent(
        "M1",
        &Intent {
            go: true,
            assign: Some("w2".into()),
            ..Intent::default()
        },
    )
    .unwrap();
    let out = scheduler::schedule(&s.scheduler_snapshot().unwrap().unwrap(), 10);
    assert_eq!(
        out.assignments
            .iter()
            .map(|a| (a.task.as_str(), a.worker.as_str(), a.job))
            .collect::<Vec<_>>(),
        [("M1", "w2", Job::New)]
    );
    assert_eq!(
        hold_of(&out, "M2"),
        Some(&Hold::LeadHold),
        "M2 still waits for the lead"
    );
    // Claim it atomically, verify the busy rules.
    s.claim("M1", "w2", None).unwrap();
    assert_eq!(
        s.intent("M1").unwrap().unwrap().assign,
        None,
        "--to holds for one dispatch"
    );
    assert!(matches!(
        s.claim("M2", "w2", None),
        Err(ClaimError::WorkerBusy { .. })
    ));
    assert_eq!(s.get_task("M1").unwrap().status, State::Coding);
}

#[test]
fn two_drivers_claiming_the_same_milestone_never_both_win() {
    let (_d, mut s) = store();
    sync_run(
        &mut s,
        &[("w1", "claude", Some("medium"))],
        vec![plan_task("M1", Some("medium"), &[], false)],
        false,
    );
    go(&mut s, "M1");
    let path = s.path().to_path_buf();
    drop(s);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|i| {
            let p = path.clone();
            let b = barrier.clone();
            std::thread::spawn(move || {
                let mut s = Store::open(&p).unwrap();
                // Both drivers decide on a snapshot taken before either claims, then race
                // to claim (a snapshot taken after the other's claim would see nothing ready).
                let snap = s.scheduler_snapshot().unwrap().unwrap();
                let out = scheduler::schedule(&snap, 10);
                assert_eq!(out.assignments.len(), 1, "driver {i}");
                b.wait();
                let ok = s.claim("M1", "w1", None).is_ok();
                (i, ok)
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(
        results.iter().filter(|(_, ok)| *ok).count(),
        1,
        "{results:?}"
    );
    let s = Store::open(&path).unwrap();
    let t = s.get_task("M1").unwrap();
    assert_eq!(t.status, State::Coding);
    assert_eq!(s.history("M1").unwrap().len(), 1);
}

#[test]
fn file_serialization_and_benches_across_a_full_pass() {
    let (_d, mut s) = store();
    sync_run(
        &mut s,
        &[
            ("w1", "claude", Some("medium")),
            ("w2", "codex", Some("medium")),
        ],
        vec![
            plan_task("M1", Some("medium"), &["src/a.rs"], false),
            plan_task("M2", Some("medium"), &["src/a.rs"], false),
            plan_task("M3", Some("medium"), &["src/b.rs"], false),
        ],
        false,
    );
    for id in ["M1", "M2", "M3"] {
        go(&mut s, id);
    }
    s.bench_agent("w2", "rate-limit", 1_000).unwrap();
    // Pass 1: only w1 is free; M1 takes it and M2 (same files) waits.
    let out = scheduler::schedule(&s.scheduler_snapshot().unwrap().unwrap(), 10);
    assert_eq!(assigned_to(&out, "M1"), Some("w1"));
    assert_eq!(
        hold_of(&out, "M2"),
        Some(&Hold::FilesBusy(vec!["M1".into()]))
    );
    assert_eq!(hold_of(&out, "M3"), Some(&Hold::CodersBusy));
    s.claim("M1", "w1", None).unwrap();
    // Pass 2: M1 runs; M2 still blocked on files, M3 still benched out.
    let out = scheduler::schedule(&s.scheduler_snapshot().unwrap().unwrap(), 10);
    assert!(out.assignments.is_empty());
    assert_eq!(
        hold_of(&out, "M2"),
        Some(&Hold::FilesBusy(vec!["M1".into()]))
    );
    assert_eq!(hold_of(&out, "M3"), Some(&Hold::CodersBusy));
    // M1 merges: the file frees, the bench expires, and both go out in one pass.
    s.transition("M1", &TransitionRequest::manual(State::Merged, None, true))
        .unwrap();
    let out = scheduler::schedule(&s.scheduler_snapshot().unwrap().unwrap(), 2_000);
    assert_eq!(
        out.assignments
            .iter()
            .map(|a| (a.task.as_str(), a.worker.as_str()))
            .collect::<Vec<_>>(),
        [("M2", "w1"), ("M3", "w2")]
    );
    s.claim("M2", "w1", None).unwrap();
    s.claim("M3", "w2", None).unwrap();
}

#[test]
fn a_fix_job_goes_out_before_new_work_and_back_to_the_builder() {
    let (_d, mut s) = store();
    sync_run(
        &mut s,
        &[("w1", "claude", Some("medium"))],
        vec![
            plan_task("M1", Some("medium"), &[], false),
            plan_task("M2", Some("medium"), &[], false),
        ],
        false,
    );
    go(&mut s, "M1");
    go(&mut s, "M2");
    s.claim("M1", "w1", None).unwrap();
    assert!(matches!(
        s.claim("M2", "w1", None),
        Err(ClaimError::WorkerBusy { .. })
    ));
    // M1's checks fail: a fix round for the same coder.
    s.transition(
        "M1",
        &TransitionRequest::manual(State::NeedsFix, Some("checks"), true),
    )
    .unwrap();
    s.release_claim("M1").unwrap();
    let out = scheduler::schedule(&s.scheduler_snapshot().unwrap().unwrap(), 10);
    assert_eq!(
        out.assignments
            .iter()
            .map(|a| (a.task.as_str(), a.worker.as_str(), a.job))
            .collect::<Vec<_>>(),
        [("M1", "w1", Job::Fix)],
        "the fix job first, back to the builder"
    );
    assert_eq!(hold_of(&out, "M2"), Some(&Hold::CodersBusy));
}

#[test]
fn review_pipelining_frees_the_coder_for_the_next_milestone() {
    let (_d, mut s) = store();
    sync_run(
        &mut s,
        &[
            ("w1", "claude", Some("medium")),
            ("w2", "codex", Some("medium")),
        ],
        vec![
            plan_task("M1", Some("medium"), &["src/a.rs"], false),
            plan_task("M2", Some("medium"), &["src/b.rs"], false),
        ],
        false,
    );
    go(&mut s, "M1");
    go(&mut s, "M2");
    s.claim("M1", "w1", None).unwrap();
    s.transition("M1", &TransitionRequest::auto(State::Verifying, None))
        .unwrap();
    // Checks passed with Review: yes — handoff: the work is saved, the coder is freed,
    // the review is queued. M1 keeps its file booked (it is still active) until it merges.
    s.release_claim("M1").unwrap();
    s.transition(
        "M1",
        &TransitionRequest::auto(State::AwaitingReview, Some("review")),
    )
    .unwrap();
    let out = scheduler::schedule(&s.scheduler_snapshot().unwrap().unwrap(), 10);
    assert_eq!(
        assigned_to(&out, "M2"),
        Some("w1"),
        "the freed coder takes the next ready milestone while the review is pending"
    );
    s.claim("M2", "w1", None).unwrap();
    // The reviewer finds blockers: the fix job comes back and prefers the builder.
    s.transition(
        "M1",
        &TransitionRequest::manual(State::Reviewed, None, true),
    )
    .unwrap();
    s.transition(
        "M1",
        &TransitionRequest::manual(State::NeedsFix, Some("blockers"), true),
    )
    .unwrap();
    s.release_claim("M2").unwrap();
    let snap = s.scheduler_snapshot().unwrap().unwrap();
    assert_eq!(scheduler::pick(&snap, "M1", 10).as_deref(), Some("w1"));
}

#[test]
fn escalation_moves_once_to_the_next_stronger_tier_then_stops() {
    let (_d, mut s) = store();
    sync_run(
        &mut s,
        &[
            ("w1", "claude", Some("medium")),
            ("w2", "codex", Some("high")),
        ],
        vec![plan_task("M1", Some("medium"), &[], false)],
        false,
    );
    go(&mut s, "M1");
    s.claim("M1", "w1", None).unwrap();
    let snap = s.scheduler_snapshot().unwrap().unwrap();
    let team: Vec<Option<&str>> = snap.workers.iter().map(|w| w.tier.as_deref()).collect();
    let task = &snap.tasks[0];
    let need = scheduler::need_rank(task.tier.as_deref(), None);
    let cur = task
        .task
        .worker
        .as_deref()
        .and_then(|w| snap.workers.iter().find(|x| x.name == w))
        .and_then(|w| w.tier.as_deref());
    assert_eq!(
        scheduler::escalate(false, cur, need, &team),
        Escalation::ToTier("high")
    );
    s.set_intent(
        "M1",
        &Intent {
            go: true,
            min_tier: Some("high".into()),
            ..Intent::default()
        },
    )
    .unwrap();
    s.mark_escalated("M1").unwrap();
    assert_eq!(
        scheduler::escalate(true, cur, need, &team),
        Escalation::NoEscalation,
        "the second request answers no escalation (exit 3)"
    );
    // The escalated intent sends it to the stronger coder this time.
    let mut snap2 = s.scheduler_snapshot().unwrap().unwrap();
    snap2.tasks[0].task.status = State::Pending;
    snap2.tasks[0].task.worker = None;
    let out = scheduler::schedule(&snap2, 10);
    assert_eq!(assigned_to(&out, "M1"), Some("w2"));
}

#[test]
fn shared_mode_serializes_and_still_serves_fix_jobs() {
    let (_d, mut s) = store();
    sync_run(
        &mut s,
        &[("w1", "claude", Some("medium"))],
        vec![
            plan_task("M1", Some("medium"), &["src/a.rs"], false),
            plan_task("M2", Some("medium"), &["src/b.rs"], false),
        ],
        true,
    );
    go(&mut s, "M1");
    go(&mut s, "M2");
    s.claim("M1", "w1", None).unwrap();
    s.transition(
        "M1",
        &TransitionRequest::manual(State::Conflict, Some("merge"), true),
    )
    .unwrap();
    s.release_claim("M1").unwrap();
    let out = scheduler::schedule(&s.scheduler_snapshot().unwrap().unwrap(), 10);
    assert_eq!(
        out.assignments
            .iter()
            .map(|a| (a.task.as_str(), a.worker.as_str(), a.job))
            .collect::<Vec<_>>(),
        [("M1", "w1", Job::Fix)],
        "the conflict job gets the shared mode's one slot"
    );
    assert_eq!(
        hold_of(&out, "M2"),
        Some(&Hold::NoSlots(1)),
        "M2 waits for the slot, not for the files"
    );
}

#[test]
fn the_new_task_path_still_works_alongside_synced_plans() {
    let (_d, mut s) = store();
    sync_run(&mut s, &[("w1", "claude", Some("medium"))], vec![], false);
    s.add_task(&NewTask {
        id: "M1".into(),
        title: "one".into(),
        deps: vec![],
    })
    .unwrap();
    go(&mut s, "M1");
    let out = scheduler::schedule(&s.scheduler_snapshot().unwrap().unwrap(), 10);
    assert_eq!(
        out.assignments.len(),
        1,
        "a task without plan fields is schedulable"
    );
    assert_eq!(assigned_to(&out, "M1"), Some("w1"));
}
