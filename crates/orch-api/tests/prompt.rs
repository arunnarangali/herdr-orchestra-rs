//! Blocked workers' prompts, read and answered through the API over real sockets, against a
//! real run database and the in-memory fake herdr: the attention item carries the prompt
//! (also on the stream), an answer types the chosen option's keys into that worker's pane
//! and nowhere else, and every other answer is refused with nothing typed. The Lead's prompt
//! is covered in `tests/lead.rs`.

use orch_api::{Api, AppState, Token, build};
use orch_core::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::store::{Store, TransitionRequest};
use orch_core::task::State;
use orch_herdr::{AgentInfo, AgentStatus, FakeHerdr, PaneInfo, Sent};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

const PERMISSION: &str = "\
⏺ Bash(cargo test)
Do you want to proceed?
❯ 1. Yes
  2. Yes, and don't ask again for cargo test commands
  3. No, and tell Claude what to do differently (esc)
";

struct Rig {
    _dir: tempfile::TempDir,
    fake: Arc<FakeHerdr>,
    store: Store,
    addr: std::net::SocketAddr,
    stop: Option<oneshot::Sender<()>>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
    }
}

fn agent(role: AgentRole, name: &str, pane: Option<&str>) -> RunAgent {
    RunAgent {
        role,
        name: name.into(),
        kind: "claude".into(),
        model: None,
        tier: None,
        detect: true,
        pane: pane.map(String::from),
    }
}

fn task(id: &str) -> PlanTask {
    PlanTask {
        id: id.into(),
        title: id.into(),
        deps: vec![],
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

/// A run with workers `claude-1` (its pane recorded: w1:p2) and `codex-1`, the Lead `lead`,
/// and milestones M1..M4; herdr has all of them plus `stranger`, an agent of no run, each in
/// its own pane, all idle.
async fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let orch = root.join(".orchestrate");
    std::fs::create_dir_all(&orch).unwrap();
    let mut store = Store::open_in(&orch).unwrap();
    store
        .apply_sync(&SyncInput {
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
                check_command: "true".into(),
                setup_command: String::new(),
                lead_mode: "events".into(),
                reviewer: None,
            },
            agents: vec![
                agent(AgentRole::Worker, "claude-1", Some("w1:p2")),
                agent(AgentRole::Worker, "codex-1", None),
                agent(AgentRole::Lead, "lead", None),
            ],
            tasks: vec![task("M1"), task("M2"), task("M3"), task("M4")],
        })
        .unwrap();
    let fake = Arc::new(FakeHerdr::new());
    for (name, pane) in [
        ("lead", "w1:p1"),
        ("claude-1", "w1:p2"),
        ("codex-1", "w1:p3"),
        ("stranger", "w1:p9"),
    ] {
        fake.add_pane(PaneInfo::new(pane, "w1", "w1:t1"));
        fake.add_agent(AgentInfo::new(name, "claude", pane, AgentStatus::Idle));
    }
    let state = AppState::with_herdr(root, Token::new(SECRET), fake.clone());
    let listener = orch_api::bind("127.0.0.1", 0).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel::<()>();
    let Api { router, .. } = build(state);
    tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await
    });
    Rig {
        _dir: dir,
        fake,
        store,
        addr,
        stop: Some(tx),
    }
}

impl Rig {
    /// `agent`, working on milestone `id`, is blocked at `screen`; the event manager has
    /// queued it. Gives the attention item's id.
    fn block(&mut self, id: &str, agent: &str, screen: &str) -> String {
        self.store
            .transition(id, &TransitionRequest::auto(State::Coding, Some("start")))
            .unwrap();
        self.fake.set_output(agent, screen);
        self.fake.set_status(agent, AgentStatus::Blocked);
        let q = self
            .store
            .enqueue_blocked_lead_event(id, agent, screen.lines().last())
            .unwrap()
            .unwrap();
        format!("blocked:{q}")
    }

