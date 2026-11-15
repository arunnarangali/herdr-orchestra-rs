//! The chat endpoints against a real run database, over real sockets with the real router.

use orch_api::{Api, AppState, Token, build};
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


async fn get(srv: &Server, path: &str) -> (u16, Value) {
    req(srv, "GET", path, None).await
}

fn setup() -> (tempfile::TempDir, Store) {
    let d = tempfile::tempdir().unwrap();
    let s = run(d.path());
    (d, s)
}

#[tokio::test]
async fn post_stores_a_developer_message_queued_and_types_into_nothing() {
    let (d, _s) = setup();
    let srv = start(d.path()).await;
    let (st, m) = post(&srv, "/api/v1/chat", json!({"kind": "message", "text": "hello lead"})).await;
    assert_eq!(st, 201, "{m}");
    assert_eq!(
        (m["id"].clone(), m["sender"].clone(), m["kind"].clone(), m["delivery"].clone()),
        (json!(1), json!("developer"), json!("message"), json!("queued"))
    );
    assert!(m["ts"].as_str().unwrap().ends_with('Z'));
    assert_eq!((m["request"].clone(), m["about"].clone()), (Value::Null, Value::Null));

    let (st, list) = get(&srv, "/api/v1/chat").await;
    assert_eq!(st, 200);
    assert_eq!(list["messages"].as_array().unwrap().len(), 1);
    assert_eq!(list["messages"][0], m, "the stored message is the one answered");
}

#[tokio::test]
async fn feature_request_and_plan_approval_are_special_messages() {
    let (d, _s) = setup();
    let srv = start(d.path()).await;
    let request = json!({
        "feature": "Dark mode", "why": "eyes", "must": "persist", "must_not": "touch the API",
        "decided": "CSS variables", "done_when": "toggle works", "mode": "worktree"
    });
    let (st, m) = post(&srv, "/api/v1/chat", json!({"kind": "feature_request", "text": "", "request": request})).await;
    assert_eq!(st, 201, "{m}");
    assert_eq!(m["request"], request, "the fields stay separate values");
    assert_eq!(m["kind"], "feature_request");
    let (st, m) = post(&srv, "/api/v1/chat", json!({"kind": "plan_approval", "text": "", "request": null})).await;
    assert_eq!((st, m["kind"].clone(), m["delivery"].clone()), (201, json!("plan_approval"), json!("queued")));

    let msg = refused(&post(&srv, "/api/v1/chat", json!({"kind": "feature_request", "text": "x"})).await, 400, "bad_request");
    assert!(msg.contains("request"), "{msg}");
    refused(&post(&srv, "/api/v1/chat", json!({"kind": "shout", "text": "x"})).await, 400, "bad_request");
    refused(&post(&srv, "/api/v1/chat", json!({"kind": "message", "text": "  "})).await, 400, "bad_request");
    refused(&post(&srv, "/api/v1/chat", json!({"kind": "message", "text": "x", "request": request})).await, 400, "bad_request");
    let (_, list) = get(&srv, "/api/v1/chat").await;
    assert_eq!(list["messages"].as_array().unwrap().len(), 2, "refusals store nothing");
}

