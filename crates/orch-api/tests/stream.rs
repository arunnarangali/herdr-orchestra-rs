//! Tests for the live event stream: SSE delivery, Last-Event-ID/since reconnect,
//! buffer eviction and re-fetch status, idle keep-alive comments, concurrency bound and refusal,
//! auth modes, real run database polling, and OpenAPI documentation.

use orch_api::routes::stream::feed::StreamHub;
use orch_api::{Api, AppState, Token, build_with};
use orch_core::actions::{RunControl, Source};
use orch_core::lead_queue::LeadKind;
use orch_core::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::store::{Store, TransitionRequest};
use orch_core::task::State;
use serde_json::Value;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, watch};

const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
static TEST_LOCK: Mutex<()> = Mutex::const_new(());

struct SseEvent {
    id: Option<String>,
    event: Option<String>,
    data: Option<String>,
    comment: Option<String>,
}

struct SseClient {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl SseClient {
    async fn connect(addr: SocketAddr, path: &str, headers: &[(&str, &str)]) -> (u16, Self) {
        let mut req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str("\r\n");

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(req.as_bytes()).await.unwrap();

        let mut client = SseClient {
            stream,
            buf: Vec::new(),
        };

        // Read headers to determine status code
        let mut header_buf = Vec::new();
        let mut temp = [0u8; 512];
        let status;
        loop {
            let n = client.stream.read(&mut temp).await.unwrap();
            assert!(n > 0, "unexpected EOF reading headers");
            header_buf.extend_from_slice(&temp[..n]);
            if let Some(pos) = header_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head_str = String::from_utf8_lossy(&header_buf[..pos]);
                let status_line = head_str.lines().next().unwrap();
                status = status_line.split(' ').nth(1).unwrap().parse().unwrap();
                client.buf.extend_from_slice(&header_buf[pos + 4..]);
                break;
            }
        }
        (status, client)
    }

    async fn read_body(&mut self) -> String {
        let mut out = String::from_utf8_lossy(&self.buf).into_owned();
        let mut tmp = [0u8; 512];
        while let Ok(n) = self.stream.read(&mut tmp).await {
            if n == 0 {
                break;
            }
            out.push_str(&String::from_utf8_lossy(&tmp[..n]));
        }
        out
    }

    async fn next_event(&mut self, timeout: Duration) -> Option<SseEvent> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let sep_pos = self
                .buf
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| (p, 4))
                .or_else(|| {
                    self.buf
                        .windows(2)
                        .position(|w| w == b"\n\n")
                        .map(|p| (p, 2))
                });

            if let Some((pos, sep_len)) = sep_pos {
                let chunk = self.buf.drain(..pos).collect::<Vec<u8>>();
                self.buf.drain(..sep_len);
                let s = String::from_utf8_lossy(&chunk);
                let mut id = None;
                let mut event = None;
                let mut data = None;
                let mut comment = None;

                for line in s.lines() {
                    let line = line.trim_end();
                    if let Some(c) = line.strip_prefix(": ") {
                        comment = Some(c.to_string());
                    } else if let Some(c) = line.strip_prefix(':') {
                        comment = Some(c.to_string());
                    } else if let Some(v) = line.strip_prefix("id: ") {
                        id = Some(v.to_string());
                    } else if let Some(v) = line.strip_prefix("event: ") {
                        event = Some(v.to_string());
                    } else if let Some(v) = line.strip_prefix("data: ") {
                        data = Some(v.to_string());
                    }
                }
                return Some(SseEvent {
                    id,
                    event,
                    data,
                    comment,
                });
            }

            let now = tokio::time::Instant::now();
            if now >= deadline {
                return None;
            }
            let remaining = deadline - now;
            let mut tmp = [0u8; 512];
            tokio::select! {
                res = self.stream.read(&mut tmp) => {
                    match res {
                        Ok(0) => return None,
                        Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                        Err(_) => return None,
                    }
                }
                _ = tokio::time::sleep(remaining) => {
                    return None;
                }
            }
        }
    }
}

