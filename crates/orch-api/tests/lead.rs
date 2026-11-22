//! The Lead endpoints over real sockets with the real router, against the in-memory fake
//! herdr: the state of the Lead with and without a run, starting exactly one Lead, refusing a
//! second one and kinds that cannot lead, Herdr being started when it is down, the first
//! prompt, the stream's `lead` events, and the started Lead taking chat messages once a run
//! exists.

use orch_api::lead::LeadOptions;
use orch_api::{Api, AppState, Token, build};
use orch_config::agents;
use orch_config::models;
use orch_core::chat::{ChatTick, chat_tick};
use orch_core::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::store::Store;
use orch_core::wake::{LeadPane, LeadRuntime, Notifier, SystemClock, WakeEnv, WakeTunables};
use orch_herdr::{AgentInfo, AgentStatus, CallKind, FakeHerdr, HerdrError, PaneInfo, RuntimeAdapter, Sent};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct Rig {
    _dir: tempfile::TempDir,
    root: PathBuf,
    fake: Arc<FakeHerdr>,
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

/// A registry where `claude` is installed (its binary is a script in the temp folder),
/// `ghost` can lead but is not installed, and `helper` is installed but cannot lead.
fn registry(dir: &Path) -> agents::Registry {
    use std::os::unix::fs::PermissionsExt;
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("claude");
    std::fs::write(&exe, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let user = dir.join("agents.toml");
    std::fs::write(
        &user,
        format!(
            "[claude]\nbinary = \"{exe}\"\n\n\
             [ghost]\nbinary = \"/nonexistent/ghost\"\nlaunch = \"ghost\"\nroles = [\"lead\"]\n\n\
             [helper]\nbinary = \"{exe}\"\nlaunch = \"helper\"\nroles = [\"coder\", \"reviewer\"]\n\n\
             [no_invoke]\nbinary = \"{exe}\"\nlaunch = \"no_invoke\"\nroles = [\"lead\"]\n",
            exe = exe.display()
        ),
    )
    .unwrap();
    agents::registry(agents::DEFAULT_TOML, &user).unwrap()
}

fn options(dir: &Path) -> LeadOptions {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    LeadOptions {
        registry: Some(registry(dir)),
        catalog: Some(
            models::catalog(models::DEFAULT_TOML, Path::new("/nonexistent/m.toml")).unwrap(),
        ),
        home: Some(home),
        server_wait_s: 1.0,
        agent_wait_s: 1.5,
        ready_wait_s: 1.5,
        watch_every: Duration::from_millis(100),
    }
}

async fn rig_with(tune: impl FnOnce(&mut LeadOptions)) -> Rig {
    rig_on(Arc::new(FakeHerdr::new()), "project", tune).await
}

/// A rig for the project folder `name`, over a herdr that other rigs may share.
async fn rig_on(fake: Arc<FakeHerdr>, name: &str, tune: impl FnOnce(&mut LeadOptions)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(name);
    std::fs::create_dir_all(&root).unwrap();
    // the GUI runs outside any herdr pane
    fake.set_in_pane(false);
    fake.set_detect_on_run(true);
    let mut opts = options(dir.path());
    tune(&mut opts);
    let state = AppState::with_herdr(root.clone(), Token::new(SECRET), fake.clone())
        .with_lead_options(opts);
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
        root,
        fake,
        addr,
        stop: Some(tx),
    }
}

async fn new_rig() -> Rig {
    rig_with(|_| {}).await
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

async fn get_lead(rig: &Rig) -> Value {
    let (status, v) = req(rig, "GET", "/api/v1/lead", None).await;
    assert_eq!(status, 200, "{v}");
    v
}

async fn start(rig: &Rig, body: Value) -> (u16, Value) {
    req(rig, "POST", "/api/v1/lead/start", Some(&body.to_string())).await
}

/// Ask until the Lead is no longer starting (bounded).
async fn settled(rig: &Rig) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let v = get_lead(rig).await;
        if v["state"] != "starting" {
            return v;
        }
        assert!(Instant::now() < deadline, "still starting: {v}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn refused(r: &(u16, Value), status: u16, code: &str) -> String {
    assert_eq!(r.0, status, "{}", r.1);
    assert_eq!(r.1["error"]["code"], code, "{}", r.1);
    r.1["error"]["message"].as_str().unwrap().to_string()
}

fn prompts(fake: &FakeHerdr) -> Vec<(String, String)> {
    fake.sent()
        .into_iter()
        .filter_map(|s| match s {
            Sent::Prompt { agent, text } => Some((agent, text)),
            _ => None,
        })
        .collect()
}

// AC1: a project with no run has no Lead yet, and that is a normal answer.
#[tokio::test]
async fn without_a_run_the_lead_is_none() {
    let rig = new_rig().await;
    let v = get_lead(&rig).await;
    assert_eq!(v["state"], "none", "{v}");
    assert!(
        v["agent"].is_null() && v["pane"].is_null() && v["status"].is_null(),
        "{v}"
    );
    assert!(v["note"].as_str().unwrap().contains("first message"), "{v}");
}

// AC2 + AC5 + AC1: one start opens one pane in the project folder, names it `lead`, and the
// Lead's first prompt is the playbook followed by the developer's message as written.
#[tokio::test]
async fn a_start_opens_one_named_pane_in_the_project_and_hands_over_the_playbook() {
    let rig = new_rig().await;
    let casual = "hey, what's in this repo? \"quotes\" and\nnewlines stay";
    let r = start(&rig, json!({ "agent": "claude", "message": casual })).await;
    assert_eq!(r.0, 202, "{}", r.1);
    assert_eq!(r.1["state"], "starting", "{}", r.1);
    assert_eq!(r.1["agent"], "claude");

    let v = settled(&rig).await;
    assert_eq!(v["state"], "running", "{v}");
    assert_eq!(v["agent"], "claude");
    let pane = v["pane"].as_str().unwrap().to_string();
    // working: the fake starts a turn on the prompt it was given
    assert_eq!(v["status"], "working", "{v}");
    assert!(v["note"].is_null(), "{v}");

    let fake = &rig.fake;
    let p = fake.pane(&pane).unwrap();
    assert_eq!(
        p.cwd.as_deref(),
        Some(rig.root.to_str().unwrap()),
        "runs in the project folder"
    );
    assert_eq!(p.label.as_deref(), Some("lead"), "the pane is named lead");
    assert_eq!(
        fake.agent("lead").unwrap().pane_id,
        pane,
        "the agent is named lead"
    );
    assert_eq!(fake.panes(None).unwrap().len(), 1, "exactly one pane");

    let sent = fake.sent();
    assert_eq!(
        sent[0],
        Sent::Run {
            pane: pane.clone(),
            command: "claude --permission-mode acceptEdits".into()
        }
    );
    let ps = prompts(fake);
    assert_eq!(ps.len(), 1, "one first prompt: {ps:?}");
    let (to, text) = &ps[0];
    assert_eq!(to, "lead");
    assert!(
        !text.contains(orch_config::playbooks::PLAN),
        "the planning playbook is not typed into the pane"
    );
    assert!(
        text.starts_with(&format!("/orch-plan {casual}")),
        "starts with the kind's skill command plus the message as written"
    );
    assert!(text.contains(&format!("Herdr pane {pane}")));
    assert!(text.contains("`Lead: lead (claude)`"));
}

// AC5: the message is optional.
#[tokio::test]
async fn a_start_without_a_message_tells_the_lead_to_wait() {
    let rig = new_rig().await;
    let r = start(&rig, json!({ "agent": "claude" })).await;
    assert_eq!(r.0, 202, "{}", r.1);
    assert_eq!(settled(&rig).await["state"], "running");
    let ps = prompts(&rig.fake);
    assert_eq!(ps.len(), 1);
    assert!(
        !ps[0].1.contains(orch_config::playbooks::PLAN),
        "the planning playbook is not typed into the pane"
    );
    assert!(ps[0].1.ends_with(
        "The developer has not written anything yet: say that you are ready, then wait for them.\n"
    ));

    // a blank message is "no message" too
    let rig = new_rig().await;
    assert_eq!(
        start(&rig, json!({ "agent": "claude", "message": "  " }))
            .await
            .0,
        202
    );
    settled(&rig).await;
    assert!(
        prompts(&rig.fake)[0]
            .1
            .contains("has not written anything yet")
    );
    assert!(!prompts(&rig.fake)[0].1.contains(orch_config::playbooks::PLAN));
}

// AC3: a second start while one runs, or while one is starting, starts nothing.
#[tokio::test]
async fn a_second_start_is_refused_and_opens_no_pane() {
    let rig = new_rig().await;
    // herdr does not see the agent: the start stays `starting` for the bounded wait
    rig.fake.set_detect_on_run(false);
    assert_eq!(start(&rig, json!({ "agent": "claude" })).await.0, 202);
    assert_eq!(get_lead(&rig).await["state"], "starting");
    // the start opens its pane on its own thread: wait (bounded) until it has one
    let deadline = Instant::now() + Duration::from_secs(5);
    while rig.fake.panes(None).unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the first start never opened its pane");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let m = refused(
        &start(&rig, json!({ "agent": "claude" })).await,
        409,
        "lead_running",
    );
    assert!(m.contains("already starting"), "{m}");
    // the first start opens its pane on its own thread after answering 202: wait for it, so a
    // slow machine is not mistaken for "no pane" (a second pane would still show as 2)
    let deadline = Instant::now() + Duration::from_secs(10);
    while rig.fake.panes(None).unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the first start never opened its pane");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        rig.fake.panes(None).unwrap().len(),
        1,
        "no second pane while starting"
    );

    // the agent never showed up: gone, with what to do
    let v = settled(&rig).await;
    assert_eq!(v["state"], "gone", "{v}");
    assert_eq!(
        rig.fake.panes(None).unwrap().len(),
        1,
        "the refused start never opened a pane"
    );
    assert!(
        v["note"].as_str().unwrap().contains("first-run question"),
        "{v}"
    );

    // a fresh project: the Lead runs, and a second start is refused
    let rig = new_rig().await;
    assert_eq!(start(&rig, json!({ "agent": "claude" })).await.0, 202);
    assert_eq!(settled(&rig).await["state"], "running");
    let before = (rig.fake.panes(None).unwrap().len(), rig.fake.sent().len());
    let m = refused(
        &start(&rig, json!({ "agent": "claude", "message": "again" })).await,
        409,
        "lead_running",
    );
    assert!(m.contains("already running") && m.contains("`lead`"), "{m}");
    assert_eq!(
        (rig.fake.panes(None).unwrap().len(), rig.fake.sent().len()),
        before,
        "nothing started, nothing sent"
    );
}

