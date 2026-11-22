//! The developer's conversation with the lead (the GUI's chat).
//!
//! * Every message is **stored before it is delivered**. The API (`POST /chat`) and `orch say`
//!   only write rows; neither types into a pane. A developer message starts `queued`.
//! * The event manager delivers them ([`chat_tick`]): the oldest `queued` message goes to the
//!   lead's pane through the same pane checks the wake-up uses (never into a working or
//!   focused pane), is recorded `sent`, and becomes `acknowledged` when the lead answers with
//!   `orch say`. A failed typing is retried and ends `failed`, with a note that says what to do.
//! * Delivery is idempotent. The attempt is saved BEFORE the prompt is typed, so a crash in
//!   between leaves the message `queued` with a recorded attempt: the next start types it
//!   again, once, marked "sent again" and carrying its `#id` so the lead can tell. A message
//!   that was recorded `sent` is never typed again.
//! * Every stored row and every change of its delivery bumps `seq`; the live stream sends a
//!   `chat` event for each row newer than the last `seq` it sent.
//! * Developer messages go to the lead only. Nothing here addresses another agent.
//! * Secrets never enter a message: [`scrub`] masks bearer tokens and well-known key formats.

use crate::store::{Res, Store, StoreError, now};
use crate::wake::{LeadPane, WakeEnv};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

/// Longest message text kept (characters). Longer text is refused, not cut.
pub const MAX_TEXT: usize = 20_000;
/// Longest single field of a feature request (characters).
pub const MAX_FIELD: usize = 8_000;
/// Typing attempts that fail before the message is `failed`.
pub const MAX_ATTEMPTS: i64 = 3;

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sender {
    Developer,
    Lead,
}

impl Sender {
    pub fn as_str(self) -> &'static str {
        match self {
            Sender::Developer => "developer",
            Sender::Lead => "lead",
        }
    }
    fn parse(s: &str) -> Sender {
        if s == "lead" { Sender::Lead } else { Sender::Developer }
    }
}

/// What a message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKind {
    Message,
    FeatureRequest,
    PlanApproval,
}

impl ChatKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChatKind::Message => "message",
            ChatKind::FeatureRequest => "feature_request",
            ChatKind::PlanApproval => "plan_approval",
        }
    }
    pub fn parse(s: &str) -> Option<ChatKind> {
        match s {
            "message" => Some(ChatKind::Message),
            "feature_request" => Some(ChatKind::FeatureRequest),
            "plan_approval" => Some(ChatKind::PlanApproval),
            _ => None,
        }
    }
}

/// Where a developer message stands on its way to the lead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Stored; not typed into the lead's pane yet.
    Queued,
    /// Typed into the lead's pane; the lead has not answered yet.
    Sent,
    /// The lead answered (`orch say`) after it was sent.
    Acknowledged,
    /// Could not be delivered; the note says what to do.
    Failed,
}

impl Delivery {
    pub fn as_str(self) -> &'static str {
        match self {
            Delivery::Queued => "queued",
            Delivery::Sent => "sent",
            Delivery::Acknowledged => "acknowledged",
            Delivery::Failed => "failed",
        }
    }
    fn parse(s: &str) -> Option<Delivery> {
        match s {
            "queued" => Some(Delivery::Queued),
            "sent" => Some(Delivery::Sent),
            "acknowledged" => Some(Delivery::Acknowledged),
            "failed" => Some(Delivery::Failed),
            _ => None,
        }
    }
}

/// The plan's feature request fields, as separate values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureRequest {
    #[serde(default)]
    pub feature: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub must: String,
    #[serde(default)]
    pub must_not: String,
    #[serde(default)]
    pub decided: String,
    #[serde(default)]
    pub done_when: String,
    #[serde(default)]
    pub mode: String,
}

impl FeatureRequest {
    fn fields(&self) -> [(&'static str, &str); 7] {
        [
            ("Feature", &self.feature),
            ("Why", &self.why),
            ("Must", &self.must),
            ("Must not", &self.must_not),
            ("Already decided", &self.decided),
            ("Done when", &self.done_when),
            ("Mode", &self.mode),
        ]
    }
}

/// One stored message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: i64,
    pub ts: i64,
    pub sender: Sender,
    pub kind: ChatKind,
    pub text: String,
    pub request: Option<FeatureRequest>,
    pub about: Option<String>,
    pub delivery: Option<Delivery>,
    pub delivery_note: Option<String>,
    /// Times the event manager began typing it into the lead's pane.
    pub attempts: i64,
}

/// Why a message was refused.
#[derive(Debug)]
pub enum ChatError {
    /// The message itself is wrong; the text says how.
    Invalid(String),
    Store(StoreError),
}

