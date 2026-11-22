//! The read endpoints against a real run database (built through `orch_core`'s store, as
//! `orch sync` does), over real sockets with the real router.

use orch_api::{Api, AppState, Token, build};
use orch_core::check::Records;
use orch_core::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::store::{Counter, Store, TransitionRequest};
use orch_core::task::State;
use orch_herdr::fake::{CallKind, FakeHerdr};
use orch_herdr::model::{AgentInfo, PaneInfo};
use orch_herdr::{AgentStatus, HerdrError, RuntimeAdapter};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

const PLAN: &str = "\
# Plan: Demo
Branch: feature/demo
Base: main
Check command: cargo test
Workers: claude-1 (claude: sonnet), codex-1 (codex: gpt-5)
Lead: lead (claude: opus)
Reviewer: none

## Feature breakdown
- the first part
- the second part

## [x] M1: One
Depends on: none
Tier: medium
Needs: coding, reasoning
Model why: normal work
Estimate: 30m
Role: backend engineer
Review: yes
Likely files: src/a.rs, src/b.rs
Goal:
- make a
Why:
- because
Acceptance criteria:
- works
- is tested
Open questions:
- none

## [x] M2: Two
Depends on: M1 (needs the table)
Why separate: second part
Tier: high
Model why: hard work
Goal:
- make b
Acceptance criteria:
- works

## [x] M3: Three
Depends on: M1
Tier: small
Goal:
- g
Acceptance criteria:
- works

## [x] M4: Four
Depends on: M1
Goal:
- g
Acceptance criteria:
- works

## [x] M5: Five
Depends on: M1
Goal:
- g
Acceptance criteria:
- works
";

fn settings() -> RunSettings {
    RunSettings {
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
    }
}

fn agent(role: AgentRole, name: &str, kind: &str, model: &str, tier: Option<&str>) -> RunAgent {
    RunAgent {
        role,
        name: name.into(),
        kind: kind.into(),
        model: Some(model.into()),
        tier: tier.map(String::from),
        detect: true,
        pane: None,
    }
}

fn plan_task(id: &str, title: &str, deps: &[&str], tier: Option<&str>) -> PlanTask {
    PlanTask {
        id: id.into(),
        title: title.into(),
        deps: deps.iter().map(|d| d.to_string()).collect(),
        enabled: true,
        open_questions: false,
        tier: tier.map(String::from),
        needs: vec!["coding".into()],
        model_why: "why".into(),
        prefer: "any".into(),
        review: true,
        files: vec!["src/".into()],
        parallel: false,
    }
}

/// A project folder with plan.md and a stored run of five milestones, none started.
fn fresh_run(root: &Path) -> Store {
    let dir = root.join(".orchestrate");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plan.md"), PLAN).unwrap();
    let mut store = Store::open_in(&dir).unwrap();
    store
        .apply_sync(&SyncInput {
            settings: settings(),
            agents: vec![
                agent(
                    AgentRole::Worker,
                    "claude-1",
                    "claude",
                    "sonnet",
                    Some("medium"),
                ),
                agent(AgentRole::Worker, "codex-1", "codex", "gpt-5", None),
                agent(AgentRole::Lead, "lead", "claude", "opus", Some("high")),
            ],
            tasks: vec![
                plan_task("M1", "One", &[], Some("medium")),
                plan_task("M2", "Two", &["M1"], Some("high")),
                plan_task("M3", "Three", &["M1"], Some("small")),
                plan_task("M4", "Four", &["M1"], None),
                plan_task("M5", "Five", &["M1"], None),
            ],
        })
        .unwrap();
    store
}

fn go(store: &mut Store, id: &str, to: State, why: &str) {
    store
        .transition(id, &TransitionRequest::auto(to, Some(why)))
        .unwrap();
}

/// M1 merged after a fix round, M2 held, M3 paused, M4 failed, M5 coding on claude-1.
fn busy_run(root: &Path) -> Store {
    let mut s = fresh_run(root);
    s.assign_worker("M1", Some("claude-1"), Some("sonnet"))
        .unwrap();
    s.bump("M1", Counter::Attempt).unwrap();
    for to in [
        State::Coding,
        State::Verifying,
        State::NeedsFix,
        State::Coding,
        State::Verifying,
    ] {
        go(&mut s, "M1", to, "step");
    }
    s.bump("M1", Counter::Fix).unwrap();
    go(&mut s, "M1", State::Merged, "landed");
    s.assign_worker("M1", None, None).unwrap();

    s.assign_worker("M2", Some("codex-1"), Some("gpt-5"))
        .unwrap();
    s.bump("M2", Counter::Attempt).unwrap();
    for to in [State::Coding, State::Verifying, State::Held] {
        go(&mut s, "M2", to, "risky");
    }
    go(&mut s, "M3", State::Paused, "agent stopped");
    go(&mut s, "M4", State::Failed, "still failing");
    s.assign_worker("M5", Some("claude-1"), Some("sonnet"))
        .unwrap();
    s.bump("M5", Counter::Attempt).unwrap();
    go(&mut s, "M5", State::Coding, "start");
    s
}

