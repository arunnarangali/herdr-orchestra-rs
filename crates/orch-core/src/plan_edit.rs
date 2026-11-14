//! The lead's levers on the stored run (§17): `orch dispatch`, `answer`, `fix` and
//! `escalate`, as Python v4.10.1's `cmd_dispatch`, `cmd_answer`, `cmd_fix` and `cmd_escalate`.
//!
//! Each [`Store`] method here is ONE `BEGIN IMMEDIATE` transaction that re-reads the task, makes
//! every state change (through [`apply_transition`]), the dispatch intent and the lead's queue
//! entry, and, for `answer` and `fix`, stages the new `plan.md` next to the real one. The staged
//! file replaces `plan.md` only after the transaction commits, so a refusal or a failure leaves
//! neither a half-written `plan.md` nor a half-changed state. The user-facing refusal texts are
//! Python's, word for word.
//!
//! The text edits ([`add_answer_note`], [`fix_section`], [`append_fix`]) are pure functions on
//! the plan text and keep every other byte of the file as it was.

use crate::lead_queue::{LeadKind, emit_answer_provided, enqueue};
use crate::run_store::AgentRole;
use crate::scheduler::{Escalation, Intent, TIERS, escalate, need_rank};
use crate::store::{Store, StoreError, TransitionRequest, apply_transition, load_task, now};
use crate::task::State;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Why a lever did not work.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    /// A refusal, with the exact wording Python prints (`ERROR: <text>`, exit 2).
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("{0}")]
    Io(String),
}

impl From<rusqlite::Error> for EditError {
    fn from(e: rusqlite::Error) -> Self {
        EditError::Store(e.into())
    }
}

type Res<T> = Result<T, EditError>;

fn refuse<T>(msg: String) -> Res<T> {
    Err(EditError::Refused(msg))
}

/// Python's `repr` of a string (what `{x!r}` prints).
pub fn py_repr(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\'', "\\'"))
    }
}

// ---- plan text (pure)

/// `## [x] M3: title` -> `M3` (Python `HEAD_RE`).
fn head_id(line: &str) -> Option<String> {
    let rest = line.strip_prefix("## [")?;
    let mut chars = rest.chars();
    let mark = chars.next()?;
    if !matches!(mark, ' ' | 'x' | 'X') {
        return None;
    }
    let rest = chars.as_str().strip_prefix(']')?.trim_start();
    let rest = rest.strip_prefix('M')?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let after = rest[digits.len()..].trim_start().strip_prefix(':')?;
    if after.trim().is_empty() {
        return None;
    }
    Some(format!("M{digits}"))
}

/// Python `FIELD_RE`: `Key: value` with a capitalised key of letters and spaces.
fn field_line(line: &str) -> Option<(String, String)> {
    let (key, val) = line.split_once(':')?;
    let mut ch = key.chars();
    if !ch.next()?.is_ascii_uppercase()
        || key.len() < 2
        || !key.chars().all(|c| c.is_ascii_alphabetic() || c == ' ')
    {
        return None;
    }
    Some((key.trim().to_lowercase(), val.trim().to_string()))
}

/// The `Key: value` fields of every `## [x] Mn:` section (Python `parse_plan`), keyed by the
/// lower-cased field name; each value is the field's lines.
fn section_fields(text: &str) -> Vec<(String, HashMap<String, Vec<String>>)> {
    let mut out: Vec<(String, HashMap<String, Vec<String>>)> = Vec::new();
    let mut in_header = true;
    let mut cur_field: Option<String> = None; // key of the open field in the open section
    let mut in_section = false;
    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some(id) = head_id(line) {
            out.push((id, HashMap::new()));
            in_section = true;
            cur_field = None;
            in_header = false;
            continue;
        }
        if line.starts_with("## ") {
            in_section = false;
            cur_field = None;
            in_header = false;
            continue;
        }
        let fm = field_line(line.trim());
        if let Some((key, val)) = &fm
            && !line.starts_with([' ', '\t', '-'])
            && (in_section || in_header)
        {
            if in_section {
                let f = &mut out.last_mut().expect("section").1;
                f.insert(
                    key.clone(),
                    if val.is_empty() {
                        vec![]
                    } else {
                        vec![val.clone()]
                    },
                );
                cur_field = Some(key.clone());
            } else {
                cur_field = None;
            }
            continue;
        }
        if fm.is_some() && !in_section && !in_header {
            cur_field = None;
            continue;
        }
        if let (Some(key), true) = (&cur_field, !line.trim().is_empty())
            && in_section
        {
            out.last_mut()
                .expect("section")
                .1
                .get_mut(key)
                .expect("key")
                .push(line.trim().to_string());
        }
    }
    out
}