// A start whose agent Herdr never saw (a first-run question) is picked up by the next start:
// it waits for that agent instead of opening a second one.
#[tokio::test]
async fn starting_again_picks_up_the_pane_whose_agent_was_not_seen() {
    let rig = new_rig().await;
    rig.fake.set_detect_on_run(false);
    assert_eq!(
        start(&rig, json!({ "agent": "claude", "message": "hello" }))
            .await
            .0,
        202
    );
    let v = settled(&rig).await;
    assert_eq!(v["state"], "gone", "{v}");
    let pane = v["pane"].as_str().unwrap().to_string();
    assert!(
        v["note"].as_str().unwrap().contains("picks that pane up"),
        "{v}"
    );

    // the developer answered the question: herdr now sees the agent in that pane
    rig.fake.add_agent(orch_herdr::AgentInfo::new(
        &pane,
        "claude",
        &pane,
        AgentStatus::Idle,
    ));
    let r = start(&rig, json!({ "agent": "claude", "message": "hello again" })).await;
    assert_eq!(r.0, 202, "{}", r.1);
    assert_eq!(r.1["pane"], pane.as_str(), "the same pane from the start");
    let v = settled(&rig).await;
    assert_eq!(
        (v["state"].as_str(), v["pane"].as_str()),
        (Some("running"), Some(pane.as_str())),
        "{v}"
    );
    assert_eq!(rig.fake.panes(None).unwrap().len(), 1, "no second pane");
    let runs = rig
        .fake
        .sent()
        .iter()
        .filter(|s| matches!(s, Sent::Run { .. }))
        .count();
    assert_eq!(runs, 1, "nothing typed into the waiting agent's pane");
    let ps = prompts(&rig.fake);
    assert_eq!(ps.len(), 1);
    assert!(ps[0].1.starts_with("/orch-plan hello again"));
}