fn write_files(root: &Path) {
    let task = root.join(".orch-task/M2");
    std::fs::create_dir_all(&task).unwrap();
    std::fs::write(
        task.join("result-1.json"),
        r#"{"status":"done","commit":"feat: two","files_changed":["src/a.rs","docs/x.md"],
            "check":{"ran":true,"passed":true,"summary":"ok"},
            "criteria":[{"id":"AC1","met":true,"evidence":"test"}],"questions":[]}"#,
    )
    .unwrap();
    std::fs::write(task.join("result-2.json"), "not json").unwrap();
    let rev = root.join(".orchestrate/review/M2");
    std::fs::create_dir_all(&rev).unwrap();
    std::fs::write(
        rev.join("review-1.json"),
        r#"{"outcome":"changes-requested",
            "criteria":[{"id":"AC1","met":false,"note":"missing test"}],
            "blockers":[{"file":"src/a.rs","line":4,"problem":"bug","why":"breaks","criterion":"AC1"}],
            "suggestions":[{"file":"src/b.rs","problem":"rename"}]}"#,
    )
    .unwrap();
    std::fs::write(rev.join("review-2.json"), "").unwrap();
    let mut rec = Records::default();
    rec.0.entry("M2".into()).or_default();
    rec.save(root).unwrap();
    Records::record_check(
        root,
        "M2",
        "worktree",
        &orch_core::check::run("true", root, std::time::Duration::from_secs(5), 5),
    )
    .unwrap();
    Records::set_out_of_scope(root, "M2", &["docs/x.md".to_string()]).unwrap();
    std::fs::write(
        root.join(".orchestrate/autopilot.log"),
        "[10:00:01] M1 coding on claude-1\n[10:00:02] M2 held: risky\n[10:00:03] codex-1 said \u{1b}[31mhello\u{1b}[0m \u{7}bell\n[10:00:04] M12 unrelated\n[10:00:05] M2 again\n",
    )
    .unwrap();
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

async fn start_with_herdr(root: &Path, herdr: Arc<dyn RuntimeAdapter + Send + Sync>) -> Server {
    let state = AppState::with_herdr(root.to_path_buf(), Token::new(SECRET), herdr);
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

async fn get(srv: &Server, path: &str) -> (u16, Value) {
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAuthorization: Bearer {SECRET}\r\n\r\n",
        srv.addr
    );
    let mut s = TcpStream::connect(srv.addr).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
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

async fn ok(srv: &Server, path: &str) -> Value {
    let (s, v) = get(srv, path).await;
    assert_eq!(s, 200, "{path}: {v}");
    v
}

fn states(v: &Value) -> Vec<(String, String)> {
    v["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["id"].as_str().unwrap().into(),
                t["state"].as_str().unwrap().into(),
            )
        })
        .collect()
}

#[tokio::test]
async fn status_of_a_run_with_nothing_started() {
    let dir = tempfile::tempdir().unwrap();
    fresh_run(dir.path());
    let srv = start(dir.path()).await;
    let v = ok(&srv, "/api/v1/status").await;
    assert_eq!(v["branch"], "feature/demo");
    assert_eq!(v["base"], "main");
    assert_eq!(v["isolation"], "worktree");
    assert_eq!(v["run_state"], "idle");
    assert_eq!(v["all_done"], false);
    assert_eq!(v["event_manager"]["running"], false);
    assert_eq!(v["events"]["unread"], 0);
    assert_eq!(v["events"]["open"], 0);
    let progress = v["progress"].as_array().unwrap();
    assert_eq!(progress.len(), 15);
    let pending = progress.iter().find(|p| p["state"] == "pending").unwrap();
    assert_eq!(pending["count"], 5);
    let w = &v["workers"][0];
    assert_eq!(
        (w["name"].as_str(), w["model"].as_str(), w["tier"].as_str()),
        (Some("claude-1"), Some("sonnet"), Some("medium"))
    );
    assert!(w["task"].is_null());
    assert_eq!(v["workers"].as_array().unwrap().len(), 2);
    // Reading changed nothing.
    assert!(!dir.path().join(".orch-task").exists());
}

