//! Open items that need developer attention (§6B, §18, §26).
//!
//! Computed from existing run state (tasks, lead queue, wake-ups, plan sync):
//! 1. Blocked agents sitting at a prompt
//! 2. Coders' questions waiting for an answer
//! 3. Held milestones with reasons
//! 4. Failed milestones
//! 5. Unacknowledged lead wake-ups
//! 6. Plans waiting for approval

use crate::error::Result;
use crate::lead_queue::LeadKind;
use crate::run_store::Run;
use crate::store::Store;
use crate::task::State;
use serde::{Deserialize, Serialize};

/// The kinds of items that need developer attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    Blocked,
    Question,
    Held,
    Failed,
    WakeUnread,
    PlanApproval,
}

impl AttentionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Question => "question",
            Self::Held => "held",
            Self::Failed => "failed",
            Self::WakeUnread => "wake_unread",
            Self::PlanApproval => "plan_approval",
        }
    }
}

/// Actions the developer can take on an attention item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionAction {
    Answer,
    Approve,
    Fix,
    Dispatch,
    OpenChat,
}

impl AttentionAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Approve => "approve",
            Self::Fix => "fix",
            Self::Dispatch => "dispatch",
            Self::OpenChat => "open_chat",
        }
    }
}

/// One item needing the developer's attention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionItem {
    pub id: String,
    pub kind: AttentionKind,
    pub task: Option<String>,
    pub agent: Option<String>,
    pub title: String,
    pub detail: Option<String>,
    pub prompt: Option<String>,
    pub since: String,
    pub actions: Vec<AttentionAction>,
}

/// Format seconds since Unix epoch as RFC 3339 UTC string (`YYYY-MM-DDTHH:MM:SSZ`).
pub fn format_rfc3339(secs: i64) -> String {
    let secs = secs.max(0);
    let mut days = secs / 86400;
    let mut rem = secs % 86400;
    if rem < 0 {
        rem += 86400;
        days -= 1;
    }
    let hours = rem / 3600;
    rem %= 3600;
    let mins = rem / 60;
    let s = rem % 60;

    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{mins:02}:{s:02}Z")
}

/// Strip ANSI escape codes and control characters (except newline, carriage return and tab).
pub fn strip_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else if !c.is_control() || c == '\t' || c == '\n' || c == '\r' {
            out.push(c);
        }
    }
    out
}