fn field(fields: &HashMap<String, Vec<String>>, key: &str, default: &str) -> String {
    match fields.get(key) {
        Some(v) if !v.is_empty() => v.join(" ").trim().to_string(),
        _ => default.to_string(),
    }
}

/// Put `- <text>` under the milestone's `Notes from user:` line (creating the line at the end
/// of the section when it is missing). `None` when `plan` has no section for `tid`. Every other
/// byte of the file is unchanged (Python `cmd_answer`).
pub fn add_answer_note(plan: &str, tid: &str, text: &str) -> Option<String> {
    let note = format!("- {text}");
    let mut out: Vec<String> = Vec::new();
    let mut cur: Option<String> = None;
    let mut done = false;
    let trim_blank = |out: &mut Vec<String>| {
        while out.last().is_some_and(|l| l.trim().is_empty()) {
            out.pop();
        }
    };
    for line in plan.split('\n') {
        let head = head_id(line);
        let new_section = head.is_some() || line.starts_with("## ");
        if new_section && cur.as_deref() == Some(tid) && !done {
            trim_blank(&mut out);
            out.extend(["Notes from user:".to_string(), note.clone(), String::new()]);
            done = true;
        }
        if new_section {
            cur = head;
        }
        out.push(line.to_string());
        if cur.as_deref() == Some(tid)
            && !done
            && line
                .get(..16)
                .is_some_and(|p| p.eq_ignore_ascii_case("notes from user:"))
        {
            out.push(note.clone());
            done = true;
        }
    }
    if !done {
        if cur.as_deref() != Some(tid) {
            return None;
        }
        trim_blank(&mut out);
        out.extend(["Notes from user:".to_string(), note, String::new()]);
    }
    Some(out.join("\n"))
}

/// The next milestone number: one above every `Mn` in the store and in the plan text.
fn next_number(task_ids: &[String], plan: &str) -> u64 {
    let n = |id: &str| id.strip_prefix('M').and_then(|d| d.parse::<u64>().ok());
    task_ids
        .iter()
        .cloned()
        .chain(section_fields(plan).into_iter().map(|(id, _)| id))
        .filter_map(|id| n(&id))
        .max()
        .unwrap_or(0)
        + 1
}

/// What the fix section copies from the milestone it follows.
pub struct FixSource {
    pub review: String,
    pub likely_files: String,
    pub tier: Option<String>,
}

/// The fix's title and plan section (Python `cmd_fix`).
pub fn fix_section(new: &str, tid: &str, text: &str, src: &FixSource) -> (String, String) {
    let cut: String = text.chars().take(60).collect();
    let title = format!(
        "Fix for {tid}: {cut}{}",
        if text.chars().count() > 60 { "..." } else { "" }
    );
    let mut sec = vec![
        format!("## [x] {new}: {title}"),
        format!("Depends on: {tid} (a fix to what {tid} built)"),
        format!("Why separate: a change the developer asked for after {tid} landed."),
        format!("Review: {}", src.review),
    ];
    if !src.likely_files.is_empty() {
        sec.push(format!("Likely files: {}", src.likely_files));
    }
    if let Some(t) = &src.tier {
        sec.push(format!("Tier: {t}"));
        sec.push(format!(
            "Model why: a fix to {tid}, for the coder who built it."
        ));
    }
    sec.extend([
        format!("Goal: {text}"),
        format!("Why: the developer asked for this after {tid} landed."),
        "Acceptance criteria:".to_string(),
        format!("- {text}"),
        format!("- nothing else that {tid} built changes, and the check command passes"),
        "Open questions:".to_string(),
        "- none".to_string(),
        String::new(),
    ]);
    (title, sec.join("\n"))
}

/// The plan with a section appended after one blank line.
pub fn append_fix(plan: &str, section: &str) -> String {
    format!("{}\n\n{section}", plan.trim_end_matches('\n'))
}

// ---- the staged plan.md write

/// A new `plan.md` written beside the old one; it takes the old one's place only on
/// [`Staged::commit`], and is removed if dropped before.
struct Staged {
    tmp: PathBuf,
    dest: PathBuf,
    done: bool,
}

