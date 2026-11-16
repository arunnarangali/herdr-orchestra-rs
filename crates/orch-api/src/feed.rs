//! Live event stream feed: polls the run's database (events, lead_queue),
//! events.jsonl, and autopilot.log, keeping a bounded event history for reconnecting clients.

use crate::error::ApiError;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use utoipa::ToSchema;

/// A live stream event pushed via Server-Sent Events.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamMessage {
    /// Monotonically increasing event ID.
    pub id: u64,
    /// Event name: `task-state`, `lead-event`, `run-state`, `chat`, or `log`.
    pub event: String,
    /// Event data payload (JSON object).
    pub data: serde_json::Value,
}

/// Payload for `task-state` events (milestone state changes).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskStateEvent {
    pub id: String,
    pub task: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub from: String,
    pub to: String,
    pub status: String,
    pub why: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub attempt: u32,
    pub forced: bool,
    pub ts: i64,
}

/// Payload for `lead-event` events (lead queue entries).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LeadQueueEvent {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub text: String,
    /// The agent the event is about (a blocked permission event names its agent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// What the agent's pane asks for (the visible prompt text, when read).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<i64>,
}

/// Payload for `run-state` events (run events, status changes, or re-fetch notifications).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RunStateEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_control: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
}

/// Payload for `log` events (autopilot.log lines).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LogEvent {
    pub message: String,
    pub line: String,
    pub ts: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttentionOp {
    Add,
    Remove,
}

/// Payload for `attention` events.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AttentionEvent {
    pub op: AttentionOp,
    pub item: crate::models::AttentionItem,
}

