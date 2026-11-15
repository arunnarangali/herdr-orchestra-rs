//! The action endpoints against a real run database, over real sockets with the real router.

use orch_api::{Api, AppState, Token, build};
use orch_core::actions::RunControl;
use orch_core::lead_queue::LeadKind;
use orch_core::lease::{driver_lock_path, take_lease};
use orch_core::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::store::{Store, TransitionRequest};
use orch_core::task::State;
use serde_json::{Value, json};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

const PLAN: &str = "\
# Plan: Demo
Branch: feature/demo
Check command: cargo test
Workers: claude-1 (claude: sonnet), codex-1 (codex: gpt-5)
Lead: lead (claude: opus)
Reviewer: none

## [x] M1: One
Depends on: none
Tier: medium
Review: no
Likely files: src/a.rs
Goal:
- make a
Acceptance criteria:
- works

## [x] M2: Two
Depends on: M1
Tier: medium
Goal:
- make b
Acceptance criteria:
- works

## [x] M3: Three
Depends on: M1
Tier: medium
Goal:
- g
Acceptance criteria:
- works
Notes from user:
- earlier note

## [x] M4: Four
Depends on: M1
Tier: medium
Goal:
- g
Acceptance criteria:
- works

## [x] M5: Five
Depends on: M1
Tier: medium
Goal:
- g
Acceptance criteria:
- works
";

fn agent(role: AgentRole, name: &str, kind: &str, model: &str, tier: &str) -> RunAgent {
    RunAgent {
        role,
        name: name.into(),
        kind: kind.into(),
        model: Some(model.into()),
        tier: Some(tier.into()),
        detect: true,
        pane: None,
    }
}

fn plan_task(id: &str, title: &str, deps: &[&str]) -> PlanTask {
    PlanTask {
        id: id.into(),
        title: title.into(),
        deps: deps.iter().map(|d| d.to_string()).collect(),
        enabled: true,
        open_questions: false,
        tier: Some("medium".into()),
        needs: vec![],
        model_why: String::new(),
        prefer: "any".into(),
        review: false,
        files: vec![],
        parallel: false,
    }
}

fn go(s: &mut Store, id: &str, to: &[State]) {
    for t in to {
        s.transition(id, &TransitionRequest::auto(*t, Some("test")))
            .unwrap();
    }
}

/// M1 merged (by claude-1), M2 held, M3 paused (last worker claude-1), M4 pending, M5 failed.
fn run(root: &Path) -> Store {
    let dir = root.join(".orchestrate");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plan.md"), PLAN).unwrap();
    let mut s = Store::open_in(&dir).unwrap();
    s.apply_sync(&SyncInput {
        settings: RunSettings {
            branch: "feature/demo".into(),
            base: Some("main".into()),
            title: "Demo".into(),
            commits: "squash".into(),
            isolation: "worktree".into(),
            max_parallel: 2,
            setup_inputs: vec![],
            scope: "warn".into(),
            risk_hold: true,
            budget_s: None,
            check_command: "cargo test".into(),
            setup_command: String::new(),
            lead_mode: "events".into(),
            reviewer: None,
        },
        agents: vec![
            agent(AgentRole::Worker, "claude-1", "claude", "sonnet", "medium"),
            agent(AgentRole::Worker, "codex-1", "codex", "gpt-5", "high"),
            agent(AgentRole::Lead, "lead", "claude", "opus", "high"),
        ],
        tasks: vec![
            plan_task("M1", "One", &[]),
            plan_task("M2", "Two", &["M1"]),
            plan_task("M3", "Three", &["M1"]),
            plan_task("M4", "Four", &["M1"]),
            plan_task("M5", "Five", &["M1"]),
        ],
    })
    .unwrap();
    s.assign_worker("M1", Some("claude-1"), Some("sonnet"))
        .unwrap();
    go(
        &mut s,
        "M1",
        &[State::Coding, State::Verifying, State::Merged],
    );
    s.assign_worker("M1", None, None).unwrap();
    s.assign_worker("M2", Some("codex-1"), Some("gpt-5"))
        .unwrap();
    go(
        &mut s,
        "M2",
        &[State::Coding, State::Verifying, State::Held],
    );
    s.assign_worker("M3", Some("claude-1"), Some("sonnet"))
        .unwrap();
    go(&mut s, "M3", &[State::Coding]);
    s.manual_set("M3", State::Paused, "needs a decision", false)
        .unwrap();
    s.assign_worker("M5", Some("claude-1"), Some("sonnet"))
        .unwrap();
    go(&mut s, "M5", &[State::Failed]);
    s
}