// AC3: two starts at the same moment start exactly one Lead.
#[tokio::test]
async fn simultaneous_starts_start_exactly_one_lead() {
    let rig = new_rig().await;
    let body = json!({ "agent": "claude" });
    let (a, b) = tokio::join!(start(&rig, body.clone()), start(&rig, body.clone()));
    let mut codes = [a.0, b.0];
    codes.sort();
    assert_eq!(codes, [202, 409], "{a:?} {b:?}");
    assert_eq!(settled(&rig).await["state"], "running");
    assert_eq!(rig.fake.panes(None).unwrap().len(), 1);
    assert_eq!(prompts(&rig.fake).len(), 1);
}

// AC4: kinds that are unknown, not installed, or cannot lead are refused by name.
#[tokio::test]
async fn kinds_that_cannot_lead_here_are_refused_by_name() {
    let rig = new_rig().await;
    let m = refused(
        &start(&rig, json!({ "agent": "ghost" })).await,
        400,
        "agent_not_installed",
    );
    assert!(m.contains("`ghost`") && m.contains("not installed"), "{m}");
    let m = refused(
        &start(&rig, json!({ "agent": "mystery" })).await,
        400,
        "agent_not_installed",
    );
    assert!(m.contains("`mystery`") && m.contains("not known"), "{m}");
    let m = refused(
        &start(&rig, json!({ "agent": "helper" })).await,
        400,
        "bad_request",
    );
    assert!(
        m.contains("`helper`") && m.contains("cannot be the Lead") && m.contains("coder"),
        "{m}"
    );
    let m = refused(
        &start(&rig, json!({ "agent": "no_invoke" })).await,
        400,
        "bad_request",
    );
    assert!(
        m.contains("`no_invoke`") && m.contains("no `invoke` command"),
        "{m}"
    );
    let m = refused(&start(&rig, json!({})).await, 400, "bad_request");
    assert!(m.contains("`agent` is required"), "{m}");
    refused(
        &req(&rig, "POST", "/api/v1/lead/start", Some("{nope")).await,
        400,
        "bad_request",
    );
    assert!(rig.fake.panes(None).unwrap().is_empty(), "nothing opened");
    assert!(rig.fake.sent().is_empty(), "nothing run");
    assert_eq!(rig.fake.server_starts(), 0);
    assert_eq!(
        get_lead(&rig).await["state"],
        "none",
        "a refused start leaves no trace"
    );
}