/// Polling interval for disk checks.
pub fn poll_interval() -> Duration {
    let ms = std::env::var("ORCH_STREAM_POLL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(200);
    Duration::from_millis(ms)
}

/// Interval for keep-alive SSE comments on idle connections.
pub fn keepalive_interval() -> Duration {
    let ms = std::env::var("ORCH_STREAM_KEEPALIVE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(15_000);
    Duration::from_millis(ms)
}

/// Maximum concurrent clients per project stream.
pub fn max_clients() -> usize {
    std::env::var("ORCH_STREAM_MAX_CLIENTS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(16)
}

/// Capacity of the retained message ring buffer.
pub fn buffer_capacity() -> usize {
    std::env::var("ORCH_STREAM_BUFFER_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(500)
}

/// The hub coordinating event distribution for a project.
pub struct StreamHub {
    pub project_root: PathBuf,
    pub next_id: AtomicU64,
    pub active_clients: AtomicUsize,
    pub max_clients: usize,
    pub buffer_capacity: usize,
    history: RwLock<VecDeque<StreamMessage>>,
    min_kept_id: AtomicU64,
    has_dropped: AtomicBool,
    subscribers: Mutex<HashMap<u64, mpsc::Sender<StreamMessage>>>,
    next_sub_id: AtomicU64,
}

static HUBS: Mutex<Option<HashMap<PathBuf, Arc<StreamHub>>>> = Mutex::new(None);

impl StreamHub {
    pub fn new(project_root: PathBuf) -> StreamHub {
        StreamHub {
            project_root,
            next_id: AtomicU64::new(1),
            active_clients: AtomicUsize::new(0),
            max_clients: max_clients(),
            buffer_capacity: buffer_capacity(),
            history: RwLock::new(VecDeque::new()),
            min_kept_id: AtomicU64::new(1),
            has_dropped: AtomicBool::new(false),
            subscribers: Mutex::new(HashMap::new()),
            next_sub_id: AtomicU64::new(1),
        }
    }

    pub fn get_or_create(project_root: &Path, shutdown: watch::Receiver<bool>) -> Arc<StreamHub> {
        let mut guard = HUBS.lock().unwrap();
        let map = guard.get_or_insert_with(HashMap::new);
        if let Some(hub) = map.get(project_root) {
            hub.clone()
        } else {
            let hub = Arc::new(StreamHub::new(project_root.to_path_buf()));
            start_poller(hub.clone(), shutdown);
            map.insert(project_root.to_path_buf(), hub.clone());
            hub
        }
    }

    /// Reset all hubs (used by unit tests if needed).
    pub fn reset_all() {
        let mut guard = HUBS.lock().unwrap();
        if let Some(map) = guard.as_mut() {
            for (_, hub) in map.drain() {
                hub.close_all();
            }
        }
    }

    /// Publish a message to all connected clients and store in history.
    pub fn publish(&self, event: &str, data: serde_json::Value) -> StreamMessage {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = StreamMessage {
            id,
            event: event.to_string(),
            data,
        };

        // 1. Store in bounded history
        {
            let mut history = self.history.write().unwrap();
            history.push_back(msg.clone());
            while history.len() > self.buffer_capacity {
                let evicted = history.pop_front().unwrap();
                self.has_dropped.store(true, Ordering::SeqCst);
                let next_min = history.front().map(|m| m.id).unwrap_or(evicted.id + 1);
                self.min_kept_id.store(next_min, Ordering::SeqCst);
            }
        }

        // 2. Broadcast to all active subscribers without blocking
        {
            let mut subs = self.subscribers.lock().unwrap();
            let mut dead = Vec::new();
            for (&sub_id, tx) in subs.iter() {
                match tx.try_send(msg.clone()) {
                    Ok(_) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        tracing::warn!(sub_id, "slow client buffer full; dropping live message");
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        dead.push(sub_id);
                    }
                }
            }
            for sub_id in dead {
                subs.remove(&sub_id);
            }
        }

        msg
    }

    /// Subscribe to the live stream with optional replay starting after `since`.
    pub fn subscribe(
        self: &Arc<Self>,
        since: Option<u64>,
    ) -> Result<(mpsc::Receiver<StreamMessage>, ClientGuard), ApiError> {
        let current = self.active_clients.fetch_add(1, Ordering::SeqCst);
        if current >= self.max_clients {
            self.active_clients.fetch_sub(1, Ordering::SeqCst);
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_clients",
                "maximum number of live stream clients reached",
            ));
        }

        let sub_id = self.next_sub_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel(128);

        let mut initial_messages = Vec::new();
        if let Some(since_id) = since {
            let history = self.history.read().unwrap();
            let min_kept = self.min_kept_id.load(Ordering::SeqCst);
            let has_dropped = self.has_dropped.load(Ordering::SeqCst);

            if has_dropped && since_id < min_kept {
                // Client asked for an id older than what is kept: tell it to re-fetch status
                let refetch = StreamMessage {
                    id: self.next_id.load(Ordering::SeqCst),
                    event: "run-state".to_string(),
                    data: serde_json::json!({
                        "action": "refetch_status",
                        "message": "re-fetch status",
                        "status": "refetch_status",
                        "detail": "event id older than retained buffer: re-fetch status"
                    }),
                };
                initial_messages.push(refetch);
            } else {
                for msg in history.iter() {
                    if msg.id > since_id {
                        initial_messages.push(msg.clone());
                    }
                }
            }
        }

        // Register subscriber under lock and deliver initial backlog
        {
            let mut subs = self.subscribers.lock().unwrap();
            subs.insert(sub_id, tx.clone());
        }

        for msg in initial_messages {
            let _ = tx.try_send(msg);
        }

        let guard = ClientGuard {
            hub: self.clone(),
            sub_id,
        };

        Ok((rx, guard))
    }

    pub fn close_all(&self) {
        let mut subs = self.subscribers.lock().unwrap();
        subs.clear();
    }
}

/// RAII guard ensuring active_clients and subscriber registration are cleaned up on disconnect.
pub struct ClientGuard {
    hub: Arc<StreamHub>,
    sub_id: u64,
}

impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.hub.active_clients.fetch_sub(1, Ordering::SeqCst);
        let mut subs = self.hub.subscribers.lock().unwrap();
        subs.remove(&self.sub_id);
    }
}

/// Stream wrapper pairing `ClientGuard` and shutdown monitoring with Axum SSE.
pub struct GuardedStream<S> {
    stream: S,
    _guard: ClientGuard,
    shutdown: watch::Receiver<bool>,
}