struct Server {
    addr: std::net::SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        self.task.abort();
    }
}

async fn start(root: &Path) -> Server {
    let state = AppState::new(root.to_path_buf(), Token::new(SECRET));
    let listener = orch_api::bind("127.0.0.1", 0).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel::<()>();
    let Api { router, .. } = build(state);
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await
    });
    Server {
        addr,
        stop: Some(tx),
        task,
    }
}

async fn req(srv: &Server, method: &str, path: &str, body: Option<&str>) -> (u16, Value) {
    let b = body.unwrap_or("");
    let ct = if body.is_some() {
        "Content-Type: application/json\r\n"
    } else {
        ""
    };
    let text = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAuthorization: Bearer {SECRET}\r\n{ct}Content-Length: {}\r\n\r\n{b}",
        srv.addr,
        b.len()
    );
    let mut s = TcpStream::connect(srv.addr).await.unwrap();
    s.write_all(text.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let raw = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        let mut out = String::new();
        let mut rest = body;
        while let Some((len, tail)) = rest.split_once("\r\n") {
            let n = usize::from_str_radix(len.trim(), 16).unwrap_or(0);
            if n == 0 {
                break;
            }
            out.push_str(&tail[..n]);
            rest = &tail[n + 2..];
        }
        out
    } else {
        body.to_string()
    };
    (
        status,
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("{path}: {e}: {body}")),
    )
}

async fn post(srv: &Server, path: &str, body: Value) -> (u16, Value) {
    req(srv, "POST", path, Some(&body.to_string())).await
}

fn refused(r: &(u16, Value), status: u16, code: &str) -> String {
    assert_eq!(r.0, status, "{}", r.1);
    assert_eq!(r.1["error"]["code"], code, "{}", r.1);
    r.1["error"]["message"].as_str().unwrap().to_string()
}

fn state_of(s: &Store, id: &str) -> State {
    s.get_task(id).unwrap().status
}

#[tokio::test]
async fn answer_reaches_the_plan_and_the_leads_queue() {
    let dir = tempfile::tempdir().unwrap();
    let store = run(dir.path());
    let srv = start(dir.path()).await;

    refused(
        &post(&srv, "/api/v1/tasks/M99/answer", json!({"text": "x"})).await,
        404,
        "not_found",
    );
    refused(
        &post(&srv, "/api/v1/tasks/M3/answer", json!({"text": "   "})).await,
        400,
        "bad_request",
    );
    refused(
        &post(&srv, "/api/v1/tasks/M3/answer", json!({})).await,
        400,
        "bad_request",
    );
    let msg = refused(
        &post(&srv, "/api/v1/tasks/M2/answer", json!({"text": "yes"})).await,
        409,
        "conflict",
    );
    assert!(msg.contains("M2 is held, not paused"), "{msg}");
    // Nothing changed by the refusals.
    assert_eq!(state_of(&store, "M3"), State::Paused);
    assert!(
        !std::fs::read_to_string(dir.path().join(".orchestrate/plan.md"))
            .unwrap()
            .contains("use sqlite")
    );

    let (s, v) = post(
        &srv,
        "/api/v1/tasks/M3/answer",
        json!({"text": "use sqlite"}),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["action"], "answer");
    assert_eq!(v["tasks"][0]["id"], "M3");
    assert_eq!(v["tasks"][0]["state"], "pending");
    // No event manager runs in the test: the request is recorded and waiting.
    assert_eq!(
        (v["event_manager_running"].as_bool(), v["waiting"].as_bool()),
        (Some(false), Some(true))
    );
    assert!(v["message"].as_str().unwrap().contains("claude-1"));

    let plan = std::fs::read_to_string(dir.path().join(".orchestrate/plan.md")).unwrap();
    let notes = plan
        .split("## [x] M3")
        .nth(1)
        .unwrap()
        .split("## [x] M4")
        .next()
        .unwrap();
    assert!(
        notes.contains("Notes from user:")
            && notes.contains("- use sqlite")
            && notes.contains("- earlier note"),
        "{notes}"
    );
    let q = store.lead_entries().unwrap();
    assert!(q.iter().any(|e| e.kind == LeadKind::AnswerProvided
        && e.task.as_deref() == Some("M3")
        && e.text == "use sqlite"));
    let a = store.actions().unwrap();
    assert!(a.iter().any(
        |a| (a.source.as_str(), a.action.as_str(), a.task.as_deref())
            == ("api", "answer", Some("M3"))
    ));
    let why = store.history("M3").unwrap().last().unwrap().why.clone();
    assert!(why.starts_with("answered: use sqlite"), "{why}");
}