#[tokio::test]
async fn the_conversation_is_in_order_and_after_returns_only_newer() {
    let (d, mut s) = setup();
    let srv = start(d.path()).await;
    for i in 0..3 {
        let (st, _) = post(&srv, "/api/v1/chat", json!({"kind": "message", "text": format!("q{i}")})).await;
        assert_eq!(st, 201);
    }
    s.post_lead_reply("answer", Some("M1")).unwrap();
    let (_, all) = get(&srv, "/api/v1/chat").await;
    let ids: Vec<i64> = all["messages"].as_array().unwrap().iter().map(|m| m["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, [1, 2, 3, 4]);
    let (_, newer) = get(&srv, "/api/v1/chat?after=3").await;
    assert_eq!(newer["messages"].as_array().unwrap().len(), 1);
    assert_eq!(newer["messages"][0]["sender"], "lead");
    assert_eq!(newer["messages"][0]["about"], "M1");
    assert_eq!(newer["messages"][0]["delivery"], Value::Null);
    let (_, none) = get(&srv, "/api/v1/chat?after=4").await;
    assert!(none["messages"].as_array().unwrap().is_empty());
    // the lead's reply acknowledged nothing here (nothing was sent), the questions are still queued
    assert_eq!(all["messages"][0]["delivery"], "queued");
}

#[tokio::test]
async fn no_lead_pane_is_lead_unavailable_and_the_message_is_kept() {
    let (d, mut s) = setup();
    let srv = start(d.path()).await;
    // the event manager is running and saw the lead's pane gone
    let _lease = take_lease(&driver_lock_path(&d.path().join(".orchestrate"))).unwrap();
    s.set_chat_lead_pane("gone", 1_700_000_000).unwrap();
    let r = post(&srv, "/api/v1/chat", json!({"kind": "message", "text": "anyone there?"})).await;
    let msg = refused(&r, 409, "lead_unavailable");
    assert!(msg.contains("orch lead run") && msg.contains("`lead`"), "{msg}");
    assert!(msg.contains("message #1 is stored"), "{msg}");
    let (_, list) = get(&srv, "/api/v1/chat").await;
    assert_eq!(list["messages"].as_array().unwrap().len(), 1, "kept");
    assert_eq!(list["messages"][0]["delivery"], "queued");
}

#[tokio::test]
async fn a_run_without_a_lead_says_how_to_start_one() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join(".orchestrate");
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
            lead_mode: "off".into(),
            reviewer: None,
        },
        agents: vec![agent(AgentRole::Worker, "claude-1", "claude", "sonnet", "medium")],
        tasks: vec![plan_task("M1", "One", &[])],
    })
    .unwrap();
    let srv = start(d.path()).await;
    let msg = refused(&post(&srv, "/api/v1/chat", json!({"kind": "message", "text": "hi"})).await, 409, "lead_unavailable");
    assert!(msg.contains("orch lead plan"), "{msg}");
    let (_, list) = get(&srv, "/api/v1/chat").await;
    assert_eq!(list["messages"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_api_token_and_other_secrets_are_never_stored() {
    let (d, s) = setup();
    let srv = start(d.path()).await;
    let text = format!("my token is {SECRET} and Bearer abcdef0123456789abcdef");
    let (st, m) = post(&srv, "/api/v1/chat", json!({"kind": "message", "text": text})).await;
    assert_eq!(st, 201);
    let stored = m["text"].as_str().unwrap();
    assert!(!stored.contains(SECRET) && !stored.contains("abcdef0123456789"), "{stored}");
    let raw = s.chat_messages(None, 10).unwrap();
    assert!(!raw[0].text.contains(SECRET));
}

#[tokio::test]
async fn no_run_is_404_and_every_route_needs_the_token() {
    let d = tempfile::tempdir().unwrap();
    let srv = start(d.path()).await;
    refused(&get(&srv, "/api/v1/chat").await, 404, "no_run");
    refused(&post(&srv, "/api/v1/chat", json!({"kind": "message", "text": "x"})).await, 404, "no_run");
    let mut s = TcpStream::connect(srv.addr).await.unwrap();
    let text = format!("GET /api/v1/chat HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", srv.addr);
    s.write_all(text.as_bytes()).await.unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).await.unwrap();
    assert!(raw.starts_with("HTTP/1.1 401"), "{raw}");
}

#[tokio::test]
async fn there_is_no_route_to_any_agent_but_the_lead() {
    let (d, _s) = setup();
    let srv = start(d.path()).await;
    for path in ["/api/v1/chat/claude-1", "/api/v1/agents/claude-1/chat", "/api/v1/agents/claude-1/send"] {
        let (st, _) = post(&srv, path, json!({"kind": "message", "text": "x"})).await;
        assert!(st == 404 || st == 405, "{path} answered {st}");
    }
}

#[tokio::test]
async fn openapi_documents_chat() {
    let (d, _s) = setup();
    let srv = start(d.path()).await;
    let (_, doc) = get(&srv, "/api/v1/openapi.json").await;
    let chat = &doc["paths"]["/api/v1/chat"];
    assert_eq!(chat["get"]["operationId"], "chat");
    assert_eq!(chat["post"]["operationId"], "sendChat");
    assert!(chat["post"]["responses"]["201"].is_object() && chat["post"]["responses"]["409"].is_object());
    for schema in ["ChatBody", "ChatMessageView", "ChatListResponse", "ChatRequest"] {
        assert!(doc["components"]["schemas"][schema].is_object(), "{schema}");
    }
}