impl std::fmt::Display for ChatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatError::Invalid(m) => f.write_str(m),
            ChatError::Store(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for ChatError {}

impl From<StoreError> for ChatError {
    fn from(e: StoreError) -> Self {
        ChatError::Store(e)
    }
}

impl From<rusqlite::Error> for ChatError {
    fn from(e: rusqlite::Error) -> Self {
        ChatError::Store(e.into())
    }
}

/// Why the lead cannot receive a message right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeadProblem {
    /// The run has no lead (`Lead mode: off`, or no `Lead:` line in the plan).
    NoLead,
    /// The event manager looked and herdr has no pane for the lead.
    Gone(String),
}

impl LeadProblem {
    /// The API's `lead_unavailable` text: what is wrong and how to start a lead. The message
    /// itself is kept either way.
    pub fn how_to_fix(&self) -> String {
        match self {
            LeadProblem::NoLead => "no lead pane exists: this run has no lead. Open an agent in Herdr, run `orch lead plan` in it and name its pane `lead`. Your message is kept and reaches the lead once there is one.".into(),
            LeadProblem::Gone(name) => format!(
                "no lead pane exists: Herdr has no pane named `{name}`. Start the Lead from the app (it opens that pane for you), or start the lead agent in Herdr yourself, name its pane `{name}` and run `orch lead run` in it. Your message is kept and is typed into the lead as soon as it is there."
            ),
        }
    }
}

// ------------------------------------------------------------------------------ helpers

/// Mask what must never be stored: bearer tokens and the key formats of common services.
/// Everything else is kept as written.
pub fn scrub(text: &str) -> String {
    const PREFIXES: [&str; 8] = [
        "sk-", "ghp_", "gho_", "ghs_", "github_pat_", "xoxb-", "xoxp-", "AKIA",
    ];
    let mut out = String::with_capacity(text.len());
    let mut prev_bearer = false;
    let mut rest = text;
    while !rest.is_empty() {
        // keep whitespace as is, treat the rest as words
        let ws_end = rest.find(|c: char| !c.is_whitespace()).unwrap_or(rest.len());
        out.push_str(&rest[..ws_end]);
        rest = &rest[ws_end..];
        if rest.is_empty() {
            break;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..end];
        let bare = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-');
        let long = bare.len() >= 16;
        let secret = prev_bearer
            || (long && PREFIXES.iter().any(|p| bare.starts_with(p)))
            || key_value_secret(word);
        if secret {
            out.push_str(&mask(word, prev_bearer));
        } else {
            out.push_str(word);
        }
        prev_bearer = word.eq_ignore_ascii_case("bearer");
        rest = &rest[end..];
    }
    out
}

/// `token=abc`, `api_key: abc`, `Authorization=...`: the value of a secret-looking key.
fn key_value_secret(word: &str) -> bool {
    let Some((key, value)) = word.split_once(['=', ':']) else {
        return false;
    };
    let key = key.to_ascii_lowercase();
    let named = ["token", "secret", "password", "passwd", "api_key", "apikey", "api-key"]
        .iter()
        .any(|k| key.ends_with(k));
    named && value.len() >= 8
}

fn mask(word: &str, whole: bool) -> String {
    if whole {
        return "[redacted]".into();
    }
    match word.split_once(['=', ':']) {
        Some((key, _)) if key_value_secret(word) => {
            let sep = &word[key.len()..key.len() + 1];
            format!("{key}{sep}[redacted]")
        }
        _ => "[redacted]".into(),
    }
}

/// `2026-11-15T09:30:00Z` for an epoch value (UTC), the `ts` of the message shape.
pub fn rfc3339(epoch: i64) -> String {
    let days = epoch.div_euclid(86_400);
    let secs = epoch.rem_euclid(86_400);
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

fn clean_field(name: &str, v: &str) -> Result<String, ChatError> {
    let v = scrub(v.trim());
    if v.chars().count() > MAX_FIELD {
        return Err(ChatError::Invalid(format!(
            "`{name}` is longer than {MAX_FIELD} characters: shorten it"
        )));
    }
    Ok(v)
}

/// The text kept for a feature request when the developer wrote none: its feature line.
fn request_summary(r: &FeatureRequest) -> String {
    format!("Feature request: {}", r.feature)
}

// ------------------------------------------------------------------------------ the store

const COLS: &str =
    "id, ts, sender, kind, text, request, about, delivery, delivery_note, attempts";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ChatMessage> {
    let sender: String = r.get(2)?;
    let kind: String = r.get(3)?;
    let request: Option<String> = r.get(5)?;
    let delivery: Option<String> = r.get(7)?;
    Ok(ChatMessage {
        id: r.get(0)?,
        ts: r.get(1)?,
        sender: Sender::parse(&sender),
        kind: ChatKind::parse(&kind).unwrap_or(ChatKind::Message),
        text: r.get(4)?,
        request: request.and_then(|j| serde_json::from_str(&j).ok()),
        about: r.get(6)?,
        delivery: delivery.as_deref().and_then(Delivery::parse),
        delivery_note: r.get(8)?,
        attempts: r.get(9)?,
    })
}

/// Give the row the next `seq`, so the live stream sends it again.
fn bump(tx: &Transaction<'_>, id: i64) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE chat_messages SET seq = (SELECT COALESCE(MAX(seq), 0) + 1 FROM chat_messages)
         WHERE id = ?1",
        [id],
    )?;
    Ok(())
}