impl Staged {
    fn new(dest: &Path, text: &str) -> Res<Staged> {
        let name = dest
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tmp = dest.with_file_name(format!(".{name}.orch-{}.tmp", std::process::id()));
        std::fs::write(&tmp, text)
            .map_err(|e| EditError::Io(format!("cannot write {}: {e}", tmp.display())))?;
        if let Ok(meta) = std::fs::metadata(dest) {
            let _ = std::fs::set_permissions(&tmp, meta.permissions());
        }
        Ok(Staged {
            tmp,
            dest: dest.to_path_buf(),
            done: false,
        })
    }

    fn commit(mut self) -> Res<()> {
        std::fs::rename(&self.tmp, &self.dest)
            .map_err(|e| EditError::Io(format!("cannot replace {}: {e}", self.dest.display())))?;
        self.done = true;
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.done {
            let _ = std::fs::remove_file(&self.tmp);
        }
    }
}

// ---- transaction helpers

fn plan_order(conn: &Connection) -> Res<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.id FROM tasks t LEFT JOIN task_plan p ON p.task_id=t.id
         ORDER BY COALESCE(p.position, 1000000000), t.rowid",
    )?;
    Ok(stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn task_of(tx: &Transaction<'_>, tid: &str) -> Res<crate::task::Task> {
    match load_task(tx, tid)? {
        Some(t) => Ok(t),
        None => {
            let known = plan_order(tx)?;
            refuse(format!(
                "unknown task {} — known: {}",
                py_repr(tid),
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            ))
        }
    }
}

struct Agent {
    name: String,
    tier: Option<String>,
}

fn workers(conn: &Connection) -> Res<Vec<Agent>> {
    let mut stmt =
        conn.prepare("SELECT name, tier FROM run_agents WHERE role=?1 ORDER BY position")?;
    Ok(stmt
        .query_map([AgentRole::Worker.as_str()], |r| {
            Ok(Agent {
                name: r.get(0)?,
                tier: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

fn check_worker(conn: &Connection, to: &str) -> Res<Vec<Agent>> {
    let ws = workers(conn)?;
    if !ws.iter().any(|w| w.name == to) {
        let names: Vec<&str> = ws.iter().map(|w| w.name.as_str()).collect();
        return refuse(format!(
            "unknown worker {} — workers: {}",
            py_repr(to),
            names.join(", ")
        ));
    }
    Ok(ws)
}

fn load_intent(conn: &Connection, tid: &str) -> Res<Intent> {
    Ok(conn
        .query_row(
            "SELECT go, assign, min_tier, ready_ts FROM dispatch_intent WHERE task_id=?1",
            [tid],
            |r| {
                Ok(Intent {
                    go: r.get::<_, i64>(0)? != 0,
                    assign: r.get(1)?,
                    min_tier: r.get(2)?,
                    ready_ts: r.get(3)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

/// Store the intent and mark the milestone started (so it leaves the READY list).
fn save_intent(conn: &Connection, tid: &str, i: &Intent) -> Res<()> {
    conn.execute(
        "INSERT OR REPLACE INTO dispatch_intent (task_id, go, assign, min_tier, ready_ts)
         VALUES (?1,?2,?3,?4,?5)",
        params![tid, i.go, i.assign, i.min_tier, i.ready_ts],
    )?;
    if i.go {
        conn.execute(
            "INSERT OR IGNORE INTO task_go (task_id, ts) VALUES (?1, ?2)",
            params![tid, now()],
        )?;
    }
    Ok(())
}

/// Python `_park(.., "pending", why)`: free the coder (it stays `last_worker`) and move the
/// milestone to `pending`.
fn park_pending(tx: &Transaction<'_>, tid: &str, why: &str) -> Res<()> {
    let t = task_of(tx, tid)?;
    apply_transition(tx, tid, &TransitionRequest::auto(State::Pending, Some(why)))?;
    if t.worker.is_some() {
        tx.execute(
            "UPDATE tasks SET last_worker=worker, worker=NULL WHERE id=?1",
            [tid],
        )?;
    }
    Ok(())
}

fn fresh_round(tx: &Transaction<'_>, tid: &str) -> Res<()> {
    tx.execute(
        "UPDATE tasks SET fix_rounds=0, review_rounds=0 WHERE id=?1",
        [tid],
    )?;
    Ok(())
}

/// What `orch answer` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    /// The coder the milestone goes back to, when it had one.
    pub last_worker: Option<String>,
}

/// What `orch fix` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixed {
    pub id: String,
    pub title: String,
    /// The coder the fix is sent to.
    pub who: Option<String>,
    /// The fix carries a review.
    pub review: bool,
}

impl Store {
    fn immediate(&mut self) -> Res<Transaction<'_>> {
        Ok(self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }

    /// `orch dispatch <Mn>... [--to w] [--tier t]`: every milestone must be pending, paused or
    /// failed (else none changes). Each gets `go`, the optional coder and minimum tier;
    /// paused and failed ones go back to `pending` as a fresh round.
    pub fn dispatch_milestones(
        &mut self,
        tids: &[String],
        to: Option<&str>,
        tier: Option<&str>,
    ) -> Res<()> {
        let tx = self.immediate()?;
        if let Some(to) = to {
            check_worker(&tx, to)?;
        }
        if let Some(t) = tier
            && !TIERS.contains(&t)
        {
            return refuse("--tier must be small, medium or high".into());
        }
        for tid in tids {
            let t = task_of(&tx, tid)?;
            if !matches!(t.status, State::Pending | State::Paused | State::Failed) {
                return refuse(format!(
                    "{tid} is {} — only pending, paused or failed milestones can be dispatched{}",
                    t.status,
                    if t.status == State::NeedsAnswer {
                        " (answer its open questions in plan.md, then `orch sync`)"
                    } else {
                        ""
                    }
                ));
            }
        }
        for tid in tids {
            let t = task_of(&tx, tid)?;
            let mut i = load_intent(&tx, tid)?;
            i.go = true;
            i.ready_ts = None;
            if let Some(to) = to {
                i.assign = Some(to.to_string());
            }
            if let Some(tier) = tier {
                i.min_tier = Some(tier.to_string());
            }
            save_intent(&tx, tid, &i)?;
            if matches!(t.status, State::Paused | State::Failed) {
                fresh_round(&tx, tid)?;
                park_pending(&tx, tid, "dispatched again by the lead")?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// `orch answer <Mn> <text>`: the milestone must be paused. Writes the text under its
    /// `Notes from user:` in `plan_path`, sends it back to the same coder, and queues an
    /// `AnswerProvided` entry for the lead whoever answered.
    pub fn answer_milestone(&mut self, plan_path: &Path, tid: &str, text: &str) -> Res<Answered> {
        let tx = self.immediate()?;
        let t = task_of(&tx, tid)?;
        if t.status != State::Paused {
            return refuse(format!(
                "{tid} is {}, not paused{}",
                t.status,
                if t.status == State::NeedsAnswer {
                    " — its questions are in plan.md: answer them under \"Notes from user\", set \"Open questions:\" to `- none`, then `orch sync`"
                } else {
                    ""
                }
            ));
        }
        let body = std::fs::read_to_string(plan_path)
            .map_err(|e| EditError::Io(format!("cannot read {}: {e}", plan_path.display())))?;
        let Some(new_body) = add_answer_note(&body, tid, text) else {
            return refuse(format!("{tid} not found in plan.md"));
        };
        let staged = Staged::new(plan_path, &new_body)?;
        let last = t.last_worker.clone();
        let mut i = load_intent(&tx, tid)?;
        i.go = true;
        if let Some(l) = &last
            && workers(&tx)?.iter().any(|w| &w.name == l)
        {
            i.assign = Some(l.clone());
        }
        save_intent(&tx, tid, &i)?;
        let short: String = text.chars().take(200).collect();
        park_pending(&tx, tid, &format!("answered: {short}"))?;
        emit_answer_provided(&tx, tid, text)?;
        tx.commit()?;
        staged.commit()?;
        Ok(Answered { last_worker: last })
    }

    /// `orch fix <Mn> <change> [--to w]`: Mn must be merged. Appends the follow-up section to
    /// `plan_path`, stores the next milestone as pending with `go` and the coder who built Mn
    /// (or `to`), depending on Mn.
    pub fn fix_milestone(
        &mut self,
        plan_path: &Path,
        tid: &str,
        text: &str,
        to: Option<&str>,
    ) -> Res<Fixed> {
        let tx = self.immediate()?;
        let t = task_of(&tx, tid)?;
        if t.status != State::Merged {
            return refuse(format!(
                "{tid} is {}, not landed yet — `orch fix` is for work that already landed{}",
                t.status,
                if t.status == State::Paused {
                    format!(" (a paused milestone takes `orch answer {tid} \"<text>\"`)")
                } else {
                    String::new()
                }
            ));
        }
        let team = match to {
            Some(to) => check_worker(&tx, to)?,
            None => workers(&tx)?,
        };
        let body = std::fs::read_to_string(plan_path)
            .map_err(|e| EditError::Io(format!("cannot read {}: {e}", plan_path.display())))?;
        let sections = section_fields(&body);
        let Some((_, fields)) = sections.iter().find(|(id, _)| id == tid) else {
            return refuse(format!("{tid} not found in plan.md"));
        };
        let ids: Vec<String> = plan_order(&tx)?;
        let new = format!("M{}", next_number(&ids, &body));
        let built: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT worker FROM history WHERE task_id=?1 AND worker IS NOT NULL ORDER BY id DESC",
            )?;
            stmt.query_map([tid], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let in_team = |w: &&String| team.iter().any(|a| &a.name == *w);
        let who: Option<String> = match to {
            Some(to) => Some(to.to_string()),
            None => t
                .worker
                .iter()
                .chain(t.last_worker.iter())
                .chain(built.iter())
                .find(in_team)
                .cloned(),
        };
        let (plan_tier, plan_files): (Option<String>, String) = tx
            .query_row(
                "SELECT tier, likely_files FROM task_plan WHERE task_id=?1",
                [tid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or((None, "[]".into()));
        let review = field(fields, "review", "no");
        let src = FixSource {
            review: review.clone(),
            likely_files: field(fields, "likely files", ""),
            tier: plan_tier.clone(),
        };
        let (title, section) = fix_section(&new, tid, text, &src);
        let staged = Staged::new(plan_path, &append_fix(&body, &section))?;

        let has_reviewer: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM run WHERE id=1 AND reviewer IS NOT NULL)",
            [],
            |r| r.get(0),
        )?;
        let review_flag = review.to_lowercase().starts_with('y') && has_reviewer;
        tx.execute(
            "INSERT INTO tasks (id, title, reason) VALUES (?1, ?2, ?3)",
            params![new, title, format!("fix for {tid}")],
        )?;
        tx.execute(
            "INSERT INTO task_deps (task_id, dep_id) VALUES (?1, ?2)",
            params![new, tid],
        )?;
        let position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM task_plan",
            [],
            |r| r.get(0),
        )?;
        let model_why = if plan_tier.is_some() {
            format!("a fix to {tid}, for the coder who built it.")
        } else {
            String::new()
        };
        tx.execute(
            "INSERT INTO task_plan (task_id, position, enabled, tier, needs, model_why, prefer,
                 review, likely_files, parallel)
             VALUES (?1,?2,1,?3,'[]',?4,'any',?5,?6,0)",
            params![new, position, plan_tier, model_why, review_flag, plan_files],
        )?;
        let intent = Intent {
            go: true,
            assign: who.clone(),
            ..Intent::default()
        };
        save_intent(&tx, &new, &intent)?;
        tx.commit()?;
        staged.commit()?;
        Ok(Fixed {
            id: new,
            title,
            who,
            review: review_flag,
        })
    }

    /// `orch escalate <Mn> [reason]`: one more go on the next stronger tier the team has,
    /// once. `Ok(None)` means no escalation is possible (nothing changed); `Ok(Some(tier))`
    /// moved the milestone to `pending` with that minimum tier and queued `escalated`.
    pub fn escalate_milestone(&mut self, tid: &str, reason: &str) -> Res<Option<&'static str>> {
        let tx = self.immediate()?;
        let t = task_of(&tx, tid)?;
        let team = workers(&tx)?;
        let plan_tier: Option<String> = tx
            .query_row("SELECT tier FROM task_plan WHERE task_id=?1", [tid], |r| {
                r.get(0)
            })
            .optional()?
            .flatten();
        let mut intent = load_intent(&tx, tid)?;
        let cur = t
            .worker
            .as_ref()
            .or(t.last_worker.as_ref())
            .and_then(|w| team.iter().find(|a| &a.name == w))
            .and_then(|a| a.tier.clone());
        let need = need_rank(plan_tier.as_deref(), intent.min_tier.as_deref());
        let tiers: Vec<Option<&str>> = team.iter().map(|a| a.tier.as_deref()).collect();
        let Escalation::ToTier(tier) = escalate(t.escalated, cur.as_deref(), need, &tiers) else {
            return Ok(None);
        };
        tx.execute("UPDATE tasks SET escalated=1 WHERE id=?1", [tid])?;
        intent.min_tier = Some(tier.to_string());
        intent.assign = None;
        save_intent(&tx, tid, &intent)?;
        fresh_round(&tx, tid)?;
        let why = if reason.is_empty() {
            format!("escalated to a {tier}-tier model")
        } else {
            format!("escalated to a {tier}-tier model: {reason}")
        };
        park_pending(&tx, tid, &why)?;
        enqueue(&tx, LeadKind::Escalated, Some(tid), &why)?;
        tx.commit()?;
        Ok(Some(tier))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "# Plan: Demo\nBranch: b\n\n## [x] M1: One\nTier: medium\nReview: yes\nLikely files: a.rs, b.rs\nGoal:\n- g\n\n## [x] M2: Two\nGoal:\n- h\n\n## Global instructions\nNotes from user: not a milestone\n";

    #[test]
    fn heads_and_fields_parse_like_python() {
        assert_eq!(head_id("## [x] M12: title"), Some("M12".into()));
        assert_eq!(head_id("## [ ]M3 :  t "), Some("M3".into()));
        assert_eq!(head_id("## [y] M1: t"), None);
        assert_eq!(head_id("## Global"), None);
        assert_eq!(
            field_line("Likely files: a, b"),
            Some(("likely files".into(), "a, b".into()))
        );
        assert_eq!(field_line("- Goal: x"), None);
        let f = section_fields(PLAN);
        assert_eq!(f.len(), 2);
        assert_eq!(field(&f[0].1, "likely files", ""), "a.rs, b.rs");
        assert_eq!(field(&f[0].1, "goal", ""), "- g");
        assert_eq!(field(&f[1].1, "review", "no"), "no");
    }

    #[test]
    fn a_note_creates_the_line_at_the_end_of_the_section() {
        let out = add_answer_note(PLAN, "M1", "use sqlite").unwrap();
        assert_eq!(
            out,
            PLAN.replace(
                "- g\n\n## [x] M2",
                "- g\nNotes from user:\n- use sqlite\n\n## [x] M2"
            )
        );
        // the last milestone, followed by a non-milestone section
        let out = add_answer_note(PLAN, "M2", "yes").unwrap();
        assert!(out.contains("- h\nNotes from user:\n- yes\n\n## Global instructions\nNotes from user: not a milestone\n"), "{out}");
    }

    #[test]
    fn a_note_goes_under_an_existing_line_and_the_rest_is_byte_identical() {
        let plan = "## [x] M1: A\nnotes from user:   \n- old\nGoal: x\n\n## [x] M2: B\nGoal: y";
        let out = add_answer_note(plan, "M1", "new").unwrap();
        assert_eq!(
            out,
            "## [x] M1: A\nnotes from user:   \n- new\n- old\nGoal: x\n\n## [x] M2: B\nGoal: y"
        );
        // a section that ends the file without a trailing newline
        let out = add_answer_note("## [x] M1: A\nGoal: x", "M1", "n").unwrap();
        assert_eq!(out, "## [x] M1: A\nGoal: x\nNotes from user:\n- n\n");
        assert_eq!(add_answer_note(PLAN, "M9", "n"), None);
    }

    #[test]
    fn the_fix_section_and_numbering() {
        assert_eq!(next_number(&["M1".into(), "M2".into()], PLAN), 3);
        assert_eq!(next_number(&[], "## [x] M7: a\n"), 8);
        let src = FixSource {
            review: "yes".into(),
            likely_files: "a.rs".into(),
            tier: Some("medium".into()),
        };
        let (title, sec) = fix_section("M3", "M1", "tidy the   thing", &src);
        assert_eq!(title, "Fix for M1: tidy the   thing");
        assert_eq!(
            sec,
            "## [x] M3: Fix for M1: tidy the   thing\nDepends on: M1 (a fix to what M1 built)\nWhy separate: a change the developer asked for after M1 landed.\nReview: yes\nLikely files: a.rs\nTier: medium\nModel why: a fix to M1, for the coder who built it.\nGoal: tidy the   thing\nWhy: the developer asked for this after M1 landed.\nAcceptance criteria:\n- tidy the   thing\n- nothing else that M1 built changes, and the check command passes\nOpen questions:\n- none\n"
        );
        let long = "x".repeat(70);
        assert!(
            fix_section("M3", "M1", &long, &src)
                .0
                .ends_with(&format!("{}...", "x".repeat(60)))
        );
        assert_eq!(append_fix("a\n\n", "S\n"), "a\n\nS\n");
    }

    #[test]
    fn python_repr() {
        assert_eq!(py_repr("M9"), "'M9'");
        assert_eq!(py_repr("it's"), "\"it's\"");
    }

    use crate::lead_queue::tests::store_with_run;
    use crate::store::Store;

    fn plan_file(dir: &Path, text: &str) -> PathBuf {
        let p = dir.join("plan.md");
        std::fs::write(&p, text).unwrap();
        p
    }

    fn paused(s: &mut Store, id: &str) {
        s.transition(
            id,
            &TransitionRequest::auto(State::Paused, Some("question")),
        )
        .unwrap();
    }

    #[test]
    fn a_mixed_dispatch_changes_nothing() {
        let (_d, mut s) = store_with_run(true);
        paused(&mut s, "M1");
        s.transition(
            "M2",
            &TransitionRequest::manual(State::Skipped, Some("x"), false),
        )
        .unwrap();
        let e = s
            .dispatch_milestones(&["M1".into(), "M2".into()], None, None)
            .unwrap_err();
        assert_eq!(
            e.to_string(),
            "M2 is skipped — only pending, paused or failed milestones can be dispatched"
        );
        assert_eq!(s.get_task("M1").unwrap().status, State::Paused);
        assert_eq!(s.intent("M1").unwrap(), None);
        assert!(
            s.undispatched().unwrap().contains(&"M1".to_string())
                || s.get_task("M1").unwrap().status == State::Paused
        );
    }

    #[test]
    fn dispatch_records_the_intent_and_starts_paused_work_afresh() {
        let (_d, mut s) = store_with_run(true);
        paused(&mut s, "M1");
        s.conn
            .execute(
                "UPDATE tasks SET fix_rounds=2, review_rounds=1 WHERE id='M1'",
                [],
            )
            .unwrap();
        s.dispatch_milestones(&["M1".into()], Some("claude-1"), Some("high"))
            .unwrap();
        let t = s.get_task("M1").unwrap();
        assert_eq!(
            (t.status, t.fix_rounds, t.review_rounds),
            (State::Pending, 0, 0)
        );
        let i = s.intent("M1").unwrap().unwrap();
        assert_eq!(
            (i.go, i.assign.as_deref(), i.min_tier.as_deref()),
            (true, Some("claude-1"), Some("high"))
        );
        assert!(
            s.undispatched().unwrap().is_empty(),
            "M1 started, M2 waits for M1"
        );
        assert_eq!(
            s.history("M1").unwrap().last().unwrap().why,
            "dispatched again by the lead"
        );
        // the refusals
        let e = |r: Result<(), EditError>| r.unwrap_err().to_string();
        assert_eq!(
            e(s.dispatch_milestones(&["M1".into()], Some("nobody"), None)),
            "unknown worker 'nobody' — workers: claude-1"
        );
        assert_eq!(
            e(s.dispatch_milestones(&["M1".into()], None, Some("huge"))),
            "--tier must be small, medium or high"
        );
        assert_eq!(
            e(s.dispatch_milestones(&["M9".into()], None, None)),
            "unknown task 'M9' — known: M1, M2"
        );
    }

    #[test]
    fn answer_is_transactional_and_emits_for_the_lead() {
        let (d, mut s) = store_with_run(true);
        let plan = plan_file(d.path(), "## [x] M1: One\nGoal: x\n");
        let e = s.answer_milestone(&plan, "M1", "yes").unwrap_err();
        assert_eq!(e.to_string(), "M1 is pending, not paused");
        paused(&mut s, "M1");
        s.assign_worker("M1", Some("claude-1"), None).unwrap();
        s.conn
            .execute(
                "UPDATE tasks SET last_worker='claude-1', worker=NULL WHERE id='M1'",
                [],
            )
            .unwrap();
        // the plan has no section for M2: refused, nothing stored, no temp file left
        paused(&mut s, "M2");
        let before = std::fs::read_to_string(&plan).unwrap();
        let e = s.answer_milestone(&plan, "M2", "yes").unwrap_err();
        assert_eq!(e.to_string(), "M2 not found in plan.md");
        assert_eq!(s.get_task("M2").unwrap().status, State::Paused);
        assert_eq!(std::fs::read_to_string(&plan).unwrap(), before);
        let names: Vec<String> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|n| n == "plan.md" || n == ".orchestrate"),
            "{names:?}"
        );
        // a missing plan file is an error and changes nothing
        let gone = d.path().join("nope.md");
        assert!(matches!(
            s.answer_milestone(&gone, "M1", "yes"),
            Err(EditError::Io(_))
        ));
        assert_eq!(s.get_task("M1").unwrap().status, State::Paused);

        let done = s.answer_milestone(&plan, "M1", "use sqlite").unwrap();
        assert_eq!(done.last_worker.as_deref(), Some("claude-1"));
        assert_eq!(
            std::fs::read_to_string(&plan).unwrap(),
            "## [x] M1: One\nGoal: x\nNotes from user:\n- use sqlite\n"
        );
        let t = s.get_task("M1").unwrap();
        assert_eq!(
            (t.status, t.reason.as_str()),
            (State::Pending, "answered: use sqlite")
        );
        assert_eq!(
            s.intent("M1").unwrap().unwrap().assign.as_deref(),
            Some("claude-1")
        );
        let q = s.unread_lead_events().unwrap();
        assert!(
            q.iter()
                .any(|e| e.kind == LeadKind::AnswerProvided && e.text == "use sqlite")
        );
    }

    #[test]
    fn fix_stores_the_next_milestone_with_its_plan_fields() {
        let (d, mut s) = store_with_run(true);
        let plan = plan_file(d.path(), PLAN);
        let e = s.fix_milestone(&plan, "M1", "tidy", None).unwrap_err();
        assert_eq!(
            e.to_string(),
            "M1 is pending, not landed yet — `orch fix` is for work that already landed"
        );
        s.transition(
            "M1",
            &TransitionRequest::manual(State::Merged, Some("landed"), true),
        )
        .unwrap();
        s.assign_worker("M1", Some("claude-1"), None).unwrap();
        let f = s.fix_milestone(&plan, "M1", "tidy", None).unwrap();
        assert_eq!(
            (f.id.as_str(), f.who.as_deref(), f.review),
            ("M3", Some("claude-1"), false)
        );
        let t = s.get_task("M3").unwrap();
        assert_eq!(
            (t.status, t.deps.as_slice()),
            (State::Pending, &["M1".to_string()][..])
        );
        assert!(s.intent("M3").unwrap().unwrap().go);
        assert_eq!(
            s.intent("M3").unwrap().unwrap().assign.as_deref(),
            Some("claude-1")
        );
        let body = std::fs::read_to_string(&plan).unwrap();
        assert!(
            body.starts_with(PLAN.trim_end_matches('\n'))
                && body.contains("## [x] M3: Fix for M1: tidy\nDepends on: M1"),
            "{body}"
        );
        let e = s
            .fix_milestone(&plan, "M1", "x", Some("nobody"))
            .unwrap_err();
        assert_eq!(e.to_string(), "unknown worker 'nobody' — workers: claude-1");
    }

    #[test]
    fn escalate_needs_a_stronger_coder_and_works_once() {
        let (_d, mut s) = store_with_run(true);
        assert_eq!(
            s.escalate_milestone("M1", "").unwrap(),
            None,
            "no coder yet"
        );
        s.conn
            .execute(
                "UPDATE run_agents SET tier='medium' WHERE name='claude-1'",
                [],
            )
            .unwrap();
        s.conn.execute("INSERT INTO run_agents (role, name, kind, tier, position) VALUES ('worker','claude-2','claude','high',9)", []).unwrap();
        s.assign_worker("M1", Some("claude-1"), None).unwrap();
        s.transition(
            "M1",
            &TransitionRequest::auto(State::Failed, Some("rounds")),
        )
        .unwrap();
        assert_eq!(
            s.escalate_milestone("M1", "out of rounds").unwrap(),
            Some("high")
        );
        let t = s.get_task("M1").unwrap();
        assert_eq!((t.status, t.escalated), (State::Pending, true));
        assert_eq!(
            s.intent("M1").unwrap().unwrap().min_tier.as_deref(),
            Some("high")
        );
        let q = s.lead_entries().unwrap();
        assert_eq!(q.last().unwrap().kind, LeadKind::Escalated);
        assert_eq!(
            q.last().unwrap().text,
            "escalated to a high-tier model: out of rounds"
        );
        assert_eq!(s.escalate_milestone("M1", "").unwrap(), None, "only once");
        // a merged milestone cannot move: nothing changes
        s.conn
            .execute(
                "UPDATE tasks SET escalated=0, status='merged' WHERE id='M1'",
                [],
            )
            .unwrap();
        s.conn.execute("DELETE FROM dispatch_intent", []).unwrap();
        s.conn
            .execute("UPDATE tasks SET last_worker='claude-1' WHERE id='M1'", [])
            .unwrap();
        assert!(s.escalate_milestone("M1", "").is_err());
        assert!(!s.get_task("M1").unwrap().escalated);
    }
}