    fn typed(&self) -> Vec<Sent> {
        self.fake
            .sent()
            .into_iter()
            .filter(|s| {
                matches!(
                    s,
                    Sent::Keys { .. } | Sent::Text { .. } | Sent::Prompt { .. }
                )
            })
            .collect()
    }
}

async fn req(rig: &Rig, method: &str, path: &str, body: Option<&str>) -> (u16, Value) {
    let b = body.unwrap_or("");
    let text = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAuthorization: Bearer {SECRET}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
        rig.addr,
        b.len()
    );
    let mut s = TcpStream::connect(rig.addr).await.unwrap();
    s.write_all(text.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let raw = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (
        status,
        serde_json::from_str(body).unwrap_or_else(|e| panic!("{path}: {e}: {body}")),
    )
}

async fn attention(rig: &Rig) -> Vec<Value> {
    let (status, v) = req(rig, "GET", "/api/v1/attention", None).await;
    assert_eq!(status, 200, "{v}");
    v["items"].as_array().unwrap().clone()
}

async fn answer(rig: &Rig, id: &str, body: Value) -> (u16, Value) {
    req(
        rig,
        "POST",
        &format!("/api/v1/attention/{id}/prompt"),
        Some(&body.to_string()),
    )
    .await
}

fn refused(r: &(u16, Value), status: u16, code: &str) -> String {
    assert_eq!(r.0, status, "{}", r.1);
    assert_eq!(r.1["error"]["code"], code, "{}", r.1);
    r.1["error"]["message"].as_str().unwrap().to_string()
}

fn item<'a>(items: &'a [Value], id: &str) -> &'a Value {
    items
        .iter()
        .find(|i| i["id"] == id)
        .unwrap_or_else(|| panic!("no {id} in {items:?}"))
}

// AC2: a blocked worker of the run is an attention item carrying the prompt's text and
// choices, in the shape the Lead's has; once it is no longer blocked, the prompt is gone.
#[tokio::test]
async fn a_blocked_worker_is_an_attention_item_with_its_prompt() {
    let mut rig = rig().await;
    let id = rig.block("M1", "claude-1", PERMISSION);
    let items = attention(&rig).await;
    let it = item(&items, &id);
    assert_eq!(
        (it["kind"].as_str(), it["agent"].as_str()),
        (Some("blocked"), Some("claude-1"))
    );
    let p = &it["prompt"];
    assert_eq!(
        p["text"], "⏺ Bash(cargo test)\nDo you want to proceed?",
        "{p}"
    );
    let labels: Vec<&str> = p["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            assert!(o["id"].is_string(), "{o}");
            assert_eq!(o.as_object().unwrap().len(), 2, "only id and label: {o}");
            o["label"].as_str().unwrap()
        })
        .collect();
    assert_eq!(
        labels,
        [
            "Yes",
            "Yes, and don't ask again for cargo test commands",
            "No, and tell Claude what to do differently (esc)"
        ]
    );
    // reading types nothing
    assert!(rig.typed().is_empty(), "{:?}", rig.typed());

    rig.fake.set_status("claude-1", AgentStatus::Working);
    let items = attention(&rig).await;
    assert!(item(&items, &id)["prompt"].is_null());
}

// AC3: the chosen option's keys go to that worker's pane, and nowhere else.
#[tokio::test]
async fn an_answer_types_the_chosen_option_into_that_workers_pane_only() {
    let mut rig = rig().await;
    let id = rig.block("M1", "claude-1", PERMISSION);
    // another worker is blocked at the very same question
    let other = rig.block("M2", "codex-1", PERMISSION);
    let items = attention(&rig).await;
    let option = item(&items, &id)["prompt"]["options"][2]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, v) = answer(&rig, &id, json!({ "option": option })).await;
    assert_eq!(status, 200, "{v}");
    assert!(
        v["items"].is_array(),
        "the same shape as GET /attention: {v}"
    );
    assert_eq!(
        rig.typed(),
        vec![Sent::Keys {
            pane: "w1:p2".into(),
            keys: vec!["down".into(), "down".into(), "enter".into()]
        }],
        "only claude-1's pane"
    );
    // the other worker's item still waits for its own answer
    assert!(item(&v["items"].as_array().unwrap().clone(), &other)["prompt"].is_object());

    // a second click while the pane still shows the question types nothing more
    let r = answer(&rig, &id, json!({ "option": option })).await;
    refused(&r, 409, "no_prompt");
    assert_eq!(rig.typed().len(), 1);
}