fn fetch(tx: &Transaction<'_>, id: i64) -> Res<ChatMessage> {
    Ok(tx.query_row(
        &format!("SELECT {COLS} FROM chat_messages WHERE id=?1"),
        [id],
        row,
    )?)
}

impl Store {
    fn chat_tx(&mut self) -> Res<Transaction<'_>> {
        Ok(self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }

    /// Store a developer message, `queued` for the lead. Nothing is typed anywhere.
    /// A `feature_request` needs `request` with a feature line; `plan_approval` and `message`
    /// take no request. The text is scrubbed of secrets.
    pub fn post_developer_message(
        &mut self,
        kind: ChatKind,
        text: &str,
        request: Option<&FeatureRequest>,
    ) -> Result<ChatMessage, ChatError> {
        let mut text = text.trim().to_string();
        let mut stored_request = None;
        match kind {
            ChatKind::Message => {
                if request.is_some() {
                    return Err(ChatError::Invalid(
                        "`request` only goes with kind `feature_request`".into(),
                    ));
                }
                if text.is_empty() {
                    return Err(ChatError::Invalid("the message is empty: write something to the lead".into()));
                }
            }
            ChatKind::PlanApproval => {
                if request.is_some() {
                    return Err(ChatError::Invalid(
                        "`request` only goes with kind `feature_request`".into(),
                    ));
                }
                if text.is_empty() {
                    text = "Go: the plan is approved. Start the run.".into();
                }
            }
            ChatKind::FeatureRequest => {
                let Some(r) = request else {
                    return Err(ChatError::Invalid(
                        "a feature_request needs `request` with at least `feature`".into(),
                    ));
                };
                let r = FeatureRequest {
                    feature: clean_field("feature", &r.feature)?,
                    why: clean_field("why", &r.why)?,
                    must: clean_field("must", &r.must)?,
                    must_not: clean_field("must_not", &r.must_not)?,
                    decided: clean_field("decided", &r.decided)?,
                    done_when: clean_field("done_when", &r.done_when)?,
                    mode: clean_field("mode", &r.mode)?,
                };
                if r.feature.is_empty() {
                    return Err(ChatError::Invalid(
                        "a feature_request needs `request.feature`: say what the feature is".into(),
                    ));
                }
                if text.is_empty() {
                    text = request_summary(&r);
                }
                stored_request = Some(r);
            }
        }
        let text = scrub(&text);
        if text.chars().count() > MAX_TEXT {
            return Err(ChatError::Invalid(format!(
                "the message is longer than {MAX_TEXT} characters: shorten it"
            )));
        }
        let json = stored_request
            .as_ref()
            .map(|r| serde_json::to_string(r).unwrap_or_default());
        let tx = self.chat_tx()?;
        tx.execute(
            "INSERT INTO chat_messages (ts, sender, kind, text, request, delivery)
             VALUES (?1, 'developer', ?2, ?3, ?4, 'queued')",
            params![now(), kind.as_str(), text, json],
        )?;
        let id = tx.last_insert_rowid();
        bump(&tx, id)?;
        let m = fetch(&tx, id)?;
        tx.commit()?;
        Ok(m)
    }