#[tokio::test]
async fn approve_moves_a_held_milestone_like_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let store = run(dir.path());
    let srv = start(dir.path()).await;
    refused(
        &post(&srv, "/api/v1/tasks/M99/approve", json!({})).await,
        404,
        "not_found",
    );
    let msg = refused(
        &req(&srv, "POST", "/api/v1/tasks/M4/approve", None).await,
        409,
        "conflict",
    );
    assert_eq!(msg, "M4 is pending, not held");
    assert_eq!(state_of(&store, "M2"), State::Held);

    let (s, v) = req(&srv, "POST", "/api/v1/tasks/M2/approve", None).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(
        (
            v["tasks"][0]["state"].as_str(),
            v["tasks"][0]["worker"].as_str()
        ),
        (Some("committed"), Some("codex-1"))
    );
    assert_eq!(state_of(&store, "M2"), State::Committed);
    let h = store.history("M2").unwrap();
    let last = h.last().unwrap();
    assert_eq!((last.from, last.to), (State::Held, State::Committed));
    assert_eq!(last.why, "approved by the developer (via the API)");
    assert!(
        store
            .actions()
            .unwrap()
            .iter()
            .any(|a| a.source == "api" && a.action == "approve")
    );
    // Approving again is refused: it is no longer held.
    refused(
        &req(&srv, "POST", "/api/v1/tasks/M2/approve", None).await,
        409,
        "conflict",
    );
}

#[tokio::test]
async fn dispatch_validates_everything_before_changing_anything() {
    let dir = tempfile::tempdir().unwrap();
    let store = run(dir.path());
    let srv = start(dir.path()).await;
    refused(
        &post(&srv, "/api/v1/dispatch", json!({"ids": []})).await,
        400,
        "bad_request",
    );
    refused(
        &post(&srv, "/api/v1/dispatch", json!({"ids": ["M99"]})).await,
        404,
        "not_found",
    );
    refused(
        &post(
            &srv,
            "/api/v1/dispatch",
            json!({"ids": ["M3"], "tier": "huge"}),
        )
        .await,
        400,
        "bad_request",
    );
    refused(
        &post(
            &srv,
            "/api/v1/dispatch",
            json!({"ids": ["M3"], "to": "nobody"}),
        )
        .await,
        400,
        "bad_request",
    );
    refused(
        &post(&srv, "/api/v1/dispatch", json!({"ids": "M3"})).await,
        400,
        "bad_request",
    );
    // One bad milestone in the list refuses the whole request.
    let msg = refused(
        &post(&srv, "/api/v1/dispatch", json!({"ids": ["M3", "M2"]})).await,
        409,
        "conflict",
    );
    assert!(msg.contains("M2 is held"), "{msg}");
    assert_eq!(state_of(&store, "M3"), State::Paused);
    assert!(store.actions().unwrap().is_empty());

    let (s, v) = post(
        &srv,
        "/api/v1/dispatch",
        json!({"ids": ["M3", "M5", "M3"], "to": "codex-1", "tier": "high"}),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let tasks = v["tasks"].as_array().unwrap();
    assert_eq!(
        tasks
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["M3", "M5"]
    );
    assert!(tasks.iter().all(|t| t["state"] == "pending"));
    assert_eq!(v["waiting"], true);
    let intent = store.intent("M3").unwrap().unwrap();
    assert!(intent.go);
    assert_eq!(
        (intent.assign.as_deref(), intent.min_tier.as_deref()),
        (Some("codex-1"), Some("high"))
    );
    assert_eq!(
        store
            .actions()
            .unwrap()
            .iter()
            .filter(|a| a.action == "dispatch")
            .count(),
        2
    );
}

#[tokio::test]
async fn fix_adds_a_follow_up_to_landed_work() {
    let dir = tempfile::tempdir().unwrap();
    let store = run(dir.path());
    let srv = start(dir.path()).await;
    refused(
        &post(&srv, "/api/v1/tasks/M99/fix", json!({"text": "x"})).await,
        404,
        "not_found",
    );
    refused(
        &post(&srv, "/api/v1/tasks/M1/fix", json!({"text": "  "})).await,
        400,
        "bad_request",
    );
    refused(
        &post(
            &srv,
            "/api/v1/tasks/M1/fix",
            json!({"text": "x", "to": "nobody"}),
        )
        .await,
        400,
        "bad_request",
    );
    let msg = refused(
        &post(&srv, "/api/v1/tasks/M2/fix", json!({"text": "x"})).await,
        409,
        "conflict",
    );
    assert!(msg.contains("not landed yet"), "{msg}");
    assert!(store.get_task("M6").is_err());

    let (s, v) = post(
        &srv,
        "/api/v1/tasks/M1/fix",
        json!({"text": "rename   the  field"}),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["tasks"][0]["id"], "M6");
    assert_eq!(v["tasks"][0]["state"], "pending");
    assert_eq!(store.get_task("M6").unwrap().deps, ["M1"]);
    let plan = std::fs::read_to_string(dir.path().join(".orchestrate/plan.md")).unwrap();
    assert!(plan.contains("## [x] M6:") && plan.contains("rename the field"));
    assert!(
        store
            .actions()
            .unwrap()
            .iter()
            .any(|a| a.source == "api" && a.action == "fix" && a.task.as_deref() == Some("M6"))
    );
}

#[tokio::test]
async fn escalate_once_then_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = run(dir.path());
    let srv = start(dir.path()).await;
    refused(
        &post(&srv, "/api/v1/tasks/M99/escalate", json!({})).await,
        404,
        "not_found",
    );
    let (s, v) = post(
        &srv,
        "/api/v1/tasks/M5/escalate",
        json!({"reason": "out of rounds"}),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["tasks"][0]["state"], "pending");
    assert!(v["message"].as_str().unwrap().contains("high"));
    assert!(store.get_task("M5").unwrap().escalated);
    assert!(
        store
            .lead_entries()
            .unwrap()
            .iter()
            .any(|e| e.kind == LeadKind::Escalated)
    );
    let why = store.history("M5").unwrap().last().unwrap().why.clone();
    assert!(
        why.contains("out of rounds") && why.contains("via the API"),
        "{why}"
    );
    // The one escalation is used.
    refused(
        &req(&srv, "POST", "/api/v1/tasks/M5/escalate", None).await,
        409,
        "conflict",
    );
}