struct TestServer {
    addr: SocketAddr,
    shutdown_tx: watch::Sender<bool>,
    _task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl TestServer {
    async fn start(root: &Path) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let state = AppState::new(root.to_path_buf(), Token::new(SECRET));
        let listener = orch_api::bind("127.0.0.1", 0).await.unwrap();
        let addr = listener.local_addr().unwrap();

        let Api { router, .. } = build_with(state.clone(), |api| api);
        let mut shutdown_clone = shutdown_rx.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_clone.wait_for(|&s| s).await;
                })
                .await
        });

        TestServer {
            addr,
            shutdown_tx,
            _task: task,
        }
    }

    fn stop(self) {
        let _ = self.shutdown_tx.send(true);
        StreamHub::reset_all();
    }
}

fn create_test_run(dir: &Path) -> Store {
    let orch_dir = dir.join(".orchestrate");
    let mut store = Store::open_in(&orch_dir).unwrap();
    store
        .apply_sync(&SyncInput {
            settings: RunSettings {
                branch: "main".into(),
                base: None,
                title: "Live Stream Test".into(),
                commits: "per-milestone".into(),
                isolation: "worktree".into(),
                max_parallel: 2,
                setup_inputs: vec![],
                scope: "warn".into(),
                risk_hold: true,
                budget_s: None,
                check_command: "".into(),
                setup_command: "none".into(),
                lead_mode: "events".into(),
                reviewer: None,
            },
            agents: vec![RunAgent {
                role: AgentRole::Lead,
                name: "lead".into(),
                kind: "claude".into(),
                model: None,
                tier: None,
                detect: true,
                pane: None,
            }],
            tasks: vec![PlanTask {
                id: "M1".into(),
                title: "First milestone".into(),
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
    store
}

#[tokio::test]
async fn test_ac4_token_auth_and_unauthorized() {
    let _lock = TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let srv = TestServer::start(dir.path()).await;

    // 1. Without token: returns 401 in shared error format before any events
    let (status, mut client) = SseClient::connect(srv.addr, "/api/v1/stream", &[]).await;
    assert_eq!(status, 401);
    let body = client.read_body().await;
    let json: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["error"]["code"], "unauthorized");

    // 2. With wrong token: returns 401
    let (status, mut client_bad) = SseClient::connect(
        srv.addr,
        "/api/v1/stream",
        &[("Authorization", "Bearer wrong-token")],
    )
    .await;
    assert_eq!(status, 401);
    let body_bad = client_bad.read_body().await;
    let json_bad: Value = serde_json::from_str(&body_bad).unwrap();
    assert_eq!(json_bad["error"]["code"], "unauthorized");

    // 3. With Bearer token: 200 OK
    let (status, _client) = SseClient::connect(
        srv.addr,
        "/api/v1/stream",
        &[("Authorization", &format!("Bearer {SECRET}"))],
    )
    .await;
    assert_eq!(status, 200);

    // 4. With ?token= query parameter: 200 OK
    let (status, _client) =
        SseClient::connect(srv.addr, &format!("/api/v1/stream?token={SECRET}"), &[]).await;
    assert_eq!(status, 200);

    srv.stop();
}

#[tokio::test]
async fn test_ac1_and_ac5_real_run_db_and_events_jsonl_and_log() {
    let _lock = TEST_LOCK.lock().await;
    unsafe {
        std::env::set_var("ORCH_STREAM_POLL_MS", "40");
    }
    let dir = tempfile::tempdir().unwrap();
    let mut store = create_test_run(dir.path());
    let srv = TestServer::start(dir.path()).await;

    let (status, mut client) =
        SseClient::connect(srv.addr, &format!("/api/v1/stream?token={SECRET}"), &[]).await;
    assert_eq!(status, 200);

    // 1. Task state transition in SQLite state.db
    store
        .transition(
            "M1",
            &TransitionRequest::auto(State::Coding, Some("start coding")),
        )
        .unwrap();

    let ev1 = client
        .next_event(Duration::from_millis(2000))
        .await
        .expect("expected task-state event");
    assert_eq!(ev1.event.as_deref(), Some("task-state"));
    assert_eq!(ev1.id.as_deref(), Some("1"));
    let data1: Value = serde_json::from_str(&ev1.data.unwrap()).unwrap();
    assert_eq!(data1["task"], "M1");
    assert_eq!(data1["status"], "coding");
    assert_eq!(data1["to"], "coding");
    assert_eq!(data1["why"], "start coding");

    // 2. Lead queue entry in SQLite state.db
    store
        .enqueue_lead_event(LeadKind::NeedsAnswer, Some("M1"), "need clarification")
        .unwrap();

    let ev2 = client
        .next_event(Duration::from_millis(2000))
        .await
        .expect("expected lead-event event");
    assert_eq!(ev2.event.as_deref(), Some("lead-event"));
    assert_eq!(ev2.id.as_deref(), Some("2"));
    let data2: Value = serde_json::from_str(&ev2.data.unwrap()).unwrap();
    assert_eq!(data2["kind"], "needs-answer");
    assert_eq!(data2["task"], "M1");
    assert_eq!(data2["text"], "need clarification");

    // 3. Write to .orchestrate/events.jsonl
    let events_jsonl = dir.path().join(".orchestrate/events.jsonl");
    let line = "{\"event\": \"run-started\", \"branch\": \"main\"}\n";
    std::fs::write(&events_jsonl, line).unwrap();

    let ev3 = client
        .next_event(Duration::from_millis(2000))
        .await
        .expect("expected run-state event");
    assert_eq!(ev3.event.as_deref(), Some("run-state"));
    assert_eq!(ev3.id.as_deref(), Some("3"));
    let data3: Value = serde_json::from_str(&ev3.data.unwrap()).unwrap();
    assert_eq!(data3["event"], "run-started");

    // 4. Write to .orchestrate/autopilot.log
    let log_file = dir.path().join(".orchestrate/autopilot.log");
    std::fs::write(&log_file, "[12:00:00] manager started\n").unwrap();

    let ev4 = client
        .next_event(Duration::from_millis(2000))
        .await
        .expect("expected log event");
    assert_eq!(ev4.event.as_deref(), Some("log"));
    assert_eq!(ev4.id.as_deref(), Some("4"));
    let data4: Value = serde_json::from_str(&ev4.data.unwrap()).unwrap();
    assert!(
        data4["line"]
            .as_str()
            .unwrap()
            .contains("[12:00:00] manager started")
    );

    unsafe {
        std::env::remove_var("ORCH_STREAM_POLL_MS");
    }
    srv.stop();
}

#[tokio::test]
async fn test_ac2_reconnect_last_event_id_and_since() {
    let _lock = TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let srv = TestServer::start(dir.path()).await;

    let hub = StreamHub::get_or_create(dir.path(), watch::channel(false).1);
    hub.publish("run-state", serde_json::json!({"step": 1}));
    hub.publish("run-state", serde_json::json!({"step": 2}));
    hub.publish("run-state", serde_json::json!({"step": 3}));
    hub.publish("run-state", serde_json::json!({"step": 4}));

    // Reconnect with Last-Event-ID: 2 -> receives events 3 and 4 in order
    let (status, mut client) = SseClient::connect(
        srv.addr,
        &format!("/api/v1/stream?token={SECRET}"),
        &[("Last-Event-ID", "2")],
    )
    .await;
    assert_eq!(status, 200);

    let ev3 = client
        .next_event(Duration::from_millis(500))
        .await
        .expect("expected event 3");
    assert_eq!(ev3.id.as_deref(), Some("3"));
    let d3: Value = serde_json::from_str(&ev3.data.unwrap()).unwrap();
    assert_eq!(d3["step"], 3);

    let ev4 = client
        .next_event(Duration::from_millis(500))
        .await
        .expect("expected event 4");
    assert_eq!(ev4.id.as_deref(), Some("4"));
    let d4: Value = serde_json::from_str(&ev4.data.unwrap()).unwrap();
    assert_eq!(d4["step"], 4);

    // Reconnect with ?since=3 -> receives event 4 only
    let (status, mut client2) = SseClient::connect(
        srv.addr,
        &format!("/api/v1/stream?token={SECRET}&since=3"),
        &[],
    )
    .await;
    assert_eq!(status, 200);

    let ev = client2
        .next_event(Duration::from_millis(500))
        .await
        .expect("expected event 4");
    assert_eq!(ev.id.as_deref(), Some("4"));
    let d: Value = serde_json::from_str(&ev.data.unwrap()).unwrap();
    assert_eq!(d["step"], 4);

    srv.stop();
}

#[tokio::test]
async fn test_ac2_client_asking_for_id_older_than_kept_gets_refetch_status() {
    let _lock = TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("ORCH_STREAM_BUFFER_SIZE", "3");
    }
    let srv = TestServer::start(dir.path()).await;

    let hub = StreamHub::get_or_create(dir.path(), watch::channel(false).1);
    for i in 1..=6 {
        hub.publish("run-state", serde_json::json!({"item": i}));
    }
    // Buffer size is 3, so events 1, 2, 3 were evicted; kept: 4, 5, 6

    // Client reconnects asking for id 1 (older than kept)
    let (status, mut client) = SseClient::connect(
        srv.addr,
        &format!("/api/v1/stream?token={SECRET}&since=1"),
        &[],
    )
    .await;
    assert_eq!(status, 200);

    let refetch_ev = client
        .next_event(Duration::from_millis(500))
        .await
        .expect("expected refetch status message");
    let data: Value = serde_json::from_str(&refetch_ev.data.unwrap()).unwrap();
    assert_eq!(data["action"], "refetch_status");
    assert!(
        data["message"]
            .as_str()
            .unwrap()
            .contains("re-fetch status")
    );

    unsafe {
        std::env::remove_var("ORCH_STREAM_BUFFER_SIZE");
    }
    srv.stop();
}

#[tokio::test]
async fn test_ac3_keep_alive_comment_on_idle_connection() {
    let _lock = TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("ORCH_STREAM_KEEPALIVE_MS", "50");
    }
    let srv = TestServer::start(dir.path()).await;

    let (status, mut client) =
        SseClient::connect(srv.addr, &format!("/api/v1/stream?token={SECRET}"), &[]).await;
    assert_eq!(status, 200);

    // Idle connection: wait for keepalive comment
    let ev = client
        .next_event(Duration::from_millis(600))
        .await
        .expect("expected keep-alive comment");
    assert_eq!(ev.comment.as_deref(), Some("keep-alive"));

    unsafe {
        std::env::remove_var("ORCH_STREAM_KEEPALIVE_MS");
    }
    srv.stop();
}

#[tokio::test]
async fn test_ac3_concurrency_bound_and_refusal_with_several_clients() {
    let _lock = TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("ORCH_STREAM_MAX_CLIENTS", "2");
        std::env::set_var("ORCH_STREAM_KEEPALIVE_MS", "40");
    }
    let srv = TestServer::start(dir.path()).await;

    // Connect client 1
    let (status1, client1) =
        SseClient::connect(srv.addr, &format!("/api/v1/stream?token={SECRET}"), &[]).await;
    assert_eq!(status1, 200);

    // Connect client 2
    let (status2, _client2) =
        SseClient::connect(srv.addr, &format!("/api/v1/stream?token={SECRET}"), &[]).await;
    assert_eq!(status2, 200);

    // Connect client 3: bound is 2 -> clear refusal (429 Too Many Requests)
    let (status3, mut client3) =
        SseClient::connect(srv.addr, &format!("/api/v1/stream?token={SECRET}"), &[]).await;
    assert_eq!(status3, 429);
    let err_body = client3.read_body().await;
    let err_json: Value = serde_json::from_str(&err_body).unwrap();
    assert_eq!(err_json["error"]["code"], "too_many_clients");

    // Disconnect client 1
    drop(client1);

    // Wait for keep-alive to detect write failure on closed socket and drop guard
    let hub = StreamHub::get_or_create(dir.path(), watch::channel(false).1);
    for _ in 0..10 {
        hub.publish("run-state", serde_json::json!({"tick": 1}));
        tokio::time::sleep(Duration::from_millis(20)).await;
        if hub.active_clients.load(std::sync::atomic::Ordering::SeqCst) < 2 {
            break;
        }
    }

    // Connect client 4: now succeeds because slot was freed
    let (status4, _client4) =
        SseClient::connect(srv.addr, &format!("/api/v1/stream?token={SECRET}"), &[]).await;
    assert_eq!(status4, 200);

    unsafe {
        std::env::remove_var("ORCH_STREAM_MAX_CLIENTS");
        std::env::remove_var("ORCH_STREAM_KEEPALIVE_MS");
    }
    srv.stop();
}

