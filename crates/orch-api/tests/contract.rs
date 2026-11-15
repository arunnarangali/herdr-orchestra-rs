//! The contract between this API and the desktop GUI's TypeScript types.
//!
//! Every response shape is built here as a Rust value (a struct literal, so adding, removing
//! or renaming a field stops this file compiling until the sample is updated) and serialised.
//! The JSON is committed under `gui/src/api/contract/`; the GUI's typecheck fails when a
//! committed sample no longer fits its TypeScript type, and this test fails when a committed
//! sample no longer matches the shape the server really produces. After a deliberate change
//! to a shape, regenerate with `ORCH_UPDATE_CONTRACT=1 cargo test -p orch-api --test contract`
//! and then fix the TypeScript types the typecheck points at.

use orch_api::models::*;
use orch_api::routes::actions::{
    ActionResponse, AnswerBody, DispatchBody, EscalateBody, FixBody, RunControlResponse,
    SetStatusBody, TaskAfter,
};
use orch_api::routes::chat::{ChatBody, ChatListResponse, ChatMessageView, ChatRequest};
use orch_api::routes::stream::{
    AttentionEvent, AttentionOp, LeadQueueEvent, LogEvent, RunStateEvent, StreamMessage,
    TaskStateEvent,
};
use orch_api::routes::system::Health;
use orch_api::{AppState, ErrorBody, Token, build};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../gui/src/api/contract")
}

fn s(v: &str) -> String {
    v.to_string()
}

fn some(v: &str) -> Option<String> {
    Some(v.to_string())
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn to_value<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap()
}

fn broken() -> BrokenInfo {
    BrokenInfo {
        reason: s("the run branch fails its check"),
        since: 1_700_000_300,
    }
}

fn task_info() -> TaskInfo {
    TaskInfo {
        id: s("M1"),
        title: s("Foundation"),
        state: s("awaiting-review"),
        reason: s("check passed"),
        deps: strings(&["M0"]),
        worker: some("rs-claude-1"),
        last_worker: some("rs-claude-1"),
        model: some("claude-sonnet-5-5"),
        tier: some("medium"),
        needs: strings(&["coding", "reasoning"]),
        ready: false,
        attempt: 2,
        fix_rounds: 1,
        review_rounds: 1,
        conflict_rounds: 0,
        lost: 0,
        escalated: false,
    }
}

fn finding() -> Finding {
    Finding {
        file: s("src/lib.rs"),
        line: some("12-14"),
        problem: s("the token is logged"),
        why: s("secrets must never reach a log"),
        criterion: some("AC4"),
    }
}

fn event() -> EventInfo {
    EventInfo {
        id: 7,
        ts: 1_700_000_100,
        kind: s("needs-answer"),
        task: some("M1"),
        task_title: some("Foundation"),
        text: s("M1 needs an answer"),
        agent: None,
        prompt: None,
        read: false,
        open: true,
        help: s("run `orch answer M1 \"…\"`"),
    }
}

fn status() -> StatusResponse {
    StatusResponse {
        title: s("Desktop app"),
        branch: s("orch/desktop"),
        base: some("main"),
        isolation: s("worktree"),
        max_parallel: 2,
        reviewer: some("rs-codex-1"),
        lead_mode: s("manual"),
        run_state: s("running"),
        all_done: false,
        stuck: false,
        broken_branch: Some(broken()),
        event_manager: EventManagerInfo {
            running: true,
            holder: some("orch autopilot (pid 42, started 1700000000)"),
        },
        events: EventCounts { unread: 1, open: 2 },
        progress: vec![StateCount {
            state: s("pending"),
            count: 3,
        }],
        workers: vec![WorkerInfo {
            name: s("rs-claude-1"),
            kind: s("claude"),
            model: some("claude-sonnet-5-5"),
            tier: some("medium"),
            detect: true,
            task: some("M1"),
        }],
        tasks: vec![TaskBrief {
            id: s("M1"),
            title: s("Foundation"),
            state: s("coding"),
            deps: strings(&["M0"]),
            worker: some("rs-claude-1"),
            tier: some("medium"),
            reason: s("dispatched"),
        }],
    }
}