    /// Store the lead's reply (`orch say`). Everything the lead was sent so far counts as read:
    /// those developer messages become `acknowledged`.
    pub fn post_lead_reply(
        &mut self,
        text: &str,
        about: Option<&str>,
    ) -> Result<ChatMessage, ChatError> {
        let text = scrub(text.trim());
        if text.is_empty() {
            return Err(ChatError::Invalid(
                "the reply is empty: usage: orch say \"<text>\" [--about Mn]".into(),
            ));
        }
        if text.chars().count() > MAX_TEXT {
            return Err(ChatError::Invalid(format!(
                "the reply is longer than {MAX_TEXT} characters: shorten it"
            )));
        }
        let about = about.map(str::trim).filter(|a| !a.is_empty());
        if let Some(a) = about
            && (a.chars().count() > 40 || a.contains(char::is_whitespace))
        {
            return Err(ChatError::Invalid(
                "--about names one milestone, like M2".into(),
            ));
        }
        let tx = self.chat_tx()?;
        tx.execute(
            "INSERT INTO chat_messages (ts, sender, kind, text, about)
             VALUES (?1, 'lead', 'message', ?2, ?3)",
            params![now(), text, about],
        )?;
        let id = tx.last_insert_rowid();
        bump(&tx, id)?;
        let sent: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM chat_messages WHERE sender='developer' AND delivery='sent' AND id < ?1",
            )?;
            stmt.query_map([id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for mid in sent {
            tx.execute(
                "UPDATE chat_messages SET delivery='acknowledged', delivery_note='the lead answered' WHERE id=?1",
                [mid],
            )?;
            bump(&tx, mid)?;
        }
        let m = fetch(&tx, id)?;
        tx.commit()?;
        Ok(m)
    }

    /// One message by id.
    pub fn chat_message(&self, id: i64) -> Res<Option<ChatMessage>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLS} FROM chat_messages WHERE id=?1"),
                [id],
                row,
            )
            .optional()?)
    }

    /// The conversation in order. With `after`, only messages with a greater id; without, the
    /// newest `limit` messages. At most `limit` come back.
    pub fn chat_messages(&self, after: Option<i64>, limit: usize) -> Res<Vec<ChatMessage>> {
        let limit = limit.max(1) as i64;
        let mut out: Vec<ChatMessage> = match after {
            Some(a) => {
                let mut stmt = self.conn.prepare(&format!(
                    "SELECT {COLS} FROM chat_messages WHERE id > ?1 ORDER BY id LIMIT ?2"
                ))?;
                stmt.query_map(params![a, limit], row)?
                    .collect::<rusqlite::Result<_>>()?
            }
            None => {
                let mut stmt = self.conn.prepare(&format!(
                    "SELECT {COLS} FROM chat_messages ORDER BY id DESC LIMIT ?1"
                ))?;
                let mut v: Vec<ChatMessage> = stmt
                    .query_map([limit], row)?
                    .collect::<rusqlite::Result<_>>()?;
                v.reverse();
                v
            }
        };
        out.sort_by_key(|m| m.id);
        Ok(out)
    }

    /// Why the lead cannot receive a message now, or `None` when it can (or when nobody has
    /// looked yet: the message waits `queued`). `manager_running` says whether the last
    /// observation of the pane is current.
    pub fn lead_problem(&self, manager_running: bool) -> Res<Option<LeadProblem>> {
        if !self.lead_on()? {
            return Ok(Some(LeadProblem::NoLead));
        }
        if manager_running
            && let Some((pane, _)) = self.chat_lead_pane()?
            && pane == "gone"
        {
            let name = self.lead_name()?.unwrap_or_else(|| "lead".into());
            return Ok(Some(LeadProblem::Gone(name)));
        }
        Ok(None)
    }

    /// What the event manager last saw of the lead's pane, and when.
    pub fn chat_lead_pane(&self) -> Res<Option<(String, i64)>> {
        Ok(self
            .conn
            .query_row("SELECT pane, seen_at FROM chat_lead WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?)
    }

    /// Record what the event manager saw of the lead's pane (`idle`, `working`, `focused`,
    /// `gone`). The API reads it to answer `lead_unavailable`.
    pub fn set_chat_lead_pane(&mut self, pane: &str, at: i64) -> Res<()> {
        self.conn.execute(
            "INSERT INTO chat_lead (id, pane, seen_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET pane=?1, seen_at=?2",
            params![pane, at],
        )?;
        Ok(())
    }

    /// The oldest developer message still `queued`.
    pub fn next_chat_to_deliver(&self) -> Res<Option<ChatMessage>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {COLS} FROM chat_messages
                     WHERE sender='developer' AND delivery='queued' ORDER BY id LIMIT 1"
                ),
                [],
                row,
            )
            .optional()?)
    }

    /// Record that typing is about to start (BEFORE it does): one more attempt.
    fn chat_begin_delivery(&mut self, id: i64) -> Res<()> {
        let tx = self.chat_tx()?;
        tx.execute(
            "UPDATE chat_messages SET attempts = attempts + 1, delivery_note = 'being typed into the lead''s pane'
             WHERE id=?1 AND delivery='queued'",
            [id],
        )?;
        bump(&tx, id)?;
        tx.commit()?;
        Ok(())
    }

    fn chat_set_delivery(&mut self, id: i64, d: Delivery, note: Option<&str>) -> Res<()> {
        let tx = self.chat_tx()?;
        tx.execute(
            "UPDATE chat_messages SET delivery=?2, delivery_note=?3,
                sent_at = CASE WHEN ?2='sent' THEN ?4 ELSE sent_at END
             WHERE id=?1",
            params![id, d.as_str(), note, now()],
        )?;
        bump(&tx, id)?;
        tx.commit()?;
        Ok(())
    }

    /// Change a queued message's note only when it differs (no stream noise on every cycle).
    fn chat_note_queued(&mut self, id: i64, note: &str) -> Res<()> {
        let tx = self.chat_tx()?;
        let changed = tx.execute(
            "UPDATE chat_messages SET delivery_note=?2
             WHERE id=?1 AND delivery='queued' AND COALESCE(delivery_note, '') <> ?2",
            params![id, note],
        )?;
        if changed > 0 {
            bump(&tx, id)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Retry the messages that `failed`: the developer asked for it (not offered by the API in
    /// this milestone; used by tests and later screens).
    pub fn chat_requeue(&mut self, id: i64) -> Res<bool> {
        let tx = self.chat_tx()?;
        let n = tx.execute(
            "UPDATE chat_messages SET delivery='queued', attempts=0, delivery_note=NULL
             WHERE id=?1 AND delivery='failed'",
            [id],
        )?;
        if n > 0 {
            bump(&tx, id)?;
        }
        tx.commit()?;
        Ok(n > 0)
    }
}

// ------------------------------------------------------------------------ the live stream

/// The newest `seq` in the database at `db_path`, 0 when there is none (read-only).
pub fn init_chat_poll_state(db_path: &std::path::Path) -> i64 {
    let Ok(conn) = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        return 0;
    };
    conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM chat_messages", [], |r| r.get(0))
        .unwrap_or(0)
}

/// Messages stored or changed since `last_seq`, oldest change first; moves `last_seq` on
/// (read-only: the stream never writes).
pub fn poll_chat_changes(db_path: &std::path::Path, last_seq: &mut i64) -> Vec<ChatMessage> {
    let Ok(conn) = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        return Vec::new();
    };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(200));
    let Ok(mut stmt) = conn.prepare(&format!(
        "SELECT {COLS}, seq FROM chat_messages WHERE seq > ?1 ORDER BY seq"
    )) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([*last_seq], |r| Ok((row(r)?, r.get::<_, i64>(10)?))) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (m, seq) in rows.flatten() {
        *last_seq = (*last_seq).max(seq);
        out.push(m);
    }
    out
}