#[tokio::test]
async fn status_with_held_paused_failed_and_running_milestones() {
    let dir = tempfile::tempdir().unwrap();
    busy_run(dir.path());
    let srv = start(dir.path()).await;
    let v = ok(&srv, "/api/v1/status").await;
    assert_eq!(v["run_state"], "running");
    assert_eq!(v["events"]["unread"], 4);
    assert_eq!(v["events"]["open"], 0);
    let count = |s: &str| {
        v["progress"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["state"] == s)
            .unwrap()["count"]
            .clone()
    };
    assert_eq!(
        (
            count("merged"),
            count("held"),
            count("paused"),
            count("failed"),
            count("coding")
        ),
        (1.into(), 1.into(), 1.into(), 1.into(), 1.into())
    );
    let claude = v["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["name"] == "claude-1")
        .unwrap();
    assert_eq!(claude["task"], "M5");
    let t: Vec<_> = states(&v);
    assert_eq!(t[1], ("M2".into(), "held".into()));
    assert_eq!(v["tasks"][1]["tier"], "high");
}

#[tokio::test]
async fn run_state_stopped_all_done_and_broken_branch() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = busy_run(dir.path());
    // M2 and M5 parked: nothing running, nothing ready, the developer is needed.
    for id in ["M2", "M5"] {
        s.manual_set(id, State::Paused, "stopped", true).unwrap();
    }
    let srv = start(dir.path()).await;
    assert_eq!(ok(&srv, "/api/v1/status").await["run_state"], "stopped");
    assert_eq!(ok(&srv, "/api/v1/status").await["stuck"], true);

    s.mark_integration_broken("check fails: 2 tests").unwrap();
    let v = ok(&srv, "/api/v1/status").await;
    assert_eq!(v["run_state"], "broken_branch");
    assert_eq!(v["broken_branch"]["reason"], "check fails: 2 tests");
    s.clear_integration_broken().unwrap();

    for id in ["M2", "M3", "M4", "M5"] {
        s.manual_set(id, State::Skipped, "skip", true).unwrap();
    }
    let v = ok(&srv, "/api/v1/status").await;
    assert_eq!(
        (v["run_state"].as_str(), v["all_done"].as_bool()),
        (Some("all_done"), Some(true))
    );
}

#[tokio::test]
async fn plan_has_every_milestone_field() {
    let dir = tempfile::tempdir().unwrap();
    fresh_run(dir.path());
    let srv = start(dir.path()).await;
    let v = ok(&srv, "/api/v1/plan").await;
    assert_eq!(v["title"], "Demo");
    assert_eq!(v["header"]["branch"], "feature/demo");
    assert_eq!(v["header"]["check_command"], "cargo test");
    assert!(
        v["header"]["workers"]
            .as_str()
            .unwrap()
            .contains("claude-1")
    );
    assert!(
        v["feature_breakdown"]
            .as_str()
            .unwrap()
            .contains("the first part")
    );
    let ms = v["milestones"].as_array().unwrap();
    assert_eq!(ms.len(), 5);
    let m1 = &ms[0];
    assert_eq!(m1["id"], "M1");
    assert_eq!(m1["goal"], "- make a");
    assert_eq!(m1["why"], "- because");
    assert_eq!(m1["tier"], "medium");
    assert_eq!(m1["needs"], serde_json::json!(["coding", "reasoning"]));
    assert_eq!(m1["model_why"], "normal work");
    assert_eq!(m1["estimate"], "30m");
    assert_eq!(m1["role"], "backend engineer");
    assert_eq!(m1["review"], "yes");
    assert_eq!(
        m1["likely_files"],
        serde_json::json!(["src/a.rs", "src/b.rs"])
    );
    assert_eq!(
        m1["acceptance_criteria"][0],
        serde_json::json!({"id": "AC1", "text": "works"})
    );
    assert_eq!(m1["acceptance_criteria"][1]["id"], "AC2");
    assert_eq!(m1["has_open_questions"], false);
    assert_eq!(m1["depends_on"], serde_json::json!([]));
    let m2 = &ms[1];
    assert_eq!(m2["why_separate"], "second part");
    assert_eq!(
        m2["depends_on"],
        serde_json::json!([{"id": "M1", "reason": "needs the table"}])
    );
}

#[tokio::test]
async fn plan_problems_are_reported_not_hidden() {
    let dir = tempfile::tempdir().unwrap();
    fresh_run(dir.path());
    let srv = start(dir.path()).await;
    std::fs::write(
        dir.path().join(".orchestrate/plan.md"),
        "## [x] M1 no colon\n",
    )
    .unwrap();
    let (s, v) = get(&srv, "/api/v1/plan").await;
    assert_eq!(s, 409, "{v}");
    assert_eq!(v["error"]["code"], "conflict");
    std::fs::remove_file(dir.path().join(".orchestrate/plan.md")).unwrap();
    let (s, v) = get(&srv, "/api/v1/plan").await;
    assert_eq!((s, v["error"]["code"].as_str()), (404, Some("not_found")));
}