#[tokio::test]
async fn test_ac3_slow_client_does_not_block_server_or_others() {
    let _lock = TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let srv = TestServer::start(dir.path()).await;

    let (status_slow, _slow_client) =
        SseClient::connect(srv.addr, &format!("/api/v1/stream?token={SECRET}"), &[]).await;
    assert_eq!(status_slow, 200);

    let (status_fast, mut fast_client) =
        SseClient::connect(srv.addr, &format!("/api/v1/stream?token={SECRET}"), &[]).await;
    assert_eq!(status_fast, 200);

    let hub = StreamHub::get_or_create(dir.path(), watch::channel(false).1);
    // Publish a burst of 150 events (more than single channel capacity of 128)
    for i in 1..=150 {
        hub.publish("run-state", serde_json::json!({"seq": i}));
    }

    // Fast client receives messages immediately without being blocked by the slow client
    let mut count = 0;
    while count < 100 {
        if let Some(ev) = fast_client.next_event(Duration::from_millis(200)).await {
            if ev.event.as_deref() == Some("run-state") {
                count += 1;
            }
        } else {
            break;
        }
    }
    assert!(count >= 50, "fast client received {count} events promptly");

    srv.stop();
}

#[tokio::test]
async fn test_ac6_openapi_document_media_type_and_shapes() {
    let _lock = TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let srv = TestServer::start(dir.path()).await;

    let (status, mut client) = SseClient::connect(
        srv.addr,
        "/api/v1/openapi.json",
        &[("Authorization", &format!("Bearer {SECRET}"))],
    )
    .await;
    assert_eq!(status, 200);

    let body = client.read_body().await;
    let doc: Value = serde_json::from_str(&body).unwrap();

    // Verify /api/v1/stream route
    let stream_op = &doc["paths"]["/api/v1/stream"]["get"];
    assert!(stream_op.is_object(), "missing /api/v1/stream get operation");

    // Verify 200 content-type is text/event-stream
    let stream_resp = &stream_op["responses"]["200"]["content"]["text/event-stream"];
    assert!(
        stream_resp.is_object(),
        "missing text/event-stream response for /api/v1/stream"
    );

    // Verify components schemas
    let schemas = &doc["components"]["schemas"];
    assert!(schemas["StreamMessage"].is_object());
    assert!(schemas["TaskStateEvent"].is_object());
    assert!(schemas["LeadQueueEvent"].is_object());
    assert!(schemas["RunStateEvent"].is_object());
    assert!(schemas["LogEvent"].is_object());

    srv.stop();
}