#[tokio::test]
async fn set_status_allows_only_what_the_cli_allows() {
    let dir = tempfile::tempdir().unwrap();
    let store = run(dir.path());
    let srv = start(dir.path()).await;
    refused(
        &post(
            &srv,
            "/api/v1/tasks/M99/status",
            json!({"status": "skipped"}),
        )
        .await,
        404,
        "not_found",
    );
    refused(
        &post(&srv, "/api/v1/tasks/M4/status", json!({"status": "bogus"})).await,
        400,
        "bad_request",
    );
    let msg = refused(
        &post(&srv, "/api/v1/tasks/M4/status", json!({"status": "merged"})).await,
        400,
        "bad_request",
    );
    assert!(msg.contains("only moves a task to"), "{msg}");
    refused(
        &post(&srv, "/api/v1/tasks/M4/status", json!({})).await,
        400,
        "bad_request",
    );
    // The state machine still applies: a landed milestone cannot go back to pending.
    refused(
        &post(
            &srv,
            "/api/v1/tasks/M1/status",
            json!({"status": "pending"}),
        )
        .await,
        409,
        "conflict",
    );
    assert_eq!(state_of(&store, "M4"), State::Pending);

    let (s, v) = post(
        &srv,
        "/api/v1/tasks/M4/status",
        json!({"status": "skipped", "reason": "not needed"}),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["tasks"][0]["state"], "skipped");
    assert_eq!(
        store.history("M4").unwrap().last().unwrap().why,
        "not needed (via the API)"
    );
    assert!(
        store
            .actions()
            .unwrap()
            .iter()
            .any(|a| a.source == "api" && a.action == "set")
    );
}