// AC6: Herdr down is started headless and the start carries on.
#[tokio::test]
async fn a_herdr_that_is_down_is_started_and_the_lead_comes_up() {
    let rig = new_rig().await;
    rig.fake.set_reachable(false);
    assert_eq!(
        get_lead(&rig).await["state"],
        "none",
        "GET never starts Herdr"
    );
    assert_eq!(rig.fake.server_starts(), 0);
    let r = start(&rig, json!({ "agent": "claude", "message": "hi" })).await;
    assert_eq!(r.0, 202, "{}", r.1);
    assert_eq!(rig.fake.server_starts(), 1);
    assert_eq!(settled(&rig).await["state"], "running");
}

// AC6: when Herdr cannot be started, the error says what happened and what to do, within the
// bound, and the Lead slot is free again.
#[tokio::test]
async fn a_herdr_that_cannot_be_started_is_a_clear_bounded_error() {
    let rig = new_rig().await;
    rig.fake.set_reachable(false);
    rig.fake.set_server_comes_up(false);
    let t = Instant::now();
    let m = refused(
        &start(&rig, json!({ "agent": "claude" })).await,
        503,
        "herdr_unavailable",
    );
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "bounded: {:?}",
        t.elapsed()
    );
    assert!(
        m.contains("did not come up within 1s") && m.contains("start Herdr yourself"),
        "{m}"
    );
    assert_eq!(get_lead(&rig).await["state"], "none");

    // not installed at all
    rig.fake.fail_next(
        CallKind::ServerStart,
        1,
        HerdrError::Unavailable {
            message: "cannot run \"herdr\": No such file or directory".into(),
        },
    );
    let m = refused(
        &start(&rig, json!({ "agent": "claude" })).await,
        503,
        "herdr_unavailable",
    );
    assert!(
        m.contains("not installed") && m.contains("https://herdr.dev"),
        "{m}"
    );

    // once Herdr comes up the start works: nothing stayed claimed
    rig.fake.set_server_comes_up(true);
    assert_eq!(start(&rig, json!({ "agent": "claude" })).await.0, 202);
    assert_eq!(settled(&rig).await["state"], "running");
}

// AC6: a pane that never opens, or a Lead that never becomes ready, ends in a clear note.
#[tokio::test]
async fn a_failed_or_unready_start_ends_with_what_to_do() {
    let rig = new_rig().await;
    rig.fake
        .fail_next(CallKind::WorkspaceCreate, 1, HerdrError::other("no room"));
    assert_eq!(start(&rig, json!({ "agent": "claude" })).await.0, 202);
    let v = settled(&rig).await;
    assert_eq!(v["state"], "none", "{v}");
    assert!(
        v["note"]
            .as_str()
            .unwrap()
            .contains("could not open a pane"),
        "{v}"
    );

    // the agent comes up blocked at a first-run question and stays there
    let rig = new_rig().await;
    rig.fake.fail_next(
        CallKind::AgentPrompt,
        100,
        HerdrError::AgentBlocked {
            agent: "lead".into(),
            message: "trust?".into(),
        },
    );
    let t = Instant::now();
    assert_eq!(
        start(&rig, json!({ "agent": "claude", "message": "hi" }))
            .await
            .0,
        202
    );
    let v = settled(&rig).await;
    assert!(
        t.elapsed() < Duration::from_secs(8),
        "bounded: {:?}",
        t.elapsed()
    );
    assert_eq!(v["state"], "running", "{v}");
    let note = v["note"].as_str().unwrap();
    assert!(
        note.contains("did not become ready") && note.contains("send your message again"),
        "{v}"
    );
}

// AC1: a Lead that disappears is `gone`; a run that names one says `gone` too.
#[tokio::test]
async fn a_lead_that_disappears_is_gone() {
    let rig = new_rig().await;
    assert_eq!(start(&rig, json!({ "agent": "claude" })).await.0, 202);
    let v = settled(&rig).await;
    rig.fake.set_status("lead", AgentStatus::Blocked);
    assert_eq!(get_lead(&rig).await["status"], "blocked");
    rig.fake.set_status("lead", AgentStatus::Idle);
    assert_eq!(get_lead(&rig).await["status"], "idle");
    rig.fake.remove_agent("lead");
    let g = get_lead(&rig).await;
    assert_eq!(g["state"], "gone", "{g}");
    assert_eq!(g["agent"], "claude");
    assert_eq!(g["pane"], v["pane"]);

    // a run whose plan names a lead herdr does not have
    let rig = new_rig().await;
    run(&rig.root);
    let g = get_lead(&rig).await;
    assert_eq!(g["state"], "gone", "{g}");
    assert!(
        g["note"]
            .as_str()
            .unwrap()
            .contains("no agent named `lead`"),
        "{g}"
    );
}