/// Compute the current list of attention items from the store and run.
pub fn attention_items(store: &Store, run: Option<&Run>) -> Result<Vec<AttentionItem>> {
    let mut items = Vec::new();
    let tasks = store.list_tasks().unwrap_or_default();

    // 1. Blocked agents (from lead queue)
    let mut blocked_seen = std::collections::HashSet::new();
    let unread = store.unread_lead_events().unwrap_or_default();
    let still_open = store.still_open_lead_events().unwrap_or_default();
    for e in unread.into_iter().chain(still_open) {
        if e.kind == LeadKind::Blocked && store.lead_entry_open(&e)? {
            let key = (e.agent.clone(), e.task.clone());
            if blocked_seen.insert(key) {
                let agent_name = e.agent.as_deref().unwrap_or("agent");
                items.push(AttentionItem {
                    id: format!("blocked:{}", e.id),
                    kind: AttentionKind::Blocked,
                    task: e.task.clone(),
                    agent: e.agent.clone(),
                    title: format!("Agent {agent_name} is blocked"),
                    detail: if e.text.is_empty() { None } else { Some(e.text.clone()) },
                    prompt: e.prompt.clone(),
                    since: format_rfc3339(e.ts),
                    actions: vec![AttentionAction::OpenChat],
                });
            }
        }
    }

    // 2. Questions waiting for an answer (Paused or NeedsAnswer milestones)
    for t in tasks.iter().filter(|t| t.status == State::Paused || t.status == State::NeedsAnswer) {
        let ts = store
            .history(&t.id)
            .ok()
            .and_then(|h| h.into_iter().rev().find(|tr| tr.to == t.status).map(|tr| tr.ts))
            .unwrap_or_else(crate::store::now);
        items.push(AttentionItem {
            id: format!("question:{}", t.id),
            kind: AttentionKind::Question,
            task: Some(t.id.clone()),
            agent: t.worker.clone().or_else(|| t.last_worker.clone()),
            title: format!("Milestone {} needs an answer", t.id),
            detail: if t.reason.is_empty() { None } else { Some(t.reason.clone()) },
            prompt: None,
            since: format_rfc3339(ts),
            actions: vec![AttentionAction::Answer],
        });
    }

    // 3. Held milestones (Held)
    for t in tasks.iter().filter(|t| t.status == State::Held) {
        let ts = store
            .history(&t.id)
            .ok()
            .and_then(|h| h.into_iter().rev().find(|tr| tr.to == State::Held).map(|tr| tr.ts))
            .unwrap_or_else(crate::store::now);
        items.push(AttentionItem {
            id: format!("held:{}", t.id),
            kind: AttentionKind::Held,
            task: Some(t.id.clone()),
            agent: t.worker.clone().or_else(|| t.last_worker.clone()),
            title: format!("Milestone {} held for approval", t.id),
            detail: if t.reason.is_empty() { None } else { Some(t.reason.clone()) },
            prompt: None,
            since: format_rfc3339(ts),
            actions: vec![AttentionAction::Approve],
        });
    }

    // 4. Failed milestones (Failed)
    for t in tasks.iter().filter(|t| t.status == State::Failed) {
        let ts = store
            .history(&t.id)
            .ok()
            .and_then(|h| h.into_iter().rev().find(|tr| tr.to == State::Failed).map(|tr| tr.ts))
            .unwrap_or_else(crate::store::now);
        items.push(AttentionItem {
            id: format!("failed:{}", t.id),
            kind: AttentionKind::Failed,
            task: Some(t.id.clone()),
            agent: t.worker.clone().or_else(|| t.last_worker.clone()),
            title: format!("Milestone {} failed", t.id),
            detail: if t.reason.is_empty() { None } else { Some(t.reason.clone()) },
            prompt: None,
            since: format_rfc3339(ts),
            actions: vec![AttentionAction::Dispatch],
        });
    }

    // 5. Unacknowledged lead wake-up
    if let Ok(wake_st) = store.wake_state() {
        let unread = store.unread_lead_events().unwrap_or_default();
        let covered = unread.iter().any(|e| e.id <= wake_st.upto);
        if wake_st.awaiting && covered {
            items.push(AttentionItem {
                id: "wake_unread".to_string(),
                kind: AttentionKind::WakeUnread,
                task: None,
                agent: store.lead_name().ok().flatten().or_else(|| Some("lead".to_string())),
                title: "Lead wake-up unacknowledged".to_string(),
                detail: Some(format!("{} unread lead event(s) waiting for acknowledgement", unread.len())),
                prompt: None,
                since: format_rfc3339(wake_st.sent_at),
                actions: vec![AttentionAction::OpenChat],
            });
        }
    }

    // 6. Plan waiting for approval
    if let Some(r) = run
        && !r.plan.is_empty()
        && !tasks.is_empty()
    {
        let all_pending = tasks.iter().all(|t| t.status == State::Pending || t.status == State::NeedsAnswer);
        let no_worker = tasks.iter().all(|t| t.worker.is_none());
        let in_task_go: bool = store
            .conn
            .query_row("SELECT EXISTS(SELECT 1 FROM task_go)", [], |r| r.get(0))
            .unwrap_or(false);
        let in_history: bool = store
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM history WHERE to_state NOT IN ('pending', 'needs-answer', 'skipped'))",
                [],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if all_pending && no_worker && !in_task_go && !in_history {
            let synced_at: i64 = store
                .conn
                .query_row("SELECT COALESCE(synced_at, 0) FROM run WHERE id=1", [], |r| r.get(0))
                .unwrap_or(0);
            let title = if r.settings.title.is_empty() {
                "Plan waiting for approval".to_string()
            } else {
                format!("Plan waiting for approval: {}", r.settings.title)
            };
            items.push(AttentionItem {
                id: "plan_approval".to_string(),
                kind: AttentionKind::PlanApproval,
                task: None,
                agent: store.lead_name().ok().flatten(),
                title,
                detail: Some(format!("Plan with {} milestone(s) ready for review", r.plan.len())),
                prompt: None,
                since: format_rfc3339(synced_at),
                actions: vec![AttentionAction::Approve],
            });
        }
    }

    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_formatting() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1700000000), "2023-11-14T22:13:20Z");
        assert_eq!(format_rfc3339(1704067200), "2024-01-01T00:00:00Z");
    }

    #[test]
    fn ansi_escape_stripping() {
        assert_eq!(strip_escapes("hello \u{1b}[32mworld\u{1b}[0m\n"), "hello world\n");
        assert_eq!(strip_escapes("line 1\r\nline 2\tfoo\x07"), "line 1\r\nline 2\tfoo");
    }
}