#[tokio::test]
async fn tasks_and_one_task_inspected() {
    let dir = tempfile::tempdir().unwrap();
    busy_run(dir.path());
    write_files(dir.path());
    let srv = start(dir.path()).await;

    let list = ok(&srv, "/api/v1/tasks").await;
    assert_eq!(
        states(&list),
        [
            ("M1", "merged"),
            ("M2", "held"),
            ("M3", "paused"),
            ("M4", "failed"),
            ("M5", "coding")
        ]
        .map(|(a, b)| (a.to_string(), b.to_string()))
    );
    let m1 = &list["tasks"][0];
    assert_eq!(
        (
            m1["attempt"].as_u64(),
            m1["fix_rounds"].as_u64(),
            m1["model"].as_str()
        ),
        (Some(1), Some(1), Some("sonnet"))
    );
    assert_eq!(m1["last_worker"], "claude-1");
    assert!(m1["worker"].is_null());
    assert_eq!(list["tasks"][2]["reason"], "agent stopped");
    assert_eq!(list["tasks"][1]["worker"], "codex-1");

    let d = ok(&srv, "/api/v1/tasks/M2").await;
    assert_eq!(d["task"]["state"], "held");
    let hist: Vec<&str> = d["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["to"].as_str().unwrap())
        .collect();
    assert_eq!(hist, ["coding", "verifying", "held"]);
    assert_eq!(d["history"][2]["why"], "risky");
    assert_eq!(d["checks"].as_array().unwrap().len(), 1);
    assert_eq!(d["checks"][0]["where"], "worktree");
    assert_eq!(d["checks"][0]["ok"], true);
    let r = d["reviews"].as_array().unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[0]["round"], 1);
    assert_eq!(r[0]["outcome"], "changes-requested");
    assert_eq!(
        r[0]["criteria"][0],
        serde_json::json!({"id": "AC1", "met": false, "note": "missing test"})
    );
    assert_eq!(r[0]["blockers"][0]["file"], "src/a.rs");
    assert_eq!(r[0]["blockers"][0]["line"], "4");
    assert_eq!(r[0]["suggestions"][0]["problem"], "rename");
    assert!(r[1]["error"].as_str().unwrap().contains("empty"));
    let res = d["results"].as_array().unwrap();
    assert_eq!(res[0]["status"], "done");
    assert_eq!(res[0]["check"]["passed"], true);
    assert_eq!(res[0]["criteria"][0]["evidence"], "test");
    assert!(res[1]["error"].as_str().unwrap().contains("not valid JSON"));
    assert_eq!(d["files_outside_likely"], serde_json::json!(["docs/x.md"]));
    // The latest result is unreadable, so no files are claimed from it.
    assert_eq!(d["files_changed"], serde_json::json!([]));
    let m3 = ok(&srv, "/api/v1/tasks/M3").await;
    assert_eq!(
        (
            m3["history"].as_array().unwrap().len(),
            m3["reviews"].as_array().unwrap().len(),
            m3["results"].as_array().unwrap().len()
        ),
        (1, 0, 0)
    );

    let (s, v) = get(&srv, "/api/v1/tasks/M99").await;
    assert_eq!((s, v["error"]["code"].as_str()), (404, Some("not_found")));
}

#[tokio::test]
async fn events_report_new_open_and_ready_without_acknowledging() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = busy_run(dir.path());
    let srv = start(dir.path()).await;
    let v = ok(&srv, "/api/v1/events").await;
    assert_eq!(v["lead_on"], true);
    let new = v["new"].as_array().unwrap();
    let kinds: Vec<&str> = new.iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["merged", "held", "paused", "failed"]);
    let e = &new[1];
    assert_eq!(
        (
            e["task"].as_str(),
            e["task_title"].as_str(),
            e["kind"].as_str()
        ),
        (Some("M2"), Some("Two"), Some("held"))
    );
    assert_eq!(
        (e["read"].as_bool(), e["open"].as_bool()),
        (Some(false), Some(true))
    );
    assert!(e["help"].as_str().unwrap().contains("orch approve M2"));
    assert!(e["text"].is_string());
    assert!(e["id"].as_i64().unwrap() > 0);
    // Asking again still shows them: GET never marks anything read.
    assert_eq!(
        ok(&srv, "/api/v1/events").await["new"]
            .as_array()
            .unwrap()
            .len(),
        4
    );

    let ids: Vec<i64> = new.iter().map(|e| e["id"].as_i64().unwrap()).collect();
    let after = ok(&srv, &format!("/api/v1/events?since={}", ids[0])).await;
    assert_eq!(after["new"].as_array().unwrap().len(), 3);
    let one = ok(&srv, "/api/v1/events?limit=1").await;
    assert_eq!(one["new"].as_array().unwrap().len(), 1);
    assert_eq!(one["new"][0]["id"], ids[0]);

    // Once the lead read them, the blocking ones stay "still open".
    store.mark_lead_read(&ids).unwrap();
    let v = ok(&srv, "/api/v1/events").await;
    assert_eq!(v["new"].as_array().unwrap().len(), 0);
    let open: Vec<&str> = v["still_open"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(open, ["held", "paused", "failed"]);
    assert_eq!(v["still_open"][0]["read"], true);

    for bad in ["limit=0", "limit=x", "since=abc", "limit=100000"] {
        let (s, v) = get(&srv, &format!("/api/v1/events?{bad}")).await;
        assert_eq!(
            (s, v["error"]["code"].as_str()),
            (400, Some("bad_request")),
            "{bad}"
        );
    }
}

#[tokio::test]
async fn events_list_the_milestones_ready_for_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = fresh_run(dir.path());
    let srv = start(dir.path()).await;
    let v = ok(&srv, "/api/v1/events").await;
    assert_eq!(
        v["ready"],
        serde_json::json!([{"task": "M1", "title": "One", "tier": "medium", "workers": ["claude-1", "codex-1"]}])
    );
    // M1 merged: the other four become ready.
    for to in [State::Coding, State::Verifying, State::Merged] {
        go(&mut s, "M1", to, "x");
    }
    let v = ok(&srv, "/api/v1/events").await;
    let ready: Vec<&str> = v["ready"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["task"].as_str().unwrap())
        .collect();
    assert_eq!(ready, ["M2", "M3", "M4", "M5"]);
    // M2 asks for `high`: only the unrated worker qualifies... and claude (medium) does not.
    assert_eq!(v["ready"][0]["workers"], serde_json::json!(["codex-1"]));
}