fn plan() -> PlanResponse {
    PlanResponse {
        title: some("Desktop app"),
        header: PlanHeader {
            branch: some("orch/desktop"),
            base: some("main"),
            check_command: some("cargo test"),
            setup_command: some("npm ci"),
            isolation: some("worktree"),
            max_parallel: some("2"),
            scope: some("warn"),
            budget: some("none"),
            commits: some("per-milestone"),
            lead: some("claude"),
            lead_mode: some("manual"),
            workers: some("rs-claude-1"),
            reviewer: some("rs-codex-1"),
            risk_hold: some("on"),
            setup_inputs: some("package-lock.json"),
            extra: vec![strings(&["Owner", "the team"])],
        },
        feature_breakdown: s("1. the app"),
        milestones: vec![PlanMilestone {
            id: s("M1"),
            title: s("Foundation"),
            enabled: true,
            goal: s("an app that starts"),
            why: s("every screen needs it"),
            why_separate: some("it is the foundation"),
            depends_on: vec![PlanDependency {
                id: s("M0"),
                reason: some("needs the API"),
            }],
            prefer: some("rs-claude-1"),
            tier: some("medium"),
            needs: strings(&["coding"]),
            model_why: some("sets the structure"),
            estimate: some("75m"),
            role: some("front-end engineer"),
            review: some("yes"),
            parallel: false,
            likely_files: strings(&["gui/"]),
            acceptance_criteria: vec![PlanCriterion {
                id: s("AC1"),
                text: s("it builds"),
            }],
            constraints: s("no remote fonts"),
            interfaces: s("gui/src/api"),
            context: s("see the plan"),
            out_of_scope: s("screens"),
            open_questions: s("none"),
            has_open_questions: false,
            notes_from_user: s("be careful"),
        }],
    }
}

fn tasks() -> TasksResponse {
    TasksResponse {
        tasks: vec![task_info()],
    }
}

fn task_detail() -> TaskDetailResponse {
    TaskDetailResponse {
        task: task_info(),
        history: vec![HistoryEntry {
            ts: 1_700_000_000,
            from: s("pending"),
            to: s("coding"),
            why: s("dispatched"),
            worker: some("rs-claude-1"),
            model: some("claude-sonnet-5-5"),
            attempt: 1,
            forced: false,
        }],
        checks: vec![CheckInfo {
            place: s("worktree"),
            ok: true,
            secs: 12.5,
            ts: 1_700_000_050,
        }],
        reviews: vec![ReviewInfo {
            round: 1,
            error: None,
            outcome: some("changes-requested"),
            criteria: vec![CriterionVerdict {
                id: s("AC1"),
                met: false,
                note: s("missing a test"),
            }],
            blockers: vec![finding()],
            suggestions: vec![finding()],
        }],
        results: vec![CoderResult {
            attempt: 1,
            error: None,
            status: some("done"),
            commit: some("feat(gui): add the app"),
            files_changed: strings(&["gui/package.json"]),
            check: Some(ResultCheck {
                ran: true,
                passed: true,
                summary: s("42 tests pass"),
            }),
            criteria: vec![ResultCriterion {
                id: s("AC1"),
                met: true,
                evidence: s("test passes"),
            }],
            questions: strings(&["which port?"]),
        }],
        files_changed: strings(&["gui/package.json"]),
        files_outside_likely: strings(&["docs/x.md"]),
    }
}

fn events() -> EventsResponse {
    EventsResponse {
        lead_on: true,
        new: vec![event()],
        still_open: vec![event()],
        ready: vec![ReadyInfo {
            task: s("M2"),
            title: s("Overview"),
            tier: some("small"),
            workers: strings(&["rs-claude-1"]),
        }],
    }
}

fn report() -> ReportResponse {
    ReportResponse {
        branch: s("orch/desktop"),
        merged: 1,
        total: 2,
        failed: 0,
        conflicts: 0,
        lost: 0,
        fix_rounds: 1,
        broken_branch: Some(broken()),
        started: Some(1_700_000_000),
        finished: Some(1_700_003_600),
        elapsed_s: Some(3600),
        first_pass: 1,
        first_pass_rate: Some(0.5),
        wasted: WastedWork {
            lost_attempts: 0,
            fix_rounds: 1,
            conflict_rounds: 0,
            extra_attempts: 1,
        },
        tasks: vec![ReportRowInfo {
            id: s("M1"),
            state: s("merged"),
            tries: 2,
            fix_rounds: 1,
            review_rounds: 1,
            conflict_rounds: 0,
            lost: 0,
            planned_tier: some("medium"),
            model: some("claude-sonnet-5-5"),
            actual_tier: some("medium"),
            below_planned_tier: false,
            started: Some(1_700_000_000),
            finished: Some(1_700_001_800),
            seconds: Some(1800),
        }],
        models: vec![ModelUse {
            model: s("claude-sonnet-5-5"),
            tier: some("medium"),
            tasks: 1,
            merged: 1,
            tries: 2,
            fix_rounds: 1,
            review_rounds: 1,
            conflict_rounds: 0,
            lost: 0,
        }],
    }
}