// ------------------------------------------------------------------------------ delivery

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The prompt typed into the lead's pane for a developer message: one line (a typed prompt
/// must not be split by a newline), carrying the message's `#id`. `again` marks a message typed
/// a second time because a restart cut the first attempt short.
pub fn chat_prompt(m: &ChatMessage, again: bool) -> String {
    let again = if again {
        " (typed again after a restart: if you already handled it, ignore this)"
    } else {
        ""
    };
    let reply = "Answer the developer with `orch say \"<your answer>\"` (add `--about Mn` when it is about a milestone), then stop.";
    match (m.kind, &m.request) {
        (ChatKind::FeatureRequest, Some(r)) => {
            let fields: Vec<String> = r
                .fields()
                .iter()
                .map(|(k, v)| {
                    let v = one_line(v);
                    format!("{k}: {}", if v.is_empty() { "(not given)" } else { &v })
                })
                .collect();
            format!(
                "orch chat #{}{again}: the developer sent a FEATURE REQUEST from the GUI. Run `orch lead plan` and follow that playbook for it. {}. {reply} Tell them the plan is ready, or put your questions to them.",
                m.id,
                fields.join(" | ")
            )
        }
        (ChatKind::PlanApproval, _) => format!(
            "orch chat #{}{again}: the developer approved the plan from the GUI: \"go\". Run `orch lead run` and start the run (its section 1). {reply}",
            m.id
        ),
        _ => format!(
            "orch chat #{}{again}: the developer wrote in the GUI chat: \"{}\". {reply}",
            m.id,
            one_line(&m.text)
        ),
    }
}

/// What one pass of [`chat_tick`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatTick {
    /// No message waits, or the run has no lead.
    Nothing,
    /// A message waits and the pane is not idle (or not there): it stays `queued`.
    Held,
    /// The message with this id was typed into the lead's pane (now `sent`).
    Sent(i64),
    /// Typing failed; the message stays `queued` for a retry.
    Retry(i64),
    /// Typing failed for the last time; the message is `failed`.
    Failed(i64),
}

fn pane_word(p: LeadPane) -> &'static str {
    match p {
        LeadPane::Idle => "idle",
        LeadPane::Working => "working",
        LeadPane::FocusedInput => "focused",
        LeadPane::Gone => "gone",
    }
}