#[tokio::test]
async fn report_has_the_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = busy_run(dir.path());
    s.bump("M3", Counter::Lost).unwrap();
    s.bump("M4", Counter::Conflict).unwrap();
    let srv = start(dir.path()).await;
    let v = ok(&srv, "/api/v1/report").await;
    assert_eq!(
        (
            v["merged"].as_u64(),
            v["total"].as_u64(),
            v["failed"].as_u64()
        ),
        (Some(1), Some(5), Some(1))
    );
    assert_eq!(
        (
            v["lost"].as_u64(),
            v["conflicts"].as_u64(),
            v["fix_rounds"].as_u64()
        ),
        (Some(1), Some(1), Some(1))
    );
    assert_eq!(
        v["wasted"],
        serde_json::json!({"lost_attempts": 1, "fix_rounds": 1, "conflict_rounds": 1, "extra_attempts": 0})
    );
    // M1 needed a fix round, so nothing merged on the first pass.
    assert_eq!(
        (v["first_pass"].as_u64(), v["first_pass_rate"].as_f64()),
        (Some(0), Some(0.0))
    );
    assert!(v["started"].as_i64().is_some() && v["finished"].as_i64().is_some());
    assert!(v["elapsed_s"].as_i64().unwrap() >= 0);
    let t1 = &v["tasks"][0];
    assert_eq!(
        (
            t1["id"].as_str(),
            t1["tries"].as_u64(),
            t1["planned_tier"].as_str(),
            t1["model"].as_str()
        ),
        (Some("M1"), Some(1), Some("medium"), Some("sonnet"))
    );
    assert!(t1["seconds"].as_i64().is_some());
    let models = v["models"].as_array().unwrap();
    let sonnet = models.iter().find(|m| m["model"] == "sonnet").unwrap();
    assert_eq!(
        (
            sonnet["tasks"].as_u64(),
            sonnet["merged"].as_u64(),
            sonnet["tries"].as_u64()
        ),
        (Some(2), Some(1), Some(2))
    );
    // planned tier versus actual model: M2 asked for high and ran on gpt-5.
    let m2 = &v["tasks"][1];
    assert_eq!(m2["planned_tier"], "high");
    assert_eq!(m2["model"], "gpt-5");
    assert!(m2.get("actual_tier").is_some() && m2.get("below_planned_tier").is_some());
}

#[tokio::test]
async fn report_of_a_run_with_nothing_started_has_no_rate() {
    let dir = tempfile::tempdir().unwrap();
    fresh_run(dir.path());
    let srv = start(dir.path()).await;
    let v = ok(&srv, "/api/v1/report").await;
    assert_eq!(
        (v["merged"].as_u64(), v["total"].as_u64()),
        (Some(0), Some(5))
    );
    assert!(v["first_pass_rate"].is_null() && v["elapsed_s"].is_null() && v["started"].is_null());
    assert_eq!(v["models"], serde_json::json!([]));
}

#[tokio::test]
async fn logs_tail_filter_and_clean() {
    let dir = tempfile::tempdir().unwrap();
    fresh_run(dir.path());
    write_files(dir.path());
    let srv = start(dir.path()).await;
    let all = ok(&srv, "/api/v1/logs").await;
    assert_eq!(all["lines"].as_array().unwrap().len(), 5);
    let last2 = ok(&srv, "/api/v1/logs?lines=2").await;
    assert_eq!(
        last2["lines"],
        serde_json::json!(["[10:00:04] M12 unrelated", "[10:00:05] M2 again"])
    );
    // `M2` does not match `M12`.
    let m2 = ok(&srv, "/api/v1/logs?milestone=M2").await;
    assert_eq!(
        m2["lines"],
        serde_json::json!(["[10:00:02] M2 held: risky", "[10:00:05] M2 again"])
    );
    let agent = ok(&srv, "/api/v1/logs?agent=codex-1").await;
    // Terminal escapes and control characters never reach the client.
    assert_eq!(
        agent["lines"],
        serde_json::json!(["[10:00:03] codex-1 said hello bell"])
    );
    for bad in ["lines=0", "lines=999999", "milestone="] {
        let (s, v) = get(&srv, &format!("/api/v1/logs?{bad}")).await;
        assert_eq!(
            (s, v["error"]["code"].as_str()),
            (400, Some("bad_request")),
            "{bad}"
        );
    }
}

#[tokio::test]
async fn a_run_without_a_log_has_empty_logs() {
    let dir = tempfile::tempdir().unwrap();
    fresh_run(dir.path());
    let srv = start(dir.path()).await;
    assert_eq!(
        ok(&srv, "/api/v1/logs").await["lines"],
        serde_json::json!([])
    );
}