#[tokio::test]
async fn run_controls_are_stored_and_say_when_they_wait() {
    let dir = tempfile::tempdir().unwrap();
    let store = run(dir.path());
    let srv = start(dir.path()).await;
    assert_eq!(store.run_control().unwrap(), RunControl::Running);

    let (s, v) = req(&srv, "POST", "/api/v1/run/pause", None).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(
        (
            v["run_control"].as_str(),
            v["waiting"].as_bool(),
            v["event_manager_running"].as_bool()
        ),
        (Some("paused"), Some(true), Some(false))
    );
    assert_eq!(store.run_control().unwrap(), RunControl::Paused);
    // A new connection to the database (a restarted event manager) still sees it.
    assert_eq!(
        Store::open_in(&dir.path().join(".orchestrate"))
            .unwrap()
            .run_control()
            .unwrap(),
        RunControl::Paused
    );

    let (_, v) = req(&srv, "POST", "/api/v1/run/resume", None).await;
    assert_eq!(v["run_control"], "running");
    assert_eq!(store.run_control().unwrap(), RunControl::Running);

    // No event manager: nothing to end, so the run is simply stopped.
    let (_, v) = req(&srv, "POST", "/api/v1/run/stop", None).await;
    assert_eq!(
        (v["run_control"].as_str(), v["waiting"].as_bool()),
        (Some("stopped"), Some(true))
    );

    // With an event manager running, stop is a request it carries out.
    let _lease = take_lease(&driver_lock_path(&dir.path().join(".orchestrate"))).unwrap();
    let (_, v) = req(&srv, "POST", "/api/v1/run/stop", None).await;
    assert_eq!(
        (
            v["run_control"].as_str(),
            v["waiting"].as_bool(),
            v["event_manager_running"].as_bool()
        ),
        (Some("stopping"), Some(false), Some(true))
    );
    assert_eq!(store.run_control().unwrap(), RunControl::Stopping);
    let (_, v) = post(&srv, "/api/v1/tasks/M2/approve", json!({})).await;
    assert_eq!(
        (v["event_manager_running"].as_bool(), v["waiting"].as_bool()),
        (Some(true), Some(false))
    );
    let acts: Vec<String> = store
        .actions()
        .unwrap()
        .into_iter()
        .filter(|a| a.source == "api")
        .map(|a| a.action)
        .collect();
    assert_eq!(acts, ["pause", "resume", "stop", "stop", "approve"]);
}

#[tokio::test]
async fn without_a_run_every_action_says_no_run_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let srv = start(dir.path()).await;
    for (path, body) in [
        ("/api/v1/tasks/M1/answer", Some(json!({"text": "x"}))),
        ("/api/v1/dispatch", Some(json!({"ids": ["M1"]}))),
        ("/api/v1/tasks/M1/fix", Some(json!({"text": "x"}))),
        ("/api/v1/tasks/M1/approve", None),
        ("/api/v1/tasks/M1/escalate", None),
        (
            "/api/v1/tasks/M1/status",
            Some(json!({"status": "skipped"})),
        ),
        ("/api/v1/run/pause", None),
        ("/api/v1/run/resume", None),
        ("/api/v1/run/stop", None),
    ] {
        let r = req(&srv, "POST", path, body.map(|b| b.to_string()).as_deref()).await;
        refused(&r, 404, "no_run");
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn actions_are_post_only_and_in_the_openapi_document() {
    let dir = tempfile::tempdir().unwrap();
    run(dir.path());
    let srv = start(dir.path()).await;
    let r = req(&srv, "GET", "/api/v1/tasks/M2/approve", None).await;
    assert_eq!(r.0, 405, "{}", r.1);
    let (_, doc) = req(&srv, "GET", "/api/v1/openapi.json", None).await;
    let paths = doc["paths"].as_object().unwrap();
    for (p, body) in [
        ("/api/v1/tasks/{id}/answer", Some("AnswerBody")),
        ("/api/v1/dispatch", Some("DispatchBody")),
        ("/api/v1/tasks/{id}/fix", Some("FixBody")),
        ("/api/v1/tasks/{id}/approve", None),
        ("/api/v1/tasks/{id}/escalate", Some("EscalateBody")),
        ("/api/v1/tasks/{id}/status", Some("SetStatusBody")),
        ("/api/v1/run/pause", None),
        ("/api/v1/run/resume", None),
        ("/api/v1/run/stop", None),
    ] {
        let post = &paths[p]["post"];
        assert!(post.is_object(), "{p}");
        for code in ["200", "400", "401", "404", "409", "500"] {
            assert!(post["responses"][code].is_object(), "{p} {code}");
        }
        match body {
            Some(b) => assert_eq!(
                post["requestBody"]["content"]["application/json"]["schema"]["$ref"],
                format!("#/components/schemas/{b}"),
                "{p}"
            ),
            None => assert!(post["requestBody"].is_null(), "{p}"),
        }
    }
    assert!(doc["components"]["schemas"]["ActionResponse"].is_object());
}