async fn api_req(addr: SocketAddr, method: &str, path: &str, body: Option<&str>) -> (u16, Value) {
    let b = body.unwrap_or("");
    let ct = if body.is_some() {
        "Content-Type: application/json\r\n"
    } else {
        ""
    };
    let text = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nAuthorization: Bearer {SECRET}\r\n{ct}Content-Length: {}\r\n\r\n{b}",
        b.len()
    );
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(text.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let resp = String::from_utf8_lossy(&raw);
    let status = resp
        .lines()
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body_str = resp.split("\r\n\r\n").nth(1).unwrap_or("{}");
    let json: Value = serde_json::from_str(body_str).unwrap_or(Value::Null);
    (status, json)
}

#[tokio::test]
async fn test_run_control_changes_reported_on_stream_and_reconnect() {
    let _lock = TEST_LOCK.lock().await;
    unsafe {
        std::env::set_var("ORCH_STREAM_POLL_MS", "30");
    }
    let dir = tempfile::tempdir().unwrap();
    let _store = create_test_run(dir.path());
    let srv = TestServer::start(dir.path()).await;

    // Connect SSE client
    let (status, mut client) =
        SseClient::connect(srv.addr, &format!("/api/v1/stream?token={SECRET}"), &[]).await;
    assert_eq!(status, 200);

    // 1. POST /api/v1/run/pause
    let (s, resp) = api_req(srv.addr, "POST", "/api/v1/run/pause", None).await;
    assert_eq!(s, 200, "{resp}");

    let ev_pause = client
        .next_event(Duration::from_millis(2000))
        .await
        .expect("expected run-state event on pause");
    assert_eq!(ev_pause.event.as_deref(), Some("run-state"));
    assert!(ev_pause.id.is_some(), "event must have an id");
    let data_pause: Value = serde_json::from_str(&ev_pause.data.unwrap()).unwrap();
    assert_eq!(data_pause["run_control"], "paused");
    assert_eq!(data_pause["source"], "api");
    assert_eq!(data_pause["action"], "pause");

    // 2. POST /api/v1/run/resume
    let (s, resp) = api_req(srv.addr, "POST", "/api/v1/run/resume", None).await;
    assert_eq!(s, 200, "{resp}");

    let ev_resume = client
        .next_event(Duration::from_millis(2000))
        .await
        .expect("expected run-state event on resume");
    assert_eq!(ev_resume.event.as_deref(), Some("run-state"));
    let resume_id = ev_resume.id.clone().expect("event must have an id");
    let data_resume: Value = serde_json::from_str(&ev_resume.data.unwrap()).unwrap();
    assert_eq!(data_resume["run_control"], "running");
    assert_eq!(data_resume["source"], "api");
    assert_eq!(data_resume["action"], "resume");

    // 3. Disconnect client and call POST /api/v1/run/pause while disconnected
    drop(client);

    let (s, resp) = api_req(srv.addr, "POST", "/api/v1/run/pause", None).await;
    assert_eq!(s, 200, "{resp}");

    // Give the poller time to pick it up and publish to history
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 4. Reconnect with Last-Event-ID set to resume_id
    let (status, mut client2) = SseClient::connect(
        srv.addr,
        "/api/v1/stream",
        &[
            ("Authorization", &format!("Bearer {SECRET}")),
            ("Last-Event-ID", &resume_id),
        ],
    )
    .await;
    assert_eq!(status, 200);

    let replayed = client2
        .next_event(Duration::from_millis(2000))
        .await
        .expect("expected replayed run-state event after resume_id");
    assert_eq!(replayed.event.as_deref(), Some("run-state"));
    let data_replayed: Value = serde_json::from_str(&replayed.data.unwrap()).unwrap();
    assert_eq!(data_replayed["run_control"], "paused");
    assert_eq!(data_replayed["source"], "api");
    assert_eq!(data_replayed["action"], "pause");

    // 5. POST /api/v1/run/stop
    let (s, resp) = api_req(srv.addr, "POST", "/api/v1/run/stop", None).await;
    assert_eq!(s, 200, "{resp}");

    let ev_stop = client2
        .next_event(Duration::from_millis(2000))
        .await
        .expect("expected run-state event on stop");
    assert_eq!(ev_stop.event.as_deref(), Some("run-state"));
    let data_stop: Value = serde_json::from_str(&ev_stop.data.unwrap()).unwrap();
    assert_eq!(data_stop["run_control"], "stopped");
    assert_eq!(data_stop["source"], "api");
    assert_eq!(data_stop["action"], "stop");

    unsafe {
        std::env::remove_var("ORCH_STREAM_POLL_MS");
    }
    srv.stop();
}