#[tokio::test]
async fn agents_and_models_come_from_the_registries() {
    let dir = tempfile::tempdir().unwrap();
    fresh_run(dir.path());
    let srv = start(dir.path()).await;
    let a = ok(&srv, "/api/v1/agents").await;
    let kinds = a["agents"].as_array().unwrap();
    let claude = kinds
        .iter()
        .find(|k| k["kind"] == "claude")
        .expect("claude is in the registry");
    for key in [
        "installed",
        "roles",
        "herdr_detect",
        "verified",
        "launch",
        "skills",
        "login",
    ] {
        assert!(claude.get(key).is_some(), "{key}");
    }
    assert!(claude["skills"]["status"].is_string() && claude["login"]["state"].is_string());
    assert!(
        claude["roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r == "coder")
    );
    assert_eq!(a["team"].as_array().unwrap().len(), 3);
    assert_eq!(a["team"][2]["role"], "lead");

    let m = ok(&srv, "/api/v1/models").await;
    assert!(!m["models"].as_array().unwrap().is_empty());
    let tiers: Vec<&str> = m["tiers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(tiers, ["small", "medium", "high"]);
    assert!(m["policy"]["deny"].is_array());
    let first = &m["models"][0];
    for key in [
        "id",
        "tier",
        "kinds",
        "reasoning",
        "coding",
        "denied",
        "denied_by",
    ] {
        assert!(first.get(key).is_some(), "{key}");
    }
}

#[tokio::test]
async fn without_a_run_the_run_routes_say_no_run_and_nothing_is_created() {
    let dir = tempfile::tempdir().unwrap();
    let srv = start(dir.path()).await;
    for path in [
        "status", "plan", "tasks", "tasks/M1", "events", "report", "logs",
    ] {
        let (s, v) = get(&srv, &format!("/api/v1/{path}")).await;
        assert_eq!(
            (s, v["error"]["code"].as_str()),
            (404, Some("no_run")),
            "{path}"
        );
    }
    // The configuration routes still answer, with an empty team.
    assert_eq!(
        ok(&srv, "/api/v1/agents").await["team"],
        serde_json::json!([])
    );
    ok(&srv, "/api/v1/models").await;
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "a read created something"
    );
}

#[tokio::test]
async fn an_orchestrate_folder_without_a_stored_run_is_no_run_too() {
    let dir = tempfile::tempdir().unwrap();
    Store::open_in(&dir.path().join(".orchestrate")).unwrap();
    let srv = start(dir.path()).await;
    let (s, v) = get(&srv, "/api/v1/status").await;
    assert_eq!((s, v["error"]["code"].as_str()), (404, Some("no_run")));
}

#[tokio::test]
async fn every_read_endpoint_is_in_the_openapi_document_with_its_shape() {
    let dir = tempfile::tempdir().unwrap();
    let srv = start(dir.path()).await;
    let doc = ok(&srv, "/api/v1/openapi.json").await;
    let paths = doc["paths"].as_object().unwrap();
    let expect = [
        ("/api/v1/status", "StatusResponse"),
        ("/api/v1/plan", "PlanResponse"),
        ("/api/v1/tasks", "TasksResponse"),
        ("/api/v1/tasks/{id}", "TaskDetailResponse"),
        ("/api/v1/events", "EventsResponse"),
        ("/api/v1/report", "ReportResponse"),
        ("/api/v1/agents", "AgentsResponse"),
        ("/api/v1/models", "ModelsResponse"),
        ("/api/v1/logs", "LogsResponse"),
        ("/api/v1/attention", "AttentionResponse"),
        ("/api/v1/agents/{name}/output", "AgentOutputResponse"),
    ];
    for (path, schema) in expect {
        let ok = &paths[path]["get"]["responses"]["200"]["content"]["application/json"]["schema"]["$ref"];
        assert_eq!(ok, &format!("#/components/schemas/{schema}"), "{path}");
        assert!(
            doc["components"]["schemas"][schema].is_object(),
            "{schema} is not defined"
        );
        assert!(paths[path]["get"]["responses"]["401"].is_object());
    }
    let names = |p: &str| -> Vec<String> {
        paths[p]["get"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(names("/api/v1/events"), ["since", "limit"]);
    assert_eq!(names("/api/v1/logs"), ["lines", "milestone", "agent"]);
    assert_eq!(names("/api/v1/tasks/{id}"), ["id"]);
    assert_eq!(names("/api/v1/agents/{name}/output"), ["name", "lines"]);
    // Nested shapes are in the document too, so a client can generate types for them.
    for nested in [
        "PlanMilestone",
        "TaskInfo",
        "EventInfo",
        "ReviewInfo",
        "AgentKindInfo",
        "ModelInfo",
        "AttentionItem",
        "WorkingOn",
    ] {
        assert!(doc["components"]["schemas"][nested].is_object(), "{nested}");
    }
}

#[tokio::test]
async fn agent_output_endpoint_reads_bounded_and_strips_ansi() {
    let dir = tempfile::tempdir().unwrap();
    let _store = fresh_run(dir.path());
    let fake = Arc::new(FakeHerdr::new());
    fake.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
    fake.add_agent(AgentInfo {
        name: "claude-1".into(),
        kind: "claude".into(),
        pane_id: "w1:p1".into(),
        tab_id: "w1:t1".into(),
        workspace_id: "w1".into(),
        cwd: None,
        status: AgentStatus::Working,
        focused: false,
    });
    fake.set_output(
        "claude-1",
        "\x1b[32mPASS\x1b[0m test_foo (0.01s)\nAll 1 tests passed\n",
    );

    let srv = start_with_herdr(dir.path(), fake).await;

    // Default lines query
    let (status, body) = get(&srv, "/api/v1/agents/claude-1/output").await;
    assert_eq!(status, 200);
    assert_eq!(body["agent"], "claude-1");
    assert_eq!(body["lines"], 2);
    assert_eq!(body["text"], "PASS test_foo (0.01s)\nAll 1 tests passed\n");
    assert!(body["ts"].is_string());

    // Explicit lines query
    let (status, body) = get(&srv, "/api/v1/agents/claude-1/output?lines=50").await;
    assert_eq!(status, 200);
    assert_eq!(body["lines"], 2);

    // Bounds: 0 is rejected
    let (status, err) = get(&srv, "/api/v1/agents/claude-1/output?lines=0").await;
    assert_eq!(status, 400);
    assert_eq!(err["error"]["code"], "bad_request");

    // Bounds: > 5000 is rejected
    let (status, err) = get(&srv, "/api/v1/agents/claude-1/output?lines=5001").await;
    assert_eq!(status, 400);
    assert_eq!(err["error"]["code"], "bad_request");
}

#[tokio::test]
async fn agent_output_endpoint_errors_on_unknown_agent_and_herdr_failure() {
    let dir = tempfile::tempdir().unwrap();
    let _store = fresh_run(dir.path());
    let fake = Arc::new(FakeHerdr::new());
    fake.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
    fake.add_agent(AgentInfo {
        name: "claude-1".into(),
        kind: "claude".into(),
        pane_id: "w1:p1".into(),
        tab_id: "w1:t1".into(),
        workspace_id: "w1".into(),
        cwd: None,
        status: AgentStatus::Working,
        focused: false,
    });
    fake.set_output("claude-1", "output\n");

    let srv = start_with_herdr(dir.path(), fake.clone()).await;

    // Unknown agent: not on team in the run -> 404 agent_not_found
    let (status, err) = get(&srv, "/api/v1/agents/unknown-agent/output").await;
    assert_eq!(status, 404);
    assert_eq!(err["error"]["code"], "agent_not_found");

    // Herdr cannot read -> clear error (502 herdr_read_failed, not 200 or empty page)
    fake.fail_next(
        CallKind::AgentRead,
        1,
        HerdrError::other("herdr daemon timed out"),
    );
    let (status, err) = get(&srv, "/api/v1/agents/claude-1/output").await;
    assert_eq!(status, 502);
    assert_eq!(err["error"]["code"], "herdr_read_failed");
    assert!(err["error"]["message"].as_str().unwrap().contains("herdr daemon timed out"));
}

#[tokio::test]
async fn agents_endpoint_reports_working_on_per_team_member() {
    let dir = tempfile::tempdir().unwrap();
    let _store = busy_run(dir.path());
    let srv = start(dir.path()).await;

    let body = ok(&srv, "/api/v1/agents").await;
    let team = body["team"].as_array().unwrap();

    // claude-1 is coding M5 in busy_run
    let claude = team.iter().find(|m| m["name"] == "claude-1").unwrap();
    assert_eq!(
        claude["working_on"],
        serde_json::json!({
            "task": "M5",
            "state": "coding"
        })
    );

    // codex-1 is held on M2 in busy_run
    let codex = team.iter().find(|m| m["name"] == "codex-1").unwrap();
    assert_eq!(
        codex["working_on"],
        serde_json::json!({
            "task": "M2",
            "state": "held"
        })
    );

    // lead is not assigned a milestone
    let lead = team.iter().find(|m| m["name"] == "lead").unwrap();
    assert!(lead["working_on"].is_null());
}

#[tokio::test]
async fn attention_lists_all_open_kinds_and_they_disappear_when_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = fresh_run(dir.path());
    // herdr has no such agent: a blocked item carries no live prompt (see tests/prompt.rs)
    let srv = start_with_herdr(dir.path(), Arc::new(FakeHerdr::new())).await;

    // 1. Plan approval: fresh run has plan_approval item
    let body = ok(&srv, "/api/v1/attention").await;
    let items = body["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["kind"] == "plan_approval" && i["actions"] == serde_json::json!(["approve"])));

    // Resolve plan approval: record a task_go row via mark_dispatched
    store.mark_dispatched("M1").unwrap();
    let body = ok(&srv, "/api/v1/attention").await;
    let items = body["items"].as_array().unwrap();
    assert!(!items.iter().any(|i| i["kind"] == "plan_approval"));

    // 2. Blocked agent: milestone in Coding has a blocked lead queue item
    store.transition("M1", &TransitionRequest::auto(State::Coding, Some("start"))).unwrap();
    let _q_id = store.enqueue_blocked_lead_event(
        "M1",
        "claude-1",
        Some("May I run `cargo test`?"),
    ).unwrap().unwrap();
    let body = ok(&srv, "/api/v1/attention").await;
    let items = body["items"].as_array().unwrap();
    let blocked = items.iter().find(|i| i["kind"] == "blocked").unwrap();
    assert_eq!(blocked["agent"], "claude-1");
    assert!(blocked["prompt"].is_null(), "{blocked}");
    assert!(blocked["detail"].as_str().unwrap().contains("May I run `cargo test`?"), "{blocked}");
    assert_eq!(blocked["actions"], serde_json::json!(["open_chat"]));

    // Resolve blocked agent: milestone moves on (e.g. finishes coding into verifying)
    store.transition("M1", &TransitionRequest::auto(State::Verifying, Some("done"))).unwrap();
    let body = ok(&srv, "/api/v1/attention").await;
    let items = body["items"].as_array().unwrap();
    assert!(!items.iter().any(|i| i["kind"] == "blocked"));

    // 3. Question: milestone M2 in needs-answer
    store.transition("M2", &TransitionRequest::auto(State::NeedsAnswer, Some("which port?"))).unwrap();
    let body = ok(&srv, "/api/v1/attention").await;
    let items = body["items"].as_array().unwrap();
    let question = items.iter().find(|i| i["kind"] == "question").unwrap();
    assert_eq!(question["task"], "M2");
    assert_eq!(question["actions"], serde_json::json!(["answer"]));

    // Resolve question: transition back to pending (answered/dispatched)
    store.transition("M2", &TransitionRequest::auto(State::Pending, Some("answered"))).unwrap();
    let body = ok(&srv, "/api/v1/attention").await;
    let items = body["items"].as_array().unwrap();
    assert!(!items.iter().any(|i| i["kind"] == "question"));

    // 4. Held milestone: milestone M3 in held (via Coding -> Verifying -> Held)
    store.transition("M3", &TransitionRequest::auto(State::Coding, Some("start"))).unwrap();
    store.transition("M3", &TransitionRequest::auto(State::Verifying, Some("check"))).unwrap();
    store.transition("M3", &TransitionRequest::auto(State::Held, Some("high risk diff"))).unwrap();
    let body = ok(&srv, "/api/v1/attention").await;
    let items = body["items"].as_array().unwrap();
    let held = items.iter().find(|i| i["kind"] == "held").unwrap();
    assert_eq!(held["task"], "M3");
    assert_eq!(held["detail"], "high risk diff");
    assert_eq!(held["actions"], serde_json::json!(["approve"]));

    // Resolve held: approve/resume (e.g. to pending for commit/dispatch)
    store.transition("M3", &TransitionRequest::auto(State::Pending, Some("approved"))).unwrap();
    let body = ok(&srv, "/api/v1/attention").await;
    let items = body["items"].as_array().unwrap();
    assert!(!items.iter().any(|i| i["kind"] == "held"));

    // 5. Failed milestone: milestone M4 in failed
    store.transition("M4", &TransitionRequest::auto(State::Coding, Some("start"))).unwrap();
    store.transition("M4", &TransitionRequest::auto(State::Failed, Some("check failed 3 times"))).unwrap();
    let body = ok(&srv, "/api/v1/attention").await;
    let items = body["items"].as_array().unwrap();
    let failed = items.iter().find(|i| i["kind"] == "failed").unwrap();
    assert_eq!(failed["task"], "M4");
    assert_eq!(failed["actions"], serde_json::json!(["dispatch"]));

    // Resolve failed: dispatch back to coding
    store.transition("M4", &TransitionRequest::auto(State::Pending, Some("retried"))).unwrap();
    let body = ok(&srv, "/api/v1/attention").await;
    let items = body["items"].as_array().unwrap();
    assert!(!items.iter().any(|i| i["kind"] == "failed"));

    // 6. Wake unread: unacknowledged wake-up
    store.transition("M5", &TransitionRequest::auto(State::Coding, Some("start"))).unwrap();
    let wake_qid = store.enqueue_blocked_lead_event("M5", "claude-1", None).unwrap().unwrap();
    store.save_wake_state(&orch_core::wake::WakeState {
        upto: wake_qid,
        since: 1_700_000_000,
        round_at: 1_700_000_000,
        sent_at: 1_700_000_000,
        sends: 1,
        confirm_n: 0,
        awaiting: true,
        notified: false,
        held: false,
        gone_told: false,
    }).unwrap();
    let body = ok(&srv, "/api/v1/attention").await;
    let items = body["items"].as_array().unwrap();
    let wake = items.iter().find(|i| i["kind"] == "wake_unread").unwrap();
    assert_eq!(wake["actions"], serde_json::json!(["open_chat"]));

    // Resolve wake unread: mark event read
    store.mark_lead_read(&[wake_qid]).unwrap();
    let body = ok(&srv, "/api/v1/attention").await;
    let items = body["items"].as_array().unwrap();
    assert!(!items.iter().any(|i| i["kind"] == "wake_unread"));
}