/// One pass of chat delivery, run by the event manager each cycle (after the wake-up). Looks
/// at the lead's pane exactly as the wake-up does and types into it only while it is idle:
/// a working pane, or one the developer is typing in, holds the message. At most one message
/// per pass, oldest first.
pub fn chat_tick(
    store: &mut Store,
    env: &WakeEnv<'_>,
    say: &mut dyn FnMut(&str),
) -> Res<ChatTick> {
    let Some(lead) = store
        .lead_name()?
        .filter(|_| store.lead_on().unwrap_or(false))
    else {
        return Ok(ChatTick::Nothing);
    };
    let pane = env.runtime.pane(&lead);
    let at = env.clock.now();
    // `gone` tells the API to answer `lead_unavailable`; keep the observation current
    store.set_chat_lead_pane(pane_word(pane), at)?;
    let Some(m) = store.next_chat_to_deliver()? else {
        return Ok(ChatTick::Nothing);
    };
    match pane {
        LeadPane::Idle => {}
        LeadPane::Gone => {
            store.chat_note_queued(
                m.id,
                &format!("waiting: the lead (`{lead}`) is not in Herdr; start it and name its pane `{lead}`"),
            )?;
            return Ok(ChatTick::Held);
        }
        LeadPane::Working => {
            store.chat_note_queued(m.id, "waiting: the lead is working; the message is typed when it is idle")?;
            return Ok(ChatTick::Held);
        }
        LeadPane::FocusedInput => {
            store.chat_note_queued(m.id, "waiting: the lead's pane is focused with input in progress")?;
            return Ok(ChatTick::Held);
        }
    }
    let again = m.attempts > 0;
    // saved BEFORE typing: a crash from here on types it again (marked), never loses it
    store.chat_begin_delivery(m.id)?;
    match env.runtime.send(&lead, &chat_prompt(&m, again)) {
        Ok(()) => {
            store.chat_set_delivery(m.id, Delivery::Sent, Some("typed into the lead's pane"))?;
            say(&format!("chat #{} sent to the lead ({lead})", m.id));
            Ok(ChatTick::Sent(m.id))
        }
        Err(e) => {
            let tries = m.attempts + 1;
            if tries >= MAX_ATTEMPTS {
                let note = format!(
                    "could not be typed into the lead's pane after {tries} tries ({e}): check that the lead (`{lead}`) is running in Herdr, then send the message again"
                );
                store.chat_set_delivery(m.id, Delivery::Failed, Some(&note))?;
                say(&format!("chat #{} failed: {note}", m.id));
                Ok(ChatTick::Failed(m.id))
            } else {
                let note = format!("typing failed ({e}); trying again");
                store.chat_set_delivery(m.id, Delivery::Queued, Some(&note))?;
                say(&format!("chat #{}: {note}", m.id));
                Ok(ChatTick::Retry(m.id))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lead_queue::tests::store_with_run;
    use crate::wake::{Clock, LeadRuntime, Notifier, WakeTunables};
    use std::cell::{Cell, RefCell};

    struct Lead {
        pane: Cell<LeadPane>,
        sent: RefCell<Vec<String>>,
        fail: Cell<bool>,
    }
    impl Lead {
        fn new(p: LeadPane) -> Self {
            Lead {
                pane: Cell::new(p),
                sent: RefCell::new(vec![]),
                fail: Cell::new(false),
            }
        }
    }
    impl LeadRuntime for Lead {
        fn pane(&self, _: &str) -> LeadPane {
            self.pane.get()
        }
        fn send(&self, _: &str, text: &str) -> Result<(), String> {
            if self.fail.get() {
                return Err("agent target lead not found".into());
            }
            self.sent.borrow_mut().push(text.into());
            Ok(())
        }
    }
    struct Quiet;
    impl Notifier for Quiet {
        fn notify(&self, _: &str) {}
    }
    struct Clk;
    impl Clock for Clk {
        fn now(&self) -> i64 {
            1_000
        }
    }

    fn tick(store: &mut Store, lead: &Lead) -> ChatTick {
        let env = WakeEnv {
            runtime: lead,
            notifier: &Quiet,
            clock: &Clk,
            tun: WakeTunables::default(),
        };
        chat_tick(store, &env, &mut |_| {}).unwrap()
    }

    fn feature() -> FeatureRequest {
        FeatureRequest {
            feature: "Dark mode".into(),
            why: "night owls".into(),
            must: "persist the choice".into(),
            must_not: "touch the API".into(),
            decided: "CSS variables".into(),
            done_when: "the toggle works".into(),
            mode: "worktree".into(),
        }
    }

    #[test]
    fn a_developer_message_is_stored_queued_with_its_sender_and_time() {
        let (_d, mut s) = store_with_run(true);
        let m = s.post_developer_message(ChatKind::Message, " hello ", None).unwrap();
        assert_eq!(
            (m.sender, m.kind, m.text.as_str(), m.delivery),
            (Sender::Developer, ChatKind::Message, "hello", Some(Delivery::Queued))
        );
        assert!(m.ts > 1_600_000_000);
        assert_eq!(s.chat_message(m.id).unwrap().unwrap(), m);
    }

    #[test]
    fn bad_messages_are_refused_and_nothing_is_stored() {
        let (_d, mut s) = store_with_run(true);
        for (kind, text, req) in [
            (ChatKind::Message, "  ", None),
            (ChatKind::FeatureRequest, "x", None),
            (ChatKind::Message, "x", Some(feature())),
            (ChatKind::PlanApproval, "", Some(feature())),
            (ChatKind::FeatureRequest, "", Some(FeatureRequest::default())),
            (ChatKind::Message, &"a".repeat(MAX_TEXT + 1), None),
        ] {
            assert!(
                matches!(
                    s.post_developer_message(kind, text, req.as_ref()),
                    Err(ChatError::Invalid(_))
                ),
                "{kind:?} {text:.10}"
            );
        }
        assert!(s.chat_messages(None, 10).unwrap().is_empty());
    }

    #[test]
    fn a_feature_request_keeps_its_fields_apart_and_is_one_readable_prompt() {
        let (_d, mut s) = store_with_run(true);
        let m = s
            .post_developer_message(ChatKind::FeatureRequest, "", Some(&feature()))
            .unwrap();
        assert_eq!(m.request.as_ref().unwrap().must_not, "touch the API");
        assert_eq!(m.text, "Feature request: Dark mode");
        let p = chat_prompt(&m, false);
        assert!(!p.contains('\n'), "one line: {p}");
        for want in [
            "FEATURE REQUEST",
            "orch lead plan",
            "Feature: Dark mode",
            "Why: night owls",
            "Must: persist the choice",
            "Must not: touch the API",
            "Already decided: CSS variables",
            "Done when: the toggle works",
            "Mode: worktree",
            "orch say",
            "#1",
        ] {
            assert!(p.contains(want), "{want} in {p}");
        }
    }

    #[test]
    fn a_plan_approval_reaches_the_lead_as_go() {
        let (_d, mut s) = store_with_run(true);
        let m = s.post_developer_message(ChatKind::PlanApproval, "", None).unwrap();
        let p = chat_prompt(&m, false);
        assert!(p.contains("\"go\"") && p.contains("orch lead run"), "{p}");
    }

    #[test]
    fn secrets_are_masked_before_they_are_stored() {
        let (_d, mut s) = store_with_run(true);
        let text = "use Authorization: Bearer abcdef0123456789abcdef and token=ghp_0123456789abcdefghij and sk-abcdefghijklmnop1234 ok";
        let m = s.post_developer_message(ChatKind::Message, text, None).unwrap();
        assert!(!m.text.contains("abcdef0123456789"), "{}", m.text);
        assert!(!m.text.contains("ghp_"), "{}", m.text);
        assert!(!m.text.contains("sk-abc"), "{}", m.text);
        assert!(m.text.ends_with("ok") && m.text.starts_with("use "), "{}", m.text);
        let r = s.post_lead_reply("api_key=supersecretvalue1", None).unwrap();
        assert!(!r.text.contains("supersecret"), "{}", r.text);
        assert_eq!(scrub("plain words stay"), "plain words stay");
    }

    #[test]
    fn the_idle_lead_gets_the_oldest_message_once() {
        let (_d, mut s) = store_with_run(true);
        let a = s.post_developer_message(ChatKind::Message, "one", None).unwrap();
        let b = s.post_developer_message(ChatKind::Message, "two", None).unwrap();
        let lead = Lead::new(LeadPane::Idle);
        assert_eq!(tick(&mut s, &lead), ChatTick::Sent(a.id));
        assert_eq!(s.chat_message(a.id).unwrap().unwrap().delivery, Some(Delivery::Sent));
        assert_eq!(tick(&mut s, &lead), ChatTick::Sent(b.id));
        assert_eq!(tick(&mut s, &lead), ChatTick::Nothing);
        assert_eq!(lead.sent.borrow().len(), 2, "each exactly once, in order");
        assert!(lead.sent.borrow()[0].contains("\"one\""));
    }

    #[test]
    fn a_busy_or_focused_or_missing_lead_is_never_typed_into() {
        let (_d, mut s) = store_with_run(true);
        let m = s.post_developer_message(ChatKind::Message, "hi", None).unwrap();
        let lead = Lead::new(LeadPane::Working);
        for pane in [LeadPane::Working, LeadPane::FocusedInput, LeadPane::Gone] {
            lead.pane.set(pane);
            assert_eq!(tick(&mut s, &lead), ChatTick::Held);
        }
        assert!(lead.sent.borrow().is_empty());
        let m2 = s.chat_message(m.id).unwrap().unwrap();
        assert_eq!(m2.delivery, Some(Delivery::Queued));
        assert!(m2.delivery_note.unwrap().contains("not in Herdr"));
        lead.pane.set(LeadPane::Idle);
        assert_eq!(tick(&mut s, &lead), ChatTick::Sent(m.id));
    }

    // AC2: a restart in the middle neither loses nor duplicates
    #[test]
    fn a_restart_in_the_middle_neither_loses_nor_doubles_a_message() {
        let (dir, mut s) = store_with_run(true);
        let m = s.post_developer_message(ChatKind::Message, "ship it", None).unwrap();

        // crash after the attempt was saved but before (or while) the prompt was typed
        s.chat_begin_delivery(m.id).unwrap();
        drop(s);
        let mut s = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
        let crashed = s.chat_message(m.id).unwrap().unwrap();
        assert_eq!((crashed.delivery, crashed.attempts), (Some(Delivery::Queued), 1));

        let lead = Lead::new(LeadPane::Idle);
        assert_eq!(tick(&mut s, &lead), ChatTick::Sent(m.id), "not lost");
        assert!(lead.sent.borrow()[0].contains("typed again after a restart"));
        assert_eq!(tick(&mut s, &lead), ChatTick::Nothing);

        // a restart after it was recorded `sent` never types it again
        drop(s);
        let mut s = Store::open_in(&dir.path().join(".orchestrate")).unwrap();
        assert_eq!(tick(&mut s, &lead), ChatTick::Nothing);
        assert_eq!(lead.sent.borrow().len(), 1, "typed exactly once after the restart");
    }

    #[test]
    fn a_failing_pane_retries_then_fails_with_what_to_do_and_can_be_requeued() {
        let (_d, mut s) = store_with_run(true);
        let m = s.post_developer_message(ChatKind::Message, "hi", None).unwrap();
        let lead = Lead::new(LeadPane::Idle);
        lead.fail.set(true);
        assert_eq!(tick(&mut s, &lead), ChatTick::Retry(m.id));
        assert_eq!(tick(&mut s, &lead), ChatTick::Retry(m.id));
        assert_eq!(tick(&mut s, &lead), ChatTick::Failed(m.id));
        let f = s.chat_message(m.id).unwrap().unwrap();
        assert_eq!(f.delivery, Some(Delivery::Failed));
        assert!(f.delivery_note.unwrap().contains("send the message again"));
        assert_eq!(tick(&mut s, &lead), ChatTick::Nothing, "failed is not retried by itself");
        lead.fail.set(false);
        assert!(s.chat_requeue(m.id).unwrap());
        assert_eq!(tick(&mut s, &lead), ChatTick::Sent(m.id));
    }

    #[test]
    fn the_lead_reply_is_stored_and_acknowledges_what_was_sent() {
        let (_d, mut s) = store_with_run(true);
        let a = s.post_developer_message(ChatKind::Message, "q", None).unwrap();
        let lead = Lead::new(LeadPane::Idle);
        tick(&mut s, &lead);
        let late = s.post_developer_message(ChatKind::Message, "later", None).unwrap();
        let r = s.post_lead_reply("on it", Some("M1")).unwrap();
        assert_eq!(
            (r.sender, r.about.as_deref(), r.delivery),
            (Sender::Lead, Some("M1"), None)
        );
        assert_eq!(
            s.chat_message(a.id).unwrap().unwrap().delivery,
            Some(Delivery::Acknowledged)
        );
        assert_eq!(
            s.chat_message(late.id).unwrap().unwrap().delivery,
            Some(Delivery::Queued),
            "a message not yet sent is not acknowledged by an earlier reply"
        );
        assert!(s.post_lead_reply("  ", None).is_err());
        assert!(s.post_lead_reply("x", Some("two words")).is_err());
    }

    #[test]
    fn the_conversation_is_in_order_and_after_returns_only_newer() {
        let (_d, mut s) = store_with_run(true);
        for i in 0..5 {
            s.post_developer_message(ChatKind::Message, &format!("m{i}"), None).unwrap();
        }
        let all = s.chat_messages(None, 100).unwrap();
        assert_eq!(all.iter().map(|m| m.id).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
        let newer = s.chat_messages(Some(3), 100).unwrap();
        assert_eq!(newer.iter().map(|m| m.id).collect::<Vec<_>>(), [4, 5]);
        let tail = s.chat_messages(None, 2).unwrap();
        assert_eq!(tail.iter().map(|m| m.id).collect::<Vec<_>>(), [4, 5]);
    }

    #[test]
    fn every_store_and_delivery_change_is_seen_by_the_stream_poll() {
        let (dir, mut s) = store_with_run(true);
        let db = dir.path().join(".orchestrate").join(crate::store::DB_FILE);
        let mut last = init_chat_poll_state(&db);
        assert_eq!(last, 0);
        let m = s.post_developer_message(ChatKind::Message, "hi", None).unwrap();
        let seen = poll_chat_changes(&db, &mut last);
        assert_eq!((seen.len(), seen[0].delivery), (1, Some(Delivery::Queued)));
        assert!(poll_chat_changes(&db, &mut last).is_empty(), "nothing new, nothing sent");
        tick(&mut s, &Lead::new(LeadPane::Idle));
        let seen = poll_chat_changes(&db, &mut last);
        assert_eq!(seen.last().unwrap().delivery, Some(Delivery::Sent));
        assert!(seen.iter().all(|x| x.id == m.id));
        s.post_lead_reply("ok", None).unwrap();
        let seen = poll_chat_changes(&db, &mut last);
        assert_eq!(seen.len(), 2, "the reply and the acknowledgement");
        // a stream that starts later starts after what already exists
        assert_eq!(init_chat_poll_state(&db), last);
    }

    #[test]
    fn lead_problem_says_when_there_is_no_lead_or_the_pane_is_gone() {
        let (_d, s) = store_with_run(false);
        assert_eq!(s.lead_problem(true).unwrap(), Some(LeadProblem::NoLead));
        assert!(LeadProblem::NoLead.how_to_fix().contains("orch lead plan"));
        assert!(LeadProblem::Gone("lead".into()).how_to_fix().contains("Start the Lead from the app"));
        let (_d, mut s) = store_with_run(true);
        assert_eq!(s.lead_problem(true).unwrap(), None, "nobody looked yet");
        tick(&mut s, &Lead::new(LeadPane::Gone));
        assert_eq!(
            s.lead_problem(true).unwrap(),
            Some(LeadProblem::Gone("lead".into()))
        );
        assert_eq!(s.lead_problem(false).unwrap(), None, "a stale look is not trusted");
        tick(&mut s, &Lead::new(LeadPane::Working));
        assert_eq!(s.lead_problem(true).unwrap(), None);
    }

    #[test]
    fn timestamps_are_utc_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(rfc3339(1_709_164_800), "2024-02-29T00:00:00Z");
    }
}