// AC3: only an offered choice is accepted: no text, no keys, no id of another question, no
// other field; each refusal types nothing.
#[tokio::test]
async fn only_an_offered_choice_is_accepted() {
    let mut rig = rig().await;
    let id = rig.block("M1", "claude-1", PERMISSION);
    let items = attention(&rig).await;
    let offered = item(&items, &id)["prompt"]["options"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    for bad in ["1", "y", "enter", "", "answered_in_pane", "1-00000000"] {
        let m = refused(
            &answer(&rig, &id, json!({ "option": bad })).await,
            400,
            "bad_option",
        );
        assert!(
            m.contains("nothing was typed") && m.contains(&offered),
            "{m}"
        );
    }
    refused(
        &answer(&rig, &id, json!({ "option": offered, "keys": ["enter"] })).await,
        400,
        "bad_request",
    );
    refused(
        &answer(&rig, &id, json!({ "text": "yes" })).await,
        400,
        "bad_request",
    );

    // the question changed since it was shown: the old id is not this question's
    rig.fake.set_output(
        "claude-1",
        PERMISSION.replace("cargo test", "rm -rf target"),
    );
    refused(
        &answer(&rig, &id, json!({ "option": offered })).await,
        400,
        "bad_option",
    );
    assert!(rig.typed().is_empty(), "{:?}", rig.typed());
}

// AC3: nothing blocked, nothing to answer; and no route reaches an agent that is not a worker
// of this run, or a pane the run did not record for it.
#[tokio::test]
async fn answers_are_refused_when_nothing_is_blocked_or_the_agent_is_not_a_run_worker() {
    let mut rig = rig().await;
    let id = rig.block("M1", "claude-1", PERMISSION);
    let option = item(&attention(&rig).await, &id)["prompt"]["options"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let body = json!({ "option": option });

    // the worker is no longer blocked
    rig.fake.set_status("claude-1", AgentStatus::Working);
    let m = refused(&answer(&rig, &id, body.clone()).await, 409, "no_prompt");
    assert!(m.contains("not waiting for an answer"), "{m}");
    rig.fake.set_status("claude-1", AgentStatus::Blocked);

    // herdr's `claude-1` is not in the pane the run recorded: another project's agent
    rig.fake.remove_agent("claude-1");
    rig.fake.add_pane(PaneInfo::new("w9:p1", "w9", "w9:t1"));
    rig.fake.add_agent(AgentInfo::new(
        "claude-1",
        "claude",
        "w9:p1",
        AgentStatus::Blocked,
    ));
    rig.fake.set_output("claude-1", PERMISSION);
    refused(&answer(&rig, &id, body.clone()).await, 409, "no_prompt");
    assert!(item(&attention(&rig).await, &id)["prompt"].is_null());

    // an agent of no run, and the Lead, blocked at the same question
    let stranger = rig.block("M2", "stranger", PERMISSION);
    let lead = rig.block("M3", "lead", PERMISSION);
    for id in [&stranger, &lead] {
        let m = refused(&answer(&rig, id, body.clone()).await, 403, "forbidden");
        assert!(m.contains("not a worker of this run"), "{m}");
        assert!(item(&attention(&rig).await, id)["prompt"].is_null());
    }

    // an item that is not a blocked agent, and one that does not exist
    rig.store
        .transition(
            "M4",
            &TransitionRequest::auto(State::NeedsAnswer, Some("which port?")),
        )
        .unwrap();
    refused(
        &answer(&rig, "question:M4", body.clone()).await,
        409,
        "no_prompt",
    );
    refused(
        &answer(&rig, "blocked:999", body.clone()).await,
        404,
        "not_found",
    );
    assert!(rig.typed().is_empty(), "{:?}", rig.typed());
}

// AC4: a prompt the app cannot read into choices is its text with one "I answered it in the
// pane" option, which types nothing.
#[tokio::test]
async fn an_unreadable_prompt_offers_only_i_answered_it_in_the_pane() {
    let mut rig = rig().await;
    let id = rig.block("M1", "claude-1", "Overwrite src/main.rs? (y/n)");
    let p = item(&attention(&rig).await, &id)["prompt"].clone();
    assert_eq!(
        p,
        json!({
            "text": "Overwrite src/main.rs? (y/n)",
            "options": [{ "id": "answered_in_pane", "label": "I answered it in the pane" }]
        })
    );
    refused(
        &answer(&rig, &id, json!({ "option": "y" })).await,
        400,
        "bad_option",
    );
    let (status, v) = answer(&rig, &id, json!({ "option": "answered_in_pane" })).await;
    assert_eq!(status, 200, "{v}");
    assert!(
        rig.typed().is_empty(),
        "no key was guessed: {:?}",
        rig.typed()
    );
}

// AC2: the stream's `attention` event for a newly blocked worker carries the prompt too.
#[tokio::test]
async fn the_attention_event_carries_the_prompt() {
    let mut rig = rig().await;
    let mut s = TcpStream::connect(rig.addr).await.unwrap();
    s.write_all(
        format!(
            "GET /api/v1/stream?token={SECRET} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            rig.addr
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut buf = vec![0u8; 512];
    let n = s.read(&mut buf).await.unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 200"));

    let id = rig.block("M1", "claude-1", PERMISSION);
    let mut seen = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    let event = loop {
        let mut tmp = [0u8; 4096];
        if let Ok(Ok(n)) = tokio::time::timeout(Duration::from_millis(200), s.read(&mut tmp)).await
        {
            seen.push_str(&String::from_utf8_lossy(&tmp[..n]));
        }
        let found = seen
            .split("\n\n")
            .filter(|e| e.contains("event: attention"))
            .filter_map(|e| e.lines().find_map(|l| l.strip_prefix("data: ")))
            .map(|d| serde_json::from_str::<Value>(d).unwrap())
            .find(|e| e["op"] == "add" && e["item"]["id"] == id.as_str());
        if let Some(e) = found {
            break e;
        }
        assert!(Instant::now() < deadline, "no attention event: {seen}");
    };
    assert_eq!(
        event["item"]["prompt"]["text"],
        "⏺ Bash(cargo test)\nDo you want to proceed?"
    );
    assert_eq!(
        event["item"]["prompt"]["options"].as_array().unwrap().len(),
        3
    );
}

// AC5: the route is documented with its body and its refusals.
#[tokio::test]
async fn the_answer_route_is_documented() {
    let rig = rig().await;
    let (status, doc) = req(&rig, "GET", "/api/v1/openapi.json", None).await;
    assert_eq!(status, 200);
    let post = &doc["paths"]["/api/v1/attention/{id}/prompt"]["post"];
    assert_eq!(
        post["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/PromptAnswerBody"
    );
    assert_eq!(
        post["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/AttentionResponse"
    );
    for code in ["400", "403", "404", "409"] {
        assert!(post["responses"][code].is_object(), "{code}: {post}");
    }
    let item = &doc["components"]["schemas"]["AttentionItem"]["properties"]["prompt"];
    assert!(item.to_string().contains("PromptView"), "{item}");
}