#[tokio::test]
async fn test_run_control_changes_via_cli_and_direct_store() {
    let _lock = TEST_LOCK.lock().await;
    unsafe {
        std::env::set_var("ORCH_STREAM_POLL_MS", "30");
    }
    let dir = tempfile::tempdir().unwrap();
    let mut store = create_test_run(dir.path());
    let srv = TestServer::start(dir.path()).await;

    // Connect SSE client
    let (status, mut client) =
        SseClient::connect(srv.addr, &format!("/api/v1/stream?token={SECRET}"), &[]).await;
    assert_eq!(status, 200);

    // Update via store (CLI simulation with record_action)
    store.set_run_control(RunControl::Paused, Source::Cli).unwrap();
    store.record_action(Source::Cli, "pause", None, "").unwrap();

    let ev = client
        .next_event(Duration::from_millis(2000))
        .await
        .expect("expected run-state event on CLI pause");
    assert_eq!(ev.event.as_deref(), Some("run-state"));
    let data: Value = serde_json::from_str(&ev.data.unwrap()).unwrap();
    assert_eq!(data["run_control"], "paused");
    assert_eq!(data["source"], "cli");
    assert_eq!(data["action"], "pause");

    // Direct update without record_action (e.g. event manager startup / direct change)
    store.set_run_control(RunControl::Running, Source::Manager).unwrap();

    let ev2 = client
        .next_event(Duration::from_millis(2000))
        .await
        .expect("expected run-state event on manager resume");
    assert_eq!(ev2.event.as_deref(), Some("run-state"));
    let data2: Value = serde_json::from_str(&ev2.data.unwrap()).unwrap();
    assert_eq!(data2["run_control"], "running");
    assert_eq!(data2["source"], "manager");

    unsafe {
        std::env::remove_var("ORCH_STREAM_POLL_MS");
    }
    srv.stop();
}


