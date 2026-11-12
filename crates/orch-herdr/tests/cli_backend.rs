//! The CLI backend run against the `fake-herdr` executable through real
//! subprocess calls: every trait method, every error kind, the timeout.
//! Nothing here touches a real herdr or the developer's `HERDR_*` environment.

use orch_herdr::cli::CliHerdr;
use orch_herdr::{AgentStatus, HerdrError, RuntimeAdapter, SplitPane};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-herdr");

/// A scratch state directory for one fake herdr, removed on drop.
struct State(PathBuf);

impl State {
    fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("fake-herdr-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir_all(&dir).unwrap();
        State(dir)
    }

    fn put(&self, rel: &str, content: &str) {
        let p = self.0.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    fn agent(&self, name: &str, status: &str, pane: &str) {
        let v = json!({
            "agent": "claude", "agent_status": status, "cwd": "/repo", "focused": false,
            "name": name, "pane_id": pane, "tab_id": "w1:t1", "workspace_id": "w1",
            "completion_seq": 3, "revision": 1
        });
        self.put(&format!("agents/{name}.json"), &v.to_string());
        self.pane(pane);
    }

    fn pane(&self, id: &str) {
        let v = json!({
            "pane_id": id, "workspace_id": "w1", "tab_id": "w1:t1", "cwd": "/repo",
            "agent_status": "unknown", "focused": false, "revision": 1
        });
        self.put(&format!("panes/{id}.json"), &v.to_string());
    }

    fn sim(&self, v: Value) {
        self.put("sim.json", &v.to_string());
    }

    fn lines(&self, file: &str) -> Vec<Value> {
        fs::read_to_string(self.0.join(file))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn sent(&self) -> Vec<Value> {
        self.lines("sent.jsonl")
    }

    fn herdr(&self) -> CliHerdr {
        CliHerdr::new(FAKE, 10)
            .without_inherited_herdr_env()
            .with_env("FAKE_HERDR_DIR", self.0.to_str().unwrap())
    }
}

impl Drop for State {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[test]
fn agents_lists_every_agent_with_typed_fields() {
    let s = State::new();
    s.agent("lead", "idle", "w1:p1");
    s.agent("claude-1", "working", "w1:p2");
    let agents = s.herdr().agents().unwrap();
    assert_eq!(agents.len(), 2);
    assert_eq!(agents[0].name, "claude-1");
    assert_eq!(agents[0].status, AgentStatus::Working);
    assert_eq!(agents[0].kind, "claude");
    assert_eq!(agents[1].pane_id, "w1:p1");
    assert_eq!(s.lines("calls.jsonl")[0]["args"], json!(["agent", "list"]));
}

#[test]
fn agent_get_reads_only_the_status_field() {
    let s = State::new();
    // The name and the cwd both say "working" and "blocked"; the status is idle.
    s.put(
        "agents/working-blocked.json",
        &json!({
            "agent": "claude", "agent_status": "idle", "cwd": "/home/blocked/working",
            "name": "working-blocked", "pane_id": "w1:p1", "tab_id": "w1:t1", "workspace_id": "w1",
            "terminal_title": "working blocked"
        })
        .to_string(),
    );
    let h = s.herdr();
    assert_eq!(h.agent("working-blocked").unwrap().status, AgentStatus::Idle);
    assert_eq!(h.agent_status("working-blocked").unwrap(), AgentStatus::Idle);
    // Addressing by pane id works too.
    assert_eq!(h.agent("w1:p1").unwrap().name, "working-blocked");
}

#[test]
fn an_unnamed_agent_is_known_by_its_pane_id() {
    let s = State::new();
    s.put(
        "agents/x.json",
        &json!({"agent": "codex", "agent_status": "done", "pane_id": "w1:p4", "tab_id": "w1:t1", "workspace_id": "w1"}).to_string(),
    );
    assert_eq!(s.herdr().agents().unwrap()[0].name, "w1:p4");
}

#[test]
fn a_missing_agent_is_agent_not_found_and_status_gone() {
    let s = State::new();
    let h = s.herdr();
    match h.agent("nobody") {
        Err(HerdrError::AgentNotFound { agent, message }) => {
            assert_eq!(agent, "nobody");
            assert!(message.contains("not found"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(h.agent_status("nobody").unwrap(), AgentStatus::Gone);
    assert!(matches!(h.read_output("nobody", 5), Err(HerdrError::AgentNotFound { .. })));
    assert!(matches!(h.send_prompt("nobody", "hi"), Err(HerdrError::AgentNotFound { .. })));
}

#[test]
fn send_prompt_delivers_and_is_recorded() {
    let s = State::new();
    s.agent("claude-1", "idle", "w1:p2");
    s.herdr().send_prompt("claude-1", "do the thing --now").unwrap();
    assert_eq!(s.sent(), vec![json!({"kind": "prompt", "target": "claude-1", "text": "do the thing --now"})]);
    assert_eq!(s.lines("calls.jsonl")[0]["args"], json!(["agent", "prompt", "claude-1", "do the thing --now"]));
}

#[test]
fn a_blocked_agent_refuses_the_prompt() {
    let s = State::new();
    s.agent("claude-1", "blocked", "w1:p2");
    match s.herdr().send_prompt("claude-1", "hi") {
        Err(HerdrError::AgentBlocked { agent, .. }) => assert_eq!(agent, "claude-1"),
        other => panic!("{other:?}"),
    }
    assert!(s.sent().is_empty(), "a refused prompt must not be recorded as delivered");
}

#[test]
fn read_output_asks_for_recent_unwrapped_lines() {
    let s = State::new();
    s.agent("claude-1", "idle", "w1:p2");
    s.put("read/claude-1.txt", "one\ntwo\nthree\nfour\n");
    let out = s.herdr().read_output("claude-1", 2).unwrap();
    assert_eq!(out, "three\nfour\n");
    assert_eq!(
        s.lines("calls.jsonl")[0]["args"],
        json!(["agent", "read", "claude-1", "--source", "recent-unwrapped", "--lines", "2"])
    );
}

#[test]
fn rename_agent_and_pane() {
    let s = State::new();
    s.agent("claude-1", "idle", "w1:p2");
    let h = s.herdr();
    h.rename_agent("w1:p2", "coder").unwrap();
    assert_eq!(h.agent("coder").unwrap().pane_id, "w1:p2");
    assert!(matches!(h.agent("claude-1"), Err(HerdrError::AgentNotFound { .. })));
    h.rename_pane("w1:p2", "the coder").unwrap();
    assert_eq!(h.pane("w1:p2").unwrap().label.as_deref(), Some("the coder"));
    assert!(matches!(h.rename_agent("ghost", "x"), Err(HerdrError::AgentNotFound { .. })));
}

#[test]
fn pane_get_list_and_workspace_filter() {
    let s = State::new();
    s.pane("w1:p1");
    s.pane("w1:p2");
    s.put(
        "panes/w2:p1.json",
        &json!({"pane_id": "w2:p1", "workspace_id": "w2", "tab_id": "w2:t1", "agent_status": "unknown"}).to_string(),
    );
    let h = s.herdr();
    assert_eq!(h.pane("w1:p2").unwrap().tab_id, "w1:t1");
    assert_eq!(h.panes(None).unwrap().len(), 3);
    let w2 = h.panes(Some("w2")).unwrap();
    assert_eq!(w2.len(), 1);
    assert_eq!(w2[0].pane_id, "w2:p1");
}

#[test]
fn an_unknown_pane_id_is_a_stale_pane_for_every_pane_call() {
    let s = State::new();
    let h = s.herdr();
    let stale = |r: Result<(), HerdrError>| match r {
        Err(HerdrError::StalePane { pane, .. }) => assert_eq!(pane, "w9:p99"),
        other => panic!("{other:?}"),
    };
    assert!(matches!(h.pane("w9:p99"), Err(HerdrError::StalePane { .. })));
    stale(h.rename_pane("w9:p99", "x"));
    stale(h.run_command("w9:p99", "ls"));
    stale(h.send_text("w9:p99", "x"));
    stale(h.send_keys("w9:p99", &["enter"]));
    stale(h.split_pane(&SplitPane::right("w9:p99", Path::new("/repo"))).map(|_| ()));
}

#[test]
fn current_pane_and_workspaces() {
    let s = State::new();
    s.pane("w1:p1");
    s.put("current_pane", "w1:p1\n");
    s.put(
        "workspaces.json",
        &json!([{"workspace_id": "w1", "number": 1, "label": "orchestra", "focused": true, "pane_count": 1, "tab_count": 1, "active_tab_id": "w1:t1", "agent_status": "idle"}])
            .to_string(),
    );
    let h = s.herdr();
    assert_eq!(h.current_pane().unwrap().pane_id, "w1:p1");
    let ws = h.workspaces().unwrap();
    assert_eq!(ws[0].label, "orchestra");
    assert_eq!(ws[0].pane_count, 1);
    assert_eq!(s.lines("calls.jsonl")[0]["args"], json!(["pane", "current", "--current"]));

    fs::remove_file(s.0.join("current_pane")).unwrap();
    assert!(matches!(h.current_pane(), Err(HerdrError::StalePane { .. })));
}

#[test]
fn split_pane_passes_cwd_and_returns_the_new_pane() {
    let s = State::new();
    s.pane("w1:p1");
    let p = s.herdr().split_pane(&SplitPane::right("w1:p1", Path::new("/work/tree"))).unwrap();
    assert_eq!(p.pane_id, "w1:p100");
    assert_eq!(p.cwd.as_deref(), Some("/work/tree"));
    assert_eq!(
        s.lines("calls.jsonl")[0]["args"],
        json!(["pane", "split", "w1:p1", "--direction", "right", "--cwd", "/work/tree", "--no-focus"])
    );
    assert_eq!(s.herdr().pane("w1:p100").unwrap().workspace_id, "w1");
}

#[test]
fn run_text_and_keys_are_sent_and_recorded_in_order() {
    let s = State::new();
    s.pane("w1:p1");
    let h = s.herdr();
    h.run_command("w1:p1", "claude --model x").unwrap();
    h.send_text("w1:p1", "hello world").unwrap();
    h.send_keys("w1:p1", &["ctrl-c", "enter"]).unwrap();
    assert_eq!(
        s.sent(),
        vec![
            json!({"kind": "run", "target": "w1:p1", "text": "claude --model x"}),
            json!({"kind": "text", "target": "w1:p1", "text": "hello world"}),
            json!({"kind": "keys", "target": "w1:p1", "keys": ["ctrl-c", "enter"]}),
        ]
    );
}

#[test]
fn an_at_pane_agent_gets_the_text_then_enter() {
    let s = State::new();
    s.pane("w1:p7");
    s.herdr().send_prompt("@pane:w1:p7", "go on").unwrap();
    assert_eq!(
        s.sent(),
        vec![
            json!({"kind": "text", "target": "w1:p7", "text": "go on"}),
            json!({"kind": "keys", "target": "w1:p7", "keys": ["enter"]}),
        ]
    );
    assert!(s.lines("calls.jsonl").iter().all(|c| c["args"][0] != "agent"), "no `agent prompt` for a bare pane");
}

#[test]
fn a_failed_at_pane_delivery_is_a_stale_pane_naming_the_pane() {
    let s = State::new();
    match s.herdr().send_prompt("@pane:w1:p9", "go on") {
        Err(HerdrError::StalePane { pane, message }) => {
            assert_eq!(pane, "w1:p9");
            assert!(message.contains("w1:p9"), "{message}");
            assert!(message.contains("herdr pane list"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn availability_reflects_herdr_env_and_reachability() {
    let s = State::new();
    let h = s.herdr();
    let a = h.availability().unwrap();
    assert!(!a.in_pane, "the developer's HERDR_ENV must not leak in");
    assert!(a.reachable);
    let a = s.herdr().with_env("HERDR_ENV", "1").availability().unwrap();
    assert!(a.in_pane);

    s.sim(json!({"fail_exit": 1, "fail_message": "no server"}));
    assert!(!h.availability().unwrap().reachable);
}

#[test]
fn a_missing_binary_is_unavailable() {
    let h = CliHerdr::new("/nonexistent/herdr-binary", 5);
    assert!(matches!(h.agents(), Err(HerdrError::Unavailable { .. })));
    assert!(matches!(h.send_prompt("a", "b"), Err(HerdrError::Unavailable { .. })));
    let a = h.availability().unwrap();
    assert!(!a.reachable);
    // A missing herdr is not a stale pane, even for an @pane delivery.
    assert!(matches!(h.send_prompt("@pane:w1:p1", "b"), Err(HerdrError::Unavailable { .. })));
}

#[test]
fn a_non_zero_exit_keeps_herdrs_message() {
    let s = State::new();
    s.sim(json!({"fail_exit": 3, "fail_message": "socket exploded"}));
    match s.herdr().agents() {
        Err(HerdrError::Other { message }) => {
            assert!(message.contains("socket exploded"), "{message}");
            assert!(message.contains("exit 3"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_failure_can_be_limited_to_one_command() {
    let s = State::new();
    s.agent("claude-1", "idle", "w1:p2");
    s.sim(json!({"fail_exit": 1, "fail_cmd": "agent prompt", "fail_message": "nope"}));
    let h = s.herdr();
    assert!(h.agent("claude-1").is_ok());
    assert!(matches!(h.send_prompt("claude-1", "x"), Err(HerdrError::Other { .. })));
}

#[test]
fn a_slow_herdr_is_a_timeout_and_leaves_no_child_behind() {
    let s = State::new();
    s.sim(json!({"slow_ms": 20000}));
    let h = CliHerdr::new(FAKE, 1).without_inherited_herdr_env().with_env("FAKE_HERDR_DIR", s.0.to_str().unwrap());
    let started = std::time::Instant::now();
    match h.agents() {
        Err(HerdrError::TimedOut { timeout_s, message }) => {
            assert_eq!(timeout_s, 1);
            assert!(message.contains("agent list"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed().as_secs() < 10, "the call must stop at the timeout");
    let pids: Vec<u32> = fs::read_to_string(s.0.join("pids.log")).unwrap().lines().map(|l| l.parse().unwrap()).collect();
    assert_eq!(pids.len(), 1);
    assert!(!pid_alive(pids[0]), "the timed-out herdr process must be gone");
}

#[test]
fn the_backend_comes_from_the_tunables() {
    let t = orch_config::tunables::Tunables::from_lookup(|k| match k {
        "HERDR" => Some(FAKE.to_string()),
        "ORCH_HERDR_TIMEOUT_S" => Some("7".to_string()),
        _ => None,
    })
    .unwrap();
    let s = State::new();
    let h = CliHerdr::from_tunables(&t).without_inherited_herdr_env().with_env("FAKE_HERDR_DIR", s.0.to_str().unwrap());
    assert_eq!(h.timeout_s(), 7);
    assert!(h.agents().unwrap().is_empty());
    assert_eq!(s.lines("calls.jsonl").len(), 1, "the HERDR binary was the one run");
}