impl<S> GuardedStream<S> {
    pub fn new(stream: S, guard: ClientGuard, shutdown: watch::Receiver<bool>) -> Self {
        GuardedStream {
            stream,
            _guard: guard,
            shutdown,
        }
    }
}

impl<S: futures_util::Stream + Unpin> futures_util::Stream for GuardedStream<S> {
    type Item = S::Item;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if *self.shutdown.borrow() {
            return std::task::Poll::Ready(None);
        }
        std::pin::Pin::new(&mut self.stream).poll_next(cx)
    }
}

#[derive(Default)]
struct PollState {
    last_event_id: i64,
    last_lead_queue_id: i64,
    last_action_id: i64,
    last_chat_seq: i64,
    last_run_control_state: Option<String>,
    last_run_control_source: Option<String>,
    last_attention_items: Option<Vec<orch_core::attention::AttentionItem>>,
    events_jsonl_offset: u64,
    log_offset: u64,
}

impl PollState {
    fn init(root: &Path) -> Self {
        let orch_dir = root.join(".orchestrate");
        let db_path = orch_dir.join("state.db");
        let events_jsonl_path = orch_dir.join("events.jsonl");
        let autopilot_log_path = orch_dir.join("autopilot.log");

        let (last_event_id, last_lead_queue_id, last_action_id, initial_control) =
            orch_core::events::init_db_poll_state_all(&db_path);

        let (last_run_control_state, last_run_control_source) = match initial_control {
            Some(rc) => (Some(rc.state), Some(rc.source)),
            None => (Some("running".to_string()), None),
        };

        let last_attention_items = if db_path.exists() {
            if let Ok(store) = orch_core::store::Store::open(&db_path) {
                if let Ok(Some(run)) = store.load_run() {
                    orch_core::attention::attention_items(&store, Some(&run)).ok()
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let events_jsonl_offset = if events_jsonl_path.exists() {
            std::fs::metadata(&events_jsonl_path)
                .map(|m| m.len())
                .unwrap_or(0)
        } else {
            0
        };

        let log_offset = if autopilot_log_path.exists() {
            std::fs::metadata(&autopilot_log_path)
                .map(|m| m.len())
                .unwrap_or(0)
        } else {
            0
        };

        PollState {
            last_event_id,
            last_lead_queue_id,
            last_action_id,
            last_chat_seq: orch_core::chat::init_chat_poll_state(&db_path),
            last_run_control_state,
            last_run_control_source,
            last_attention_items: Some(last_attention_items.unwrap_or_default()),
            events_jsonl_offset,
            log_offset,
        }
    }
}

/// Launch the background disk poller for the project.
pub fn start_poller(hub: Arc<StreamHub>, mut shutdown: watch::Receiver<bool>) {
    let mut state = PollState::init(&hub.project_root);
    tokio::spawn(async move {
        let root = hub.project_root.clone();
        let orch_dir = root.join(".orchestrate");
        let db_path = orch_dir.join("state.db");
        let events_jsonl_path = orch_dir.join("events.jsonl");
        let autopilot_log_path = orch_dir.join("autopilot.log");

        loop {
            if *shutdown.borrow() {
                break;
            }

            let hub_clone = hub.clone();
            let db_p = db_path.clone();
            let ev_p = events_jsonl_path.clone();
            let log_p = autopilot_log_path.clone();

            let (poll_res, updated_state) = tokio::task::spawn_blocking(move || {
                poll_once(&hub_clone, &db_p, &ev_p, &log_p, &mut state);
                state
            })
            .await
            .map(|s| (true, s))
            .unwrap_or_else(|_| (false, PollState::default()));

            if !poll_res {
                break;
            }
            state = updated_state;

            tokio::select! {
                _ = tokio::time::sleep(poll_interval()) => {}
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        break;
                    }
                }
            }
        }

        hub.close_all();
    });
}

fn poll_once(
    hub: &StreamHub,
    db_path: &Path,
    events_jsonl_path: &Path,
    log_path: &Path,
    state: &mut PollState,
) {
    // 1. Read SQLite database via orch-core (read-only mode so engine is untouched and stream never writes)
    for row in orch_core::events::poll_db_events(db_path, &mut state.last_event_id) {
        let ev = TaskStateEvent {
            id: row.task.clone(),
            task: row.task.clone(),
            task_id: Some(row.task.clone()),
            from: row.from,
            to: row.to.clone(),
            status: row.to,
            why: row.why.clone(),
            reason: Some(row.why),
            worker: row.agent.clone(),
            agent: row.agent,
            model: row.model,
            attempt: row.attempt,
            forced: row.forced,
            ts: row.ts,
        };
        hub.publish("task-state", serde_json::to_value(ev).unwrap_or_default());
    }

    for row in orch_core::events::poll_lead_queue_events(db_path, &mut state.last_lead_queue_id) {
        let ev = LeadQueueEvent {
            id: row.id,
            ts: row.ts,
            kind: row.kind,
            task: row.task.clone(),
            task_id: row.task,
            text: row.text,
            agent: row.agent,
            prompt: row.prompt,
            read_at: row.read_at,
        };
        hub.publish("lead-event", serde_json::to_value(ev).unwrap_or_default());
    }

    // 1b. The chat: every stored message and every change of its delivery state
    for m in orch_core::chat::poll_chat_changes(db_path, &mut state.last_chat_seq) {
        let view = crate::routes::chat::ChatMessageView::from(m);
        hub.publish("chat", serde_json::to_value(view).unwrap_or_default());
    }

    // 1c. Read run actions (audit trail) and run_control changes
    let actions = orch_core::events::poll_run_actions(db_path, &mut state.last_action_id);
    for act in actions {
        if matches!(act.action.as_str(), "pause" | "resume" | "stop") {
            let control_state = match act.action.as_str() {
                "pause" => "paused".to_string(),
                "resume" => "running".to_string(),
                "stop" => {
                    if let Some(rc) = orch_core::events::get_run_control(db_path)
                        && (rc.state == "stopping" || rc.state == "stopped")
                    {
                        rc.state
                    } else {
                        "stopping".to_string()
                    }
                }
                _ => "running".to_string(),
            };
            let changed = state.last_run_control_state.as_deref() != Some(&control_state)
                || state.last_run_control_source.as_deref() != Some(&act.source);
            if changed {
                let ev = RunStateEvent {
                    action: Some(act.action.clone()),
                    message: Some(format!("run control: {control_state} ({})", act.source)),
                    event: Some("run-control".to_string()),
                    task: None,
                    agent: None,
                    status: Some(control_state.clone()),
                    run_control: Some(control_state.clone()),
                    state: Some(control_state.clone()),
                    source: Some(act.source.clone()),
                    ts: Some(act.ts),
                };
                hub.publish("run-state", serde_json::to_value(ev).unwrap_or_default());
                state.last_run_control_state = Some(control_state);
                state.last_run_control_source = Some(act.source);
            }
        }
    }

    // Also check run_control table directly (e.g. manager changes stopping -> stopped, or direct store updates)
    if let Some(rc) = orch_core::events::get_run_control(db_path) {
        let changed = match (&state.last_run_control_state, &state.last_run_control_source) {
            (Some(last_state), Some(last_source)) => {
                last_state != &rc.state || last_source != &rc.source
            }
            (Some(last_state), None) => {
                last_state != &rc.state
            }
            _ => true,
        };
        if changed {
            let action = match rc.state.as_str() {
                "paused" => "pause",
                "running" => "resume",
                "stopping" | "stopped" => "stop",
                _ => rc.state.as_str(),
            };
            let ev = RunStateEvent {
                action: Some(action.to_string()),
                message: Some(format!("run control: {} ({})", rc.state, rc.source)),
                event: Some("run-control".to_string()),
                task: None,
                agent: None,
                status: Some(rc.state.clone()),
                run_control: Some(rc.state.clone()),
                state: Some(rc.state.clone()),
                source: Some(rc.source.clone()),
                ts: Some(rc.since),
            };
            hub.publish("run-state", serde_json::to_value(ev).unwrap_or_default());
            state.last_run_control_state = Some(rc.state);
            state.last_run_control_source = Some(rc.source);
        }
    }

    // 2. Read events.jsonl
    if let Ok(mut file) = std::fs::File::open(events_jsonl_path)
        && let Ok(meta) = file.metadata()
    {
        let len = meta.len();
        if len > state.events_jsonl_offset {
            use std::io::{Read, Seek, SeekFrom};
            if file.seek(SeekFrom::Start(state.events_jsonl_offset)).is_ok() {
                let to_read = (len - state.events_jsonl_offset) as usize;
                let mut buf = vec![0u8; to_read];
                if let Ok(n) = file.read(&mut buf) {
                    buf.truncate(n);
                    let s = String::from_utf8_lossy(&buf);
                    let has_trailing_newline = buf.last() == Some(&b'\n');
                    let lines: Vec<&str> = s.lines().collect();
                    let count = lines.len();
                    for (idx, line) in lines.into_iter().enumerate() {
                        if idx == count - 1 && !has_trailing_newline {
                            break;
                        }
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                                hub.publish("run-state", val);
                            } else if let Some(ev) = orch_core::events::RunEvent::parse(trimmed) {
                                let val = serde_json::json!({
                                    "event": ev.key(),
                                    "line": ev.line(),
                                    "task": ev.task(),
                                    "agent": ev.agent(),
                                });
                                hub.publish("run-state", val);
                            } else {
                                let val = serde_json::json!({
                                    "raw": trimmed,
                                });
                                hub.publish("run-state", val);
                            }
                        }
                    }
                    if has_trailing_newline {
                        state.events_jsonl_offset += n as u64;
                    } else if let Some(pos) = buf.iter().rposition(|&b| b == b'\n') {
                        state.events_jsonl_offset += (pos + 1) as u64;
                    }
                }
            }
        }
    }

    // 3. Read autopilot.log
    if let Ok(mut file) = std::fs::File::open(log_path)
        && let Ok(meta) = file.metadata()
    {
        let len = meta.len();
        if len > state.log_offset {
            use std::io::{Read, Seek, SeekFrom};
            if file.seek(SeekFrom::Start(state.log_offset)).is_ok() {
                let to_read = (len - state.log_offset) as usize;
                let mut buf = vec![0u8; to_read];
                if let Ok(n) = file.read(&mut buf) {
                    buf.truncate(n);
                    let s = String::from_utf8_lossy(&buf);
                    let has_trailing_newline = buf.last() == Some(&b'\n');
                    let lines: Vec<&str> = s.lines().collect();
                    let count = lines.len();
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    for (idx, line) in lines.into_iter().enumerate() {
                        if idx == count - 1 && !has_trailing_newline {
                            break;
                        }
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            let ev = LogEvent {
                                message: trimmed.to_string(),
                                line: trimmed.to_string(),
                                ts: now,
                            };
                            hub.publish("log", serde_json::to_value(ev).unwrap_or_default());
                        }
                    }
                    if has_trailing_newline {
                        state.log_offset += n as u64;
                    } else if let Some(pos) = buf.iter().rposition(|&b| b == b'\n') {
                        state.log_offset += (pos + 1) as u64;
                    }
                }
            }
        }
    }

    // 4. Attention items
    if db_path.exists()
        && let Ok(store) = orch_core::store::Store::open(db_path)
        && let Ok(Some(run)) = store.load_run()
        && let Ok(current_items) = orch_core::attention::attention_items(&store, Some(&run))
    {
        if let Some(prev_items) = state.last_attention_items.take() {
            for old in &prev_items {
                if !current_items.iter().any(|cur| cur.id == old.id) {
                    let ev = AttentionEvent {
                        op: AttentionOp::Remove,
                        item: old.clone().into(),
                    };
                    hub.publish("attention", serde_json::to_value(ev).unwrap_or_default());
                }
            }
            for cur in &current_items {
                if !prev_items.iter().any(|old| old.id == cur.id) {
                    let ev = AttentionEvent {
                        op: AttentionOp::Add,
                        item: cur.clone().into(),
                    };
                    hub.publish("attention", serde_json::to_value(ev).unwrap_or_default());
                }
            }
        }
        state.last_attention_items = Some(current_items);
    }
}