fn agents() -> AgentsResponse {
    AgentsResponse {
        agents: vec![AgentKindInfo {
            kind: s("claude"),
            display_name: some("Claude Code"),
            installed: true,
            roles: strings(&["worker", "lead"]),
            herdr_detect: true,
            verified: true,
            launch: s("claude"),
            skills: SkillsInfo {
                status: s("outdated"),
                problems: strings(&["~/.claude/skills/orch-run/SKILL.md"]),
            },
            login: LoginInfo {
                state: s("authenticated"),
                reason: s("credentials file found"),
            },
        }],
        team: vec![TeamMember {
            name: s("rs-claude-1"),
            role: s("worker"),
            kind: s("claude"),
            model: some("claude-sonnet-5-5"),
            tier: some("medium"),
            detect: true,
            working_on: Some(WorkingOn {
                task: s("M1"),
                state: s("coding"),
            }),
        }],
    }
}

fn models() -> ModelsResponse {
    ModelsResponse {
        models: vec![ModelInfo {
            id: s("claude-sonnet-5-5"),
            tier: some("medium"),
            kinds: strings(&["claude"]),
            reasoning: Some(8),
            coding: Some(9),
            architecture: Some(8),
            instructions: Some(9),
            tools: Some(9),
            planning: Some(8),
            speed: Some(7),
            context_k: Some(200),
            open_weights: Some(false),
            evidence: some("vendor benchmark"),
            denied: true,
            denied_by: some("*-preview"),
        }],
        tiers: vec![TierInfo {
            name: s("medium"),
            reasoning_floor: 6,
            purpose: some("most milestones"),
        }],
        policy: PolicyInfo {
            deny: strings(&["*-preview"]),
            deny_why: some("unstable"),
        },
    }
}

fn logs() -> LogsResponse {
    LogsResponse {
        lines: strings(&["12:00:01 dispatched M1", "12:00:05 M1 -> coding"]),
    }
}

fn health() -> Health {
    Health {
        status: s("ok"),
        version: s("5.0.0"),
        project: s("/home/me/project"),
        run: true,
    }
}

fn action() -> ActionResponse {
    ActionResponse {
        action: s("dispatch"),
        tasks: vec![TaskAfter {
            id: s("M1"),
            title: s("Foundation"),
            state: s("pending"),
            worker: some("rs-claude-1"),
        }],
        event_manager_running: false,
        waiting: true,
        message: s("recorded; the event manager is not running"),
    }
}

fn run_control() -> RunControlResponse {
    RunControlResponse {
        action: s("pause"),
        run_control: s("paused"),
        event_manager_running: true,
        waiting: false,
        message: s("no new work starts"),
    }
}

fn chat_request() -> ChatRequest {
    ChatRequest {
        feature: s("Dark mode"),
        why: s("easier on the eyes at night"),
        must: s("remember the choice"),
        must_not: s("change the API"),
        decided: s("CSS variables"),
        done_when: s("the toggle works and persists"),
        mode: s("worktree"),
    }
}

fn chat_message() -> ChatMessageView {
    ChatMessageView {
        id: 3,
        ts: s("2023-11-14T22:13:20Z"),
        sender: s("developer"),
        kind: s("feature_request"),
        text: s("Feature request: Dark mode"),
        request: Some(chat_request()),
        about: None,
        delivery: some("sent"),
        delivery_note: some("typed into the lead's pane"),
    }
}

fn chat_reply() -> ChatMessageView {
    ChatMessageView {
        id: 4,
        ts: s("2023-11-14T22:14:00Z"),
        sender: s("lead"),
        kind: s("message"),
        text: s("M1 is running"),
        request: None,
        about: some("M1"),
        delivery: None,
        delivery_note: None,
    }
}

fn error_body() -> ErrorBody {
    ErrorBody {
        error: orch_api::error::ErrorDetail {
            code: s("not_found"),
            message: s("no milestone \"M9\" in this run"),
            request_id: s("req-1a2b-0"),
        },
    }
}

fn stream_message() -> StreamMessage {
    StreamMessage {
        id: 12,
        event: s("task-state"),
        data: json!({"id": "M1", "to": "coding"}),
    }
}

fn task_state_event() -> TaskStateEvent {
    TaskStateEvent {
        id: s("M1"),
        task: s("M1"),
        task_id: some("M1"),
        from: s("pending"),
        to: s("coding"),
        status: s("coding"),
        why: s("dispatched"),
        reason: some("dispatched"),
        worker: some("rs-claude-1"),
        agent: some("rs-claude-1"),
        model: some("claude-sonnet-5-5"),
        attempt: 1,
        forced: false,
        ts: 1_700_000_000,
    }
}