// A `chat` event for every stored message and every change of its delivery state.
#[tokio::test]
async fn a_chat_event_follows_every_stored_message_and_delivery_change() {
    use orch_core::chat::{ChatKind, ChatTick, chat_tick};
    use orch_core::wake::{Clock, LeadPane, LeadRuntime, Notifier, SystemClock, WakeEnv, WakeTunables};

    struct Idle;
    impl LeadRuntime for Idle {
        fn pane(&self, _: &str) -> LeadPane {
            LeadPane::Idle
        }
        fn send(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
    }
    struct Quiet;
    impl Notifier for Quiet {
        fn notify(&self, _: &str) {}
    }
    let _ = SystemClock.now();

    let _lock = TEST_LOCK.lock().await;
    unsafe {
        std::env::set_var("ORCH_STREAM_POLL_MS", "40");
    }
    let dir = tempfile::tempdir().unwrap();
    let mut store = create_test_run(dir.path());
    let srv = TestServer::start(dir.path()).await;
    let (status, mut client) =
        SseClient::connect(srv.addr, &format!("/api/v1/stream?token={SECRET}"), &[]).await;
    assert_eq!(status, 200);

    // stored: queued
    let m = store
        .post_developer_message(ChatKind::Message, "hello", None)
        .unwrap();
    let e = client.next_event(Duration::from_millis(2000)).await.expect("stored");
    assert_eq!(e.event.as_deref(), Some("chat"));
    let d: Value = serde_json::from_str(&e.data.unwrap()).unwrap();
    assert_eq!((d["id"].as_i64(), d["delivery"].as_str(), d["sender"].as_str()), (Some(m.id), Some("queued"), Some("developer")));
    assert!(d["ts"].is_string());

    // delivered: the event manager records the attempt, then `sent`
    let env = WakeEnv { runtime: &Idle, notifier: &Quiet, clock: &SystemClock, tun: WakeTunables::default() };
    assert_eq!(chat_tick(&mut store, &env, &mut |_| {}).unwrap(), ChatTick::Sent(m.id));
    let mut last = Value::Null;
    while let Some(e) = client.next_event(Duration::from_millis(600)).await {
        assert_eq!(e.event.as_deref(), Some("chat"));
        last = serde_json::from_str(&e.data.unwrap()).unwrap();
    }
    assert_eq!(last["delivery"], "sent", "{last}");

    // the lead's reply, and the acknowledgement it causes
    store.post_lead_reply("on it", Some("M1")).unwrap();
    let mut seen = Vec::new();
    while let Some(e) = client.next_event(Duration::from_millis(600)).await {
        seen.push(serde_json::from_str::<Value>(&e.data.unwrap()).unwrap());
    }
    assert!(seen.iter().any(|d| d["sender"] == "lead" && d["about"] == "M1" && d["delivery"].is_null()), "{seen:?}");
    assert!(seen.iter().any(|d| d["id"].as_i64() == Some(m.id) && d["delivery"] == "acknowledged"), "{seen:?}");
    srv.stop();
}