// The live stream carries the Lead's state changes as `lead` events, so the page never polls.
#[tokio::test]
async fn lead_state_changes_arrive_on_the_stream() {
    let rig = new_rig().await;
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
    // let the subscription register before anything happens
    let mut buf = vec![0u8; 512];
    let n = s.read(&mut buf).await.unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 200"));

    assert_eq!(start(&rig, json!({ "agent": "claude" })).await.0, 202);
    let mut seen = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !seen.contains("\"state\":\"running\"") && Instant::now() < deadline {
        let mut tmp = [0u8; 4096];
        if let Ok(Ok(n)) = tokio::time::timeout(Duration::from_millis(200), s.read(&mut tmp)).await
        {
            seen.push_str(&String::from_utf8_lossy(&tmp[..n]));
        }
    }
    let events: Vec<Value> = seen
        .split("\n\n")
        .filter(|e| e.contains("event: lead"))
        .filter_map(|e| e.lines().find_map(|l| l.strip_prefix("data: ")))
        .map(|d| serde_json::from_str(d).unwrap())
        .collect();
    assert!(events.iter().any(|e| e["state"] == "starting"), "{seen}");
    let last = events.last().unwrap();
    assert_eq!(last["state"], "running", "{seen}");
    assert_eq!(last["agent"], "claude");
    assert!(last["pane"].is_string() && last.get("status").is_some() && last.get("note").is_some());
}

// ---------------------------------------------------------------- AC7: chat delivery

/// The event manager's view of the lead's pane, over the same fake herdr.
struct HerdrLead<'a>(&'a FakeHerdr);
impl LeadRuntime for HerdrLead<'_> {
    fn pane(&self, lead: &str) -> LeadPane {
        match self.0.agent(lead) {
            Ok(a) if a.status == AgentStatus::Working || a.status == AgentStatus::Blocked => {
                LeadPane::Working
            }
            Ok(a) if a.focused => LeadPane::FocusedInput,
            Ok(_) => LeadPane::Idle,
            Err(_) => LeadPane::Gone,
        }
    }
    fn send(&self, lead: &str, text: &str) -> Result<(), String> {
        self.0.send_prompt(lead, text).map_err(|e| e.to_string())
    }
}
struct Quiet;
impl Notifier for Quiet {
    fn notify(&self, _: &str) {}
}

/// A run whose plan says `Lead: lead (claude)`, as the Lead writes it.
fn run(root: &Path) -> Store {
    let dir = root.join(".orchestrate");
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Store::open_in(&dir).unwrap();
    let agent = |role, name: &str| RunAgent {
        role,
        name: name.into(),
        kind: "claude".into(),
        model: None,
        tier: None,
        detect: true,
        pane: None,
    };
    s.apply_sync(&SyncInput {
        settings: RunSettings {
            branch: "feature/demo".into(),
            base: Some("main".into()),
            title: "Demo".into(),
            commits: "squash".into(),
            isolation: "worktree".into(),
            max_parallel: 1,
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
            agent(AgentRole::Worker, "claude-1"),
            agent(AgentRole::Lead, "lead"),
        ],
        tasks: vec![PlanTask {
            id: "M1".into(),
            title: "One".into(),
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
        }],
    })
    .unwrap();
    s
}

#[tokio::test]
async fn the_started_lead_takes_chat_messages_once_a_run_exists() {
    let rig = new_rig().await;
    assert_eq!(
        start(
            &rig,
            json!({ "agent": "claude", "message": "plan a login page" })
        )
        .await
        .0,
        202
    );
    let v = settled(&rig).await;
    assert_eq!(v["state"], "running");

    // the Lead writes its plan (`Lead: lead (claude)`) and finishes its turn
    let mut store = run(&rig.root);
    rig.fake.set_status("lead", AgentStatus::Idle);
    let after_plan = get_lead(&rig).await;
    assert_eq!(
        (after_plan["state"].as_str(), after_plan["pane"].as_str()),
        (Some("running"), v["pane"].as_str())
    );

    let (status, m) = req(
        &rig,
        "POST",
        "/api/v1/chat",
        Some(&json!({ "kind": "message", "text": "add a logout button" }).to_string()),
    )
    .await;
    assert_eq!(status, 201, "the stored message is deliverable: {m}");

    let lead = HerdrLead(&rig.fake);
    let env = WakeEnv {
        runtime: &lead,
        notifier: &Quiet,
        clock: &SystemClock,
        tun: WakeTunables::default(),
    };
    let id = m["id"].as_i64().unwrap();
    assert_eq!(
        chat_tick(&mut store, &env, &mut |_| {}).unwrap(),
        ChatTick::Sent(id)
    );
    let ps = prompts(&rig.fake);
    assert_eq!(ps.len(), 2, "{ps:?}");
    assert_eq!(ps[1].0, "lead");
    assert!(
        ps[1].1.contains(&format!("orch chat #{id}")) && ps[1].1.contains("add a logout button"),
        "{}",
        ps[1].1
    );
}