fn lead_event() -> LeadQueueEvent {
    LeadQueueEvent {
        id: 7,
        ts: 1_700_000_100,
        kind: s("needs-answer"),
        task: some("M1"),
        task_id: some("M1"),
        text: s("M1 needs an answer"),
        agent: None,
        prompt: None,
        read_at: Some(1_700_000_200),
    }
}

fn run_state_event() -> RunStateEvent {
    RunStateEvent {
        action: some("pause"),
        message: some("run control: paused (api)"),
        event: some("run-control"),
        task: some("M1"),
        agent: some("rs-claude-1"),
        status: some("paused"),
        run_control: some("paused"),
        state: some("paused"),
        source: some("api"),
        ts: Some(1_700_000_300),
    }
}

fn log_event() -> LogEvent {
    LogEvent {
        message: s("dispatched M1"),
        line: s("dispatched M1"),
        ts: 1_700_000_400,
    }
}

fn attention() -> AttentionResponse {
    AttentionResponse {
        items: vec![AttentionItem {
            id: s("blocked:1"),
            kind: AttentionKind::Blocked,
            task: some("M1"),
            agent: some("rs-claude-1"),
            title: s("rs-claude-1 is blocked on permission"),
            detail: some("needs bash permission"),
            prompt: some("May I run `cargo test`?"),
            since: s("2023-11-14T22:13:20Z"),
            actions: vec![AttentionAction::OpenChat],
        }],
    }
}

fn agent_output() -> AgentOutputResponse {
    AgentOutputResponse {
        agent: s("rs-claude-1"),
        lines: 2,
        text: s("line 1\nline 2"),
        ts: s("2023-11-14T22:13:20Z"),
    }
}

fn attention_event() -> AttentionEvent {
    AttentionEvent {
        op: AttentionOp::Add,
        item: AttentionItem {
            id: s("blocked:1"),
            kind: AttentionKind::Blocked,
            task: some("M1"),
            agent: some("rs-claude-1"),
            title: s("rs-claude-1 is blocked on permission"),
            detail: some("needs bash permission"),
            prompt: some("May I run `cargo test`?"),
            since: s("2023-11-14T22:13:20Z"),
            actions: vec![AttentionAction::OpenChat],
        },
    }
}

/// Response shapes, by file name.
fn responses() -> Vec<(&'static str, Value)> {
    vec![
        ("Health", to_value(&health())),
        ("ErrorBody", to_value(&error_body())),
        ("StatusResponse", to_value(&status())),
        ("PlanResponse", to_value(&plan())),
        ("TasksResponse", to_value(&tasks())),
        ("TaskDetailResponse", to_value(&task_detail())),
        ("EventsResponse", to_value(&events())),
        ("ReportResponse", to_value(&report())),
        ("AgentsResponse", to_value(&agents())),
        ("ModelsResponse", to_value(&models())),
        ("LogsResponse", to_value(&logs())),
        ("ActionResponse", to_value(&action())),
        ("RunControlResponse", to_value(&run_control())),
        ("ChatMessageView", to_value(&chat_message())),
        ("ChatListResponse", to_value(&ChatListResponse { messages: vec![chat_message(), chat_reply()] })),
        ("StreamMessage", to_value(&stream_message())),
        ("TaskStateEvent", to_value(&task_state_event())),
        ("LeadQueueEvent", to_value(&lead_event())),
        ("RunStateEvent", to_value(&run_state_event())),
        ("LogEvent", to_value(&log_event())),
        ("AttentionResponse", to_value(&attention())),
        ("AgentOutputResponse", to_value(&agent_output())),
        ("AttentionEvent", to_value(&attention_event())),
    ]
}

/// Request bodies, by file name. They are only read by the server, so each committed sample
/// must parse into the real type and give back the values the sample is written with.
fn check_requests() -> Vec<&'static str> {
    let read = |name: &str| -> String {
        std::fs::read_to_string(dir().join(format!("{name}.json")))
            .unwrap_or_else(|e| panic!("{name}.json is missing ({e}): {HOW}"))
    };
    let a: AnswerBody = serde_json::from_str(&read("AnswerBody")).unwrap();
    assert_eq!(a.text, "use port 8080");
    let d: DispatchBody = serde_json::from_str(&read("DispatchBody")).unwrap();
    assert_eq!(
        (d.ids, d.to.as_deref(), d.tier.as_deref()),
        (strings(&["M1", "M2"]), Some("rs-claude-1"), Some("medium"))
    );
    let f: FixBody = serde_json::from_str(&read("FixBody")).unwrap();
    assert_eq!(
        (f.text.as_str(), f.to.as_deref()),
        ("rename the flag", Some("rs-claude-1"))
    );
    let e: EscalateBody = serde_json::from_str(&read("EscalateBody")).unwrap();
    assert_eq!(e.reason.as_deref(), Some("needs a stronger model"));
    let st: SetStatusBody = serde_json::from_str(&read("SetStatusBody")).unwrap();
    assert_eq!(
        (st.status.as_str(), st.reason.as_deref()),
        ("paused", Some("waiting for the API"))
    );
    let c: ChatBody = serde_json::from_str(&read("ChatBody")).unwrap();
    assert_eq!(c.kind, "feature_request");
    assert_eq!(c.text, "");
    assert_eq!(c.request.unwrap().must_not, "change the API");
    vec![
        "ChatBody",
        "AnswerBody",
        "DispatchBody",
        "FixBody",
        "EscalateBody",
        "SetStatusBody",
    ]
}

const HOW: &str = "regenerate with `ORCH_UPDATE_CONTRACT=1 cargo test -p orch-api --test contract` \
                   (request samples are edited by hand)";

fn pretty(v: &Value) -> String {
    let mut text = serde_json::to_string_pretty(v).unwrap();
    text.push('\n');
    text
}

#[test]
fn committed_samples_match_the_shapes_the_server_produces() {
    let update = std::env::var_os("ORCH_UPDATE_CONTRACT").is_some();
    let mut wrong = Vec::new();
    for (name, value) in responses() {
        let path = dir().join(format!("{name}.json"));
        if update {
            std::fs::write(&path, pretty(&value)).unwrap();
            continue;
        }
        match std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        {
            Some(committed) if committed == value => {}
            Some(_) => wrong.push(format!("{name}.json differs from what the server produces")),
            None => wrong.push(format!("{name}.json is missing or not JSON")),
        }
    }
    assert!(wrong.is_empty(), "{}\n{HOW}", wrong.join("\n"));
}

#[test]
fn no_stale_samples_are_committed() {
    let mut known: BTreeSet<String> = responses()
        .into_iter()
        .map(|(n, _)| n.to_string())
        .collect();
    known.extend(check_requests().into_iter().map(String::from));
    let on_disk: BTreeSet<String> = std::fs::read_dir(dir())
        .unwrap()
        .filter_map(|e| {
            let name = e.unwrap().file_name().into_string().unwrap();
            name.strip_suffix(".json").map(String::from)
        })
        .collect();
    assert_eq!(
        on_disk, known,
        "samples on disk vs samples known to this test"
    );
}

/// Every schema a documented route answers with or accepts has a sample, so a new route
/// cannot be added without extending the contract.
#[test]
fn every_documented_body_has_a_sample() {
    let dir_ = tempfile::tempdir().unwrap();
    let api = build(AppState::new(
        dir_.path().to_path_buf(),
        Token::new("x".repeat(64)),
    ));
    let doc = serde_json::to_value(&api.openapi).unwrap();
    let mut known: BTreeSet<String> = responses()
        .into_iter()
        .map(|(n, _)| n.to_string())
        .collect();
    known.extend(check_requests().into_iter().map(String::from));
    // The OpenAPI document describes itself; the client types it loosely.
    known.insert(s("OpenApi"));
    assert!(doc["openapi"].is_string() && doc["paths"].is_object());

    let mut used = BTreeSet::new();
    for (path, item) in doc["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            let refs = op["responses"]["200"]["content"]
                .as_object()
                .into_iter()
                .chain(op["requestBody"]["content"].as_object())
                .flat_map(|c| c.values())
                .filter_map(|c| c["schema"]["$ref"].as_str());
            for r in refs {
                let name = r.rsplit('/').next().unwrap().to_string();
                assert!(
                    known.contains(&name),
                    "{method} {path} uses `{name}`, which has no contract sample"
                );
                used.insert(name);
            }
        }
    }
    // And the other way: the samples above describe routes that exist.
    for (name, _) in responses() {
        // The error body is every route's non-200 answer; the stream payloads are the `data`
        // of the stream's messages.
        let stream_part = [
            "ErrorBody",
            "ChatMessageView",
            "TaskStateEvent",
            "LeadQueueEvent",
            "RunStateEvent",
            "LogEvent",
            "AttentionEvent",
        ];
        assert!(
            used.contains(name) || stream_part.contains(&name),
            "`{name}` is not used by any route"
        );
    }
}