// AC8: both routes are in the OpenAPI document with their answers.
#[tokio::test]
async fn the_routes_are_documented() {
    let rig = new_rig().await;
    let (status, doc) = req(&rig, "GET", "/api/v1/openapi.json", None).await;
    assert_eq!(status, 200);
    let get = &doc["paths"]["/api/v1/lead"]["get"];
    assert_eq!(
        get["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/LeadView"
    );
    let post = &doc["paths"]["/api/v1/lead/start"]["post"];
    assert_eq!(
        post["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/LeadStartBody"
    );
    for code in ["202", "400", "409", "503"] {
        assert!(post["responses"][code].is_object(), "{code}: {post}");
    }
    let view = &doc["components"]["schemas"]["LeadView"];
    for field in ["state", "agent", "pane", "status", "note"] {
        assert!(view["properties"][field].is_object(), "{field}: {view}");
    }
}

// Herdr agent names are global and chat reaches the Lead by its bare name: a second project's
// Lead must never share the first one's name, and each project sees only its own.
#[tokio::test]
async fn two_projects_never_share_a_lead_name() {
    let fake = Arc::new(FakeHerdr::new());
    let a = rig_on(fake.clone(), "alpha", |_| {}).await;
    let b = rig_on(fake.clone(), "beta", |_| {}).await;
    assert_eq!(start(&a, json!({ "agent": "claude" })).await.0, 202);
    assert_eq!(settled(&a).await["state"], "running");
    assert_eq!(get_lead(&b).await["state"], "none", "A's Lead is not B's");

    assert_eq!(start(&b, json!({ "agent": "claude" })).await.0, 202);
    assert_eq!(settled(&b).await["state"], "running");
    let leads: Vec<_> = fake.agents().unwrap();
    assert_eq!(leads.len(), 2, "one Lead per project: {leads:?}");
    let names: std::collections::HashSet<_> = leads.iter().map(|x| x.name.clone()).collect();
    assert_eq!(names.len(), 2, "two Leads, two names: {names:?}");
    for l in &leads {
        let own = if l.cwd.as_deref() == Some(a.root.to_str().unwrap()) {
            &a
        } else {
            &b
        };
        assert_eq!(l.cwd.as_deref(), Some(own.root.to_str().unwrap()));
        let v = get_lead(own).await;
        assert_eq!(v["state"], "running", "{v}");
        assert_eq!(v["pane"], l.pane_id.as_str());
    }
    // each project's playbook names its own Lead in the plan header
    for (rig, l) in [&a, &b].iter().zip(["alpha", "beta"]) {
        let sent = prompts(&rig.fake);
        assert!(
            sent.iter().any(
                |(who, text)| text.contains(&format!("Lead: {who} (claude)"))
                    && leads
                        .iter()
                        .any(|x| &x.name == who
                            && x.cwd.as_deref() == Some(rig.root.to_str().unwrap()))
            ),
            "{l}: {sent:?}"
        );
    }
    // a second start in either project is still refused
    refused(
        &start(&b, json!({ "agent": "claude" })).await,
        409,
        "lead_running",
    );
    assert_eq!(fake.panes(None).unwrap().len(), 2);
}

// AC3: When the kind's orch skills are already installed nothing extra happens; when they
// are not, the app installs them itself through orch's own skill installer before the Lead
// starts, and a test with a fake home folder proves both cases.
#[tokio::test]
async fn skills_installed_when_missing_and_left_alone_when_present() {
    let home = tempfile::tempdir().unwrap();
    let home_path = home.path().to_path_buf();
    let rig = rig_with(|opts| {
        opts.home = Some(home_path.clone());
    })
    .await;

    // Case 1: Skills are not installed yet in fake home
    let skill_file = home_path.join(".claude/skills/orch-plan/SKILL.md");
    let cmd_file = home_path.join(".claude/commands/orch-plan.md");
    let manifest = home_path.join(".local/share/herdr-orchestra/manifest.txt");
    assert!(!skill_file.exists());
    assert!(!cmd_file.exists());
    assert!(!manifest.exists());

    // Starting installs the skills before the Lead starts
    assert_eq!(
        start(&rig, json!({ "agent": "claude", "message": "plan a feature" })).await.0,
        202
    );
    assert_eq!(settled(&rig).await["state"], "running");
    assert!(skill_file.exists(), "skill file was installed");
    assert!(cmd_file.exists(), "command file was installed");
    assert!(manifest.exists(), "manifest was written");

    // Case 2: Skills already installed — nothing extra happens (file is not overwritten)
    std::fs::write(&skill_file, "custom-existing-skill").unwrap();
    let rig2 = rig_on(Arc::new(FakeHerdr::new()), "project2", |opts| {
        opts.home = Some(home_path.clone());
    })
    .await;

    assert_eq!(
        start(&rig2, json!({ "agent": "claude", "message": "second feature" })).await.0,
        202
    );
    assert_eq!(settled(&rig2).await["state"], "running");
    assert_eq!(
        std::fs::read_to_string(&skill_file).unwrap(),
        "custom-existing-skill",
        "existing installed skill was preserved without extra install"
    );
}

// AC4: When installing the skills fails, the start is refused before any pane opens, with a
// message naming the kind and what happened; nothing is typed into any pane.
#[tokio::test]
async fn failed_skill_installation_refuses_start_before_any_pane_opens() {
    let home = tempfile::tempdir().unwrap();
    // Block installation by creating a file where .claude folder needs to be created
    std::fs::write(home.path().join(".claude"), "not a directory").unwrap();

    let rig = rig_with(|opts| {
        opts.home = Some(home.path().to_path_buf());
    })
    .await;

    let res = start(&rig, json!({ "agent": "claude", "message": "hi" })).await;
    let msg = refused(&res, 400, "bad_request");
    assert!(
        msg.contains("`claude`") && (msg.contains("cannot write") || msg.contains("cannot install")),
        "message names kind and what happened: {msg}"
    );
    assert!(
        rig.fake.panes(None).unwrap().is_empty(),
        "no pane was opened"
    );
    assert!(
        rig.fake.sent().is_empty(),
        "nothing was typed into any pane"
    );
    assert_eq!(get_lead(&rig).await["state"], "none");
}

// ---------------------------------------------------------------- the Lead's open prompt

const TRUST: &str = "\
╭──────────────────────────────────────────╮
│ Do you trust the files in this folder?   │
│                                          │
│ ❯ 1. Yes, proceed                        │
│   2. No, exit                            │
╰──────────────────────────────────────────╯
   Enter to confirm · Esc to exit
";

/// Keys typed into any pane, and text or prompts sent to any agent, after `from` sends.
fn typed_since(fake: &FakeHerdr, from: usize) -> Vec<Sent> {
    fake.sent()
        .into_iter()
        .skip(from)
        .filter(|s| matches!(s, Sent::Keys { .. } | Sent::Text { .. } | Sent::Prompt { .. }))
        .collect()
}

async fn answer_lead(rig: &Rig, body: Value) -> (u16, Value) {
    req(rig, "POST", "/api/v1/lead/prompt", Some(&body.to_string())).await
}

/// A started Lead, settled and running.
async fn running_lead() -> (Rig, String) {
    let rig = new_rig().await;
    assert_eq!(start(&rig, json!({ "agent": "claude" })).await.0, 202);
    let v = settled(&rig).await;
    assert_eq!(v["state"], "running", "{v}");
    let pane = v["pane"].as_str().unwrap().to_string();
    (rig, pane)
}

// AC1: while the Lead is blocked, `GET /lead` carries the question and its choices; otherwise
// `prompt` is null.
#[tokio::test]
async fn a_blocked_lead_shows_its_prompt_and_none_otherwise() {
    let rig = new_rig().await;
    assert!(get_lead(&rig).await["prompt"].is_null(), "no Lead, no prompt");
    assert_eq!(start(&rig, json!({ "agent": "claude" })).await.0, 202);
    let v = settled(&rig).await;
    assert!(v["prompt"].is_null(), "{v}");

    rig.fake.set_output("lead", TRUST);
    rig.fake.set_status("lead", AgentStatus::Blocked);
    let v = get_lead(&rig).await;
    assert_eq!(v["status"], "blocked");
    let p = &v["prompt"];
    assert_eq!(p["text"], "Do you trust the files in this folder?", "{p}");
    let labels: Vec<&str> = p["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["Yes, proceed", "No, exit"]);

    rig.fake.set_status("lead", AgentStatus::Working);
    assert!(get_lead(&rig).await["prompt"].is_null());
}

// AC1: the stream's `lead` events carry the prompt while the Lead is blocked, and drop it
// when it is not.
#[tokio::test]
async fn the_lead_event_carries_the_prompt_while_blocked() {
    let (rig, _) = running_lead().await;
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
    get_lead(&rig).await; // the page asks about the Lead: the watch runs

    let mut seen = String::new();
    let mut next = async |want: &dyn Fn(&Value) -> bool| -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let mut tmp = [0u8; 4096];
            if let Ok(Ok(n)) =
                tokio::time::timeout(Duration::from_millis(200), s.read(&mut tmp)).await
            {
                seen.push_str(&String::from_utf8_lossy(&tmp[..n]));
            }
            let found = seen
                .split("\n\n")
                .filter(|e| e.contains("event: lead\n"))
                .filter_map(|e| e.lines().find_map(|l| l.strip_prefix("data: ")))
                .map(|d| serde_json::from_str::<Value>(d).unwrap())
                .find(|e| want(e));
            if let Some(e) = found {
                seen.clear();
                return e;
            }
            assert!(Instant::now() < deadline, "no such lead event: {seen}");
        }
    };
    rig.fake.set_output("lead", TRUST);
    rig.fake.set_status("lead", AgentStatus::Blocked);
    let e = next(&|e| e["prompt"].is_object()).await;
    assert_eq!(e["status"], "blocked");
    assert_eq!(e["prompt"]["options"][0]["label"], "Yes, proceed");
    rig.fake.set_status("lead", AgentStatus::Idle);
    let e = next(&|e| e["status"] == "idle").await;
    assert!(e["prompt"].is_null(), "{e}");
}

// AC3: an offered option is typed into the Lead's pane, and into no other pane, even when
// another agent waits at the same question.
#[tokio::test]
async fn answering_the_lead_types_the_choice_into_its_pane_only() {
    let (rig, pane) = running_lead().await;
    rig.fake.add_pane(PaneInfo::new("w7:p1", "w7", "w7:t1"));
    rig.fake
        .add_agent(AgentInfo::new("other", "claude", "w7:p1", AgentStatus::Blocked));
    rig.fake.set_output("other", TRUST);
    rig.fake.set_output("lead", TRUST);
    rig.fake.set_status("lead", AgentStatus::Blocked);
    let before = rig.fake.sent().len();
    let no = get_lead(&rig).await["prompt"]["options"][1]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, v) = answer_lead(&rig, json!({ "option": no })).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["state"], "running", "the same shape as GET /lead: {v}");
    assert_eq!(
        typed_since(&rig.fake, before),
        vec![Sent::Keys {
            pane,
            keys: vec!["down".into(), "enter".into()]
        }]
    );
}

// AC3: refused with a plain message, and nothing typed, when nothing is blocked, for an
// option the prompt does not offer, and for anything but an option.
#[tokio::test]
async fn lead_answers_are_refused_with_nothing_typed() {
    // no Lead at all: a normal state, but nothing to answer
    let rig = new_rig().await;
    let m = refused(&answer_lead(&rig, json!({ "option": "1" })).await, 409, "no_prompt");
    assert!(m.contains("not waiting for an answer"), "{m}");

    let (rig, _) = running_lead().await;
    rig.fake.set_output("lead", TRUST);
    let before = rig.fake.sent().len();
    // running but not blocked
    refused(&answer_lead(&rig, json!({ "option": "1" })).await, 409, "no_prompt");

    rig.fake.set_status("lead", AgentStatus::Blocked);
    let yes = get_lead(&rig).await["prompt"]["options"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    for bad in ["1", "y", "enter", "answered_in_pane", ""] {
        let m = refused(&answer_lead(&rig, json!({ "option": bad })).await, 400, "bad_option");
        assert!(m.contains("nothing was typed") && m.contains("Yes, proceed"), "{m}");
    }
    refused(
        &answer_lead(&rig, json!({ "option": yes, "text": "yes" })).await,
        400,
        "bad_request",
    );
    assert!(typed_since(&rig.fake, before).is_empty());

    // the Lead of another project, blocked at the same question, is not this project's
    let fake = Arc::new(FakeHerdr::new());
    let other = rig_on(fake.clone(), "other", |_| {}).await;
    let mut pane = PaneInfo::new("w8:p1", "w8", "w8:t1");
    pane.cwd = Some(other.root.display().to_string());
    fake.add_pane(pane);
    fake.add_agent(AgentInfo::new("lead", "claude", "w8:p1", AgentStatus::Blocked));
    fake.set_output("lead", TRUST);
    let mine = rig_on(fake.clone(), "mine", |_| {}).await;
    assert!(get_lead(&mine).await["prompt"].is_null());
    refused(&answer_lead(&mine, json!({ "option": yes })).await, 409, "no_prompt");
    assert!(typed_since(&fake, 0).is_empty());
}

// AC4: a question the app cannot read into choices is shown as text with the single
// "I answered it in the pane" action, which types nothing.
#[tokio::test]
async fn an_unreadable_lead_prompt_offers_only_i_answered_it_in_the_pane() {
    let (rig, _) = running_lead().await;
    rig.fake.set_output("lead", "Sign in to continue? (y/n)");
    rig.fake.set_status("lead", AgentStatus::Blocked);
    let before = rig.fake.sent().len();
    assert_eq!(
        get_lead(&rig).await["prompt"],
        json!({
            "text": "Sign in to continue? (y/n)",
            "options": [{ "id": "answered_in_pane", "label": "I answered it in the pane" }]
        })
    );
    let (status, v) = answer_lead(&rig, json!({ "option": "answered_in_pane" })).await;
    assert_eq!(status, 200, "{v}");
    assert!(typed_since(&rig.fake, before).is_empty());
}

// AC5: the answer route is in the OpenAPI document, with its body, answer and refusals.
#[tokio::test]
async fn the_lead_prompt_route_is_documented() {
    let rig = new_rig().await;
    let (status, doc) = req(&rig, "GET", "/api/v1/openapi.json", None).await;
    assert_eq!(status, 200);
    let post = &doc["paths"]["/api/v1/lead/prompt"]["post"];
    assert_eq!(
        post["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/PromptAnswerBody"
    );
    assert_eq!(
        post["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/LeadView"
    );
    for code in ["400", "409", "503"] {
        assert!(post["responses"][code].is_object(), "{code}: {post}");
    }
    let view = &doc["components"]["schemas"]["LeadView"]["properties"]["prompt"];
    assert!(view.to_string().contains("PromptView"), "{view}");
}
