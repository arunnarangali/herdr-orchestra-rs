//! The review pipeline's contract (§14, Python `cmd_review`, `review_findings`): what the
//! reviewer is told, what it writes back, and how that file is read. The prompt (writer side)
//! and the parser (reader side) live here together; this module's schema is the only agreement
//! between them.
//!
//! # `review-N.json`
//!
//! Written by the reviewer to `.orchestrate/review/<Mn>/review-<N>.json`:
//!
//! ```json
//! {
//!   "outcome": "approved | approved-with-warnings | changes-requested | blocked",
//!   "criteria":    [{"id": "AC1", "met": true, "note": "covered by test_upload"}],
//!   "blockers":    [{"file": "src/upload.ts", "line": 42, "problem": "what is wrong",
//!                    "why": "why it matters", "criterion": "AC1"}],
//!   "suggestions": [{"file": "src/a.ts", "line": 7, "problem": "optional improvement", "why": "…"}]
//! }
//! ```
//!
//! * `outcome`, `criteria`, `blockers` and `suggestions` are required. `criteria` holds one
//!   verdict (`id`, `met`, optional `note`) per acceptance criterion; `blockers` and `suggestions`
//!   are lists of findings (`file` and `problem` required; `line`, `why`, `criterion` optional).
//! * `approved` and `approved-with-warnings` need every criterion met and no blockers;
//!   `changes-requested` and `blocked` need at least one blocker or unmet criterion. A file that
//!   contradicts itself is unusable, never guessed at.
//! * An unmet criterion counts as a blocker.
//! * Problems only: a finding never carries a fix.
//!
//! A file that is missing, empty, not JSON or breaks any rule above is *unusable*; the reason is
//! a line [`read_round`] returns. The reviewer must not edit files: [`write_snapshot`] records the
//! main folder's state when a round is sent and [`changed_since`] finds what moved by the verdict.

use crate::error::{Error, Result};
use crate::git;
use crate::paths::ORCH_DIR;
use crate::store::Store;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The reviewer's overall verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Approved,
    ApprovedWithWarnings,
    ChangesRequested,
    Blocked,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Approved => "approved",
            Outcome::ApprovedWithWarnings => "approved-with-warnings",
            Outcome::ChangesRequested => "changes-requested",
            Outcome::Blocked => "blocked",
        }
    }

    fn parse(s: &str) -> Option<Outcome> {
        [Outcome::Approved, Outcome::ApprovedWithWarnings, Outcome::ChangesRequested, Outcome::Blocked]
            .into_iter()
            .find(|o| o.as_str() == s)
    }

    /// Whether the milestone may land.
    pub fn approves(self) -> bool {
        matches!(self, Outcome::Approved | Outcome::ApprovedWithWarnings)
    }
}

/// One problem the reviewer found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub file: String,
    pub line: Option<String>,
    pub problem: String,
    pub why: String,
    pub criterion: Option<String>,
}

impl Finding {
    /// `file:line — what — why (criterion)`, as Python words a finding.
    pub fn text(&self) -> String {
        let mut s = format!(
            "{}:{} — {} — {}",
            self.file,
            self.line.as_deref().unwrap_or("?"),
            self.problem,
            self.why
        );
        if let Some(c) = &self.criterion {
            s.push_str(&format!(" ({c})"));
        }
        s
    }
}

/// The reviewer's verdict on one acceptance criterion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriterionVerdict {
    pub id: String,
    pub met: bool,
    pub note: String,
}

/// A parsed, schema-valid `review-N.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub outcome: Outcome,
    pub criteria: Vec<CriterionVerdict>,
    pub blockers: Vec<Finding>,
    pub suggestions: Vec<Finding>,
}

impl Review {
    /// Blockers plus unmet criteria: what the coder must fix.
    pub fn blocker_count(&self) -> usize {
        self.blockers.len() + self.criteria.iter().filter(|c| !c.met).count()
    }

    /// `BLOCKER: …` lines: the findings, then each unmet criterion.
    pub fn blocker_lines(&self) -> Vec<String> {
        self.blockers
            .iter()
            .map(|f| format!("BLOCKER: {}", f.text()))
            .chain(self.criteria.iter().filter(|c| !c.met).map(|c| format!("BLOCKER: {} not met — {}", c.id, c.note)))
            .collect()
    }

    /// `SUGGESTION: …` lines.
    pub fn suggestion_lines(&self) -> Vec<String> {
        self.suggestions.iter().map(|f| format!("SUGGESTION: {}", f.text())).collect()
    }
}

fn text_field(v: &Value, key: &str, what: &str, required: bool) -> std::result::Result<String, String> {
    match v.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        Some(Value::String(_)) | None | Some(Value::Null) if !required => Ok(String::new()),
        Some(Value::String(_)) | None | Some(Value::Null) => Err(format!("{what} needs a non-empty `{key}`")),
        Some(_) => Err(format!("{what}: `{key}` must be text")),
    }
}

fn findings(v: &Value, key: &str) -> std::result::Result<Vec<Finding>, String> {
    let Some(items) = v.get(key).and_then(Value::as_array) else {
        return Err(format!("`{key}` must be a list"));
    };
    items
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let what = format!("{key}[{i}]");
            if !f.is_object() {
                return Err(format!("{what} must be an object"));
            }
            let line = match f.get("line") {
                None | Some(Value::Null) => None,
                Some(Value::Number(n)) => Some(n.to_string()),
                Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
                Some(_) => return Err(format!("{what}: `line` must be a number")),
            };
            let criterion = text_field(f, "criterion", &what, false)?;
            Ok(Finding {
                file: text_field(f, "file", &what, true)?,
                line,
                problem: text_field(f, "problem", &what, true)?,
                why: text_field(f, "why", &what, false)?,
                criterion: (!criterion.is_empty()).then_some(criterion),
            })
        })
        .collect()
}

/// Parse and validate the text of a `review-N.json`. `Err` is the reason it is unusable.
pub fn parse(text: &str) -> std::result::Result<Review, String> {
    if text.trim().is_empty() {
        return Err("the file is empty".into());
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not valid JSON ({e})"))?;
    if !v.is_object() {
        return Err("must be a JSON object".into());
    }
    let outcome = v
        .get("outcome")
        .and_then(Value::as_str)
        .and_then(Outcome::parse)
        .ok_or("`outcome` must be approved, approved-with-warnings, changes-requested or blocked")?;
    let Some(crit) = v.get("criteria").and_then(Value::as_array) else {
        return Err("`criteria` must be a list".into());
    };
    let criteria = crit
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let what = format!("criteria[{i}]");
            if !c.is_object() {
                return Err(format!("{what} must be an object"));
            }
            let Some(met) = c.get("met").and_then(Value::as_bool) else {
                return Err(format!("{what}: `met` must be true or false"));
            };
            Ok(CriterionVerdict {
                id: text_field(c, "id", &what, true)?,
                met,
                note: text_field(c, "note", &what, false)?,
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let r = Review {
        outcome,
        criteria,
        blockers: findings(&v, "blockers")?,
        suggestions: findings(&v, "suggestions")?,
    };
    if outcome.approves() && r.blocker_count() > 0 {
        return Err(format!("says {} but lists blockers or unmet criteria", outcome.as_str()));
    }
    if !outcome.approves() && r.blocker_count() == 0 {
        return Err(format!("says {} but lists no blocker and no unmet criterion", outcome.as_str()));
    }
    Ok(r)
}

/// `.orchestrate/review/<task>/` under `orch_dir`.
pub fn review_dir(orch_dir: &Path, task: &str) -> PathBuf {
    orch_dir.join("review").join(task)
}

/// Read round `round`'s file. `Err` is one line saying why the file is unusable.
pub fn read_round(orch_dir: &Path, task: &str, round: u32) -> std::result::Result<Review, String> {
    let name = format!("review-{round}.json");
    let path = review_dir(orch_dir, task).join(&name);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("review output not usable: {name} is missing (.orchestrate/review/{task}/{name})")
        } else {
            format!("review output not usable: {name} could not be read: {e}")
        }
    })?;
    parse(&text).map_err(|why| format!("review output not usable: {name} {why}"))
}

/// The acceptance criteria ids in the review folder's copy of the milestone's `task.json`; none
/// when the copy is missing or unreadable (the reviewer is then not held to a list).
pub fn contract_criteria(orch_dir: &Path, task: &str) -> Vec<String> {
    std::fs::read(review_dir(orch_dir, task).join("task.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| {
            v.get("acceptance_criteria").and_then(Value::as_array).map(|a| {
                a.iter().filter_map(|c| c.get("id").and_then(Value::as_str).map(String::from)).collect()
            })
        })
        .unwrap_or_default()
}

/// Acceptance criteria ids with no verdict in the review (Python's reviewer must "say for EVERY
/// acceptance criterion whether it is met").
pub fn unjudged<'a>(review: &Review, wanted: &'a [String]) -> Vec<&'a str> {
    wanted
        .iter()
        .filter(|id| !review.criteria.iter().any(|c| c.id == **id))
        .map(String::as_str)
        .collect()
}

// ---------------------------------------------------------------- the prompt

/// The review prompt file's body for round `round`; `rel` is `.orchestrate/review/<Mn>`,
/// `done_file` the file the reviewer creates last. `resolution` adds the conflict-resolution
/// patch note.
pub fn prompt(task: &str, title: &str, round: u32, done_file: &str, resolution: bool) -> String {
    let rel = format!(".orchestrate/review/{task}");
    let extra = if resolution {
        format!(
            "This round reviews a MERGE-CONFLICT RESOLUTION: you approved this milestone before, then it \
             conflicted with work that landed meanwhile. What the coder changed to resolve it: \
             {rel}/resolution.patch (the whole milestone is still in diff.patch).\n\n"
        )
    } else {
        String::new()
    };
    format!(
        "# Review {task}: {title} (round {round})\n\n\
         Goal and acceptance criteria: {rel}/task.md (criteria ids AC1, AC2… in {rel}/task.json)\n\
         Changes (against the merge-base): {rel}/diff.patch\n\n{extra}\
         Inspect the changes for functional issues against the acceptance criteria, architecture and \
         API/contract consistency, missing tests, and security. Read the repository as needed.\n\n\
         Rules:\n\
         - Do NOT modify, create or delete any file in the repository (only your review file below). \
         A review that changed the repository is thrown away.\n\
         - Report PROBLEMS, not solutions: the coder decides how to fix. Do not prescribe fixes.\n\
         - A blocker must be fixed before merge; write it as `BLOCKER: file:line — what — why`: in the JSON, \
         `file`, `line`, `problem` (what) and `why`. A suggestion is optional. At most 15 findings.\n\
         - Say for EVERY acceptance criterion whether it is met; an unmet criterion counts as a blocker.\n\n\
         Write your review to {rel}/review-{round}.json in exactly this shape (`outcome` is one of \
         approved, approved-with-warnings, changes-requested, blocked; `approved*` means no blockers and \
         every criterion met; `changes-requested` or `blocked` needs at least one blocker or unmet \
         criterion):\n\n```json\n{SCHEMA_EXAMPLE}\n```\n\n\
         Then create the empty file {done_file} as your very last action.\n"
    )
}

/// The example shown in the prompt: the schema above.
pub const SCHEMA_EXAMPLE: &str = r#"{
  "outcome": "approved | approved-with-warnings | changes-requested | blocked",
  "criteria": [{"id": "AC1", "met": true, "note": "covered by test_upload_rejects_missing_amount"}],
  "blockers": [{"file": "src/upload.ts", "line": 42, "problem": "what is wrong", "why": "why it matters", "criterion": "AC1"}],
  "suggestions": [{"file": "src/upload.ts", "line": 10, "problem": "optional improvement", "why": "why"}]
}"#;

/// The line a reviewer-prompt delivery carries (Python `rtext`).
pub fn reviewer_message(task: &str, round: u32) -> String {
    format!("Read .orchestrate/review/{task}/review-task-{round}.md and do exactly what it says.")
}

// ---------------------------------------------------------------- the diff

fn gerr(dir: &Path, args: &[&str], out: &git::GitOutput) -> String {
    git::failed(dir, args, out).to_string()
}

/// `git diff <merge-base of branch and src> <src>` in `root`: what a milestone's saved work
/// changes. `Err` is a line saying what could not be done.
pub fn diff_of_ref(root: &Path, branch: &str, src: &str) -> std::result::Result<String, String> {
    let args = ["merge-base", branch, src];
    let mb = git::run(root, &args).map_err(|e| e.to_string())?;
    if !mb.success() {
        return Err(gerr(root, &args, &mb));
    }
    let args = ["diff", mb.out().trim(), src];
    let d = git::run(root, &args).map_err(|e| e.to_string())?;
    if d.success() { Ok(d.stdout) } else { Err(gerr(root, &args, &d)) }
}

/// The working tree of `dir` (new files included) against its merge-base with `base`, through a
/// temporary index so the real index and any merge in progress are untouched (Python
/// `snapshot_diff`). For a milestone whose work is still in a folder and has no saved ref.
pub fn snapshot_diff(dir: &Path, base: &str) -> std::result::Result<String, String> {
    let mb = git::run(dir, &["merge-base", base, "HEAD"]).map_err(|e| e.to_string())?;
    let base = if mb.success() && !mb.out().trim().is_empty() { mb.out().trim().to_string() } else { base.to_string() };
    let idx = git::run(dir, &["rev-parse", "--path-format=absolute", "--git-path", "index"]).map_err(|e| e.to_string())?;
    let real = PathBuf::from(idx.out().trim());
    let tmp = real.with_file_name("orch-snapshot-index");
    if real.exists() {
        std::fs::copy(&real, &tmp).map_err(|e| format!("copying the index: {e}"))?;
    }
    let env = [("GIT_INDEX_FILE", tmp.to_str().unwrap_or_default())];
    let result = git::run_env(dir, &["add", "-A"], &env)
        .and_then(|_| git::run_env(dir, &["diff", "--cached", &base], &env))
        .map_err(|e| e.to_string())
        .and_then(|d| if d.success() { Ok(d.stdout) } else { Err(d.text()) });
    let _ = std::fs::remove_file(&tmp);
    result
}

// ---------------------------------------------------------------- the reviewer must not edit

/// What the main folder looks like right now: its `git status` entries with the content hash of
/// each file that exists. Landing a milestone only moves `HEAD`, so a clean tree stays an empty
/// snapshot; a reviewer that edits, adds or deletes a file changes it.
pub fn folder_snapshot(root: &Path) -> Result<Vec<String>> {
    // raw stdout: `check` trims, and a porcelain line may start with a space
    let args = ["status", "--porcelain", "--untracked-files=all"];
    let st = git::run(root, &args)?;
    if !st.success() {
        return Err(git::failed(root, &args, &st));
    }
    let mut out = Vec::new();
    for line in st.stdout.lines().filter(|l| l.len() > 3) {
        let path = line[3..].trim_matches('"');
        if path.starts_with(".orchestrate/") || path.starts_with(".orch-task/") {
            continue; // orch's own files, git-excluded in a real run
        }
        let hash = if root.join(path).is_file() {
            git::run(root, &["hash-object", "--", path])?.out().trim().to_string()
        } else {
            "-".to_string()
        };
        out.push(format!("{path}\t{}\t{hash}", &line[..2]));
    }
    out.sort();
    Ok(out)
}

fn snapshot_path(orch_dir: &Path, task: &str, round: u32) -> PathBuf {
    review_dir(orch_dir, task).join(format!("folder-{round}.snapshot"))
}

/// Record the main folder's state for round `round`, before the reviewer is prompted.
pub fn write_snapshot(root: &Path, task: &str, round: u32) -> Result<()> {
    let orch = root.join(ORCH_DIR);
    let lines = folder_snapshot(root)?;
    let path = snapshot_path(&orch, task, round);
    std::fs::create_dir_all(path.parent().unwrap_or(&orch)).map_err(|e| Error::io("create review dir", e))?;
    std::fs::write(&path, lines.join("\n")).map_err(|e| Error::io("write folder snapshot", e))
}

/// The files that differ between the snapshot taken when round `round` was sent and now. Empty
/// when the reviewer left the folder alone, or when no snapshot exists (nothing to compare).
pub fn changed_since(root: &Path, task: &str, round: u32) -> Result<Vec<String>> {
    let Ok(then) = std::fs::read_to_string(snapshot_path(&root.join(ORCH_DIR), task, round)) else {
        return Ok(Vec::new());
    };
    let then: BTreeSet<String> = then.lines().map(String::from).collect();
    let now: BTreeSet<String> = folder_snapshot(root)?.into_iter().collect();
    let files: BTreeSet<String> = then
        .symmetric_difference(&now)
        .filter_map(|l| l.split('\t').next().map(String::from))
        .collect();
    Ok(files.into_iter().collect())
}

// ---------------------------------------------------------------- suggestions kept

fn suggestions_path(orch_dir: &Path, task: &str) -> PathBuf {
    review_dir(orch_dir, task).join("suggestions.json")
}

/// Keep round `round`'s suggestions on the milestone (replacing that round's earlier entry), for
/// the run summary.
pub fn store_suggestions(orch_dir: &Path, task: &str, round: u32, lines: &[String]) -> Result<()> {
    let path = suggestions_path(orch_dir, task);
    let mut all: serde_json::Map<String, Value> = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    all.insert(round.to_string(), lines.iter().cloned().map(Value::String).collect());
    std::fs::create_dir_all(path.parent().unwrap_or(orch_dir)).map_err(|e| Error::io("create review dir", e))?;
    std::fs::write(&path, serde_json::to_vec_pretty(&all).unwrap_or_default())
        .map_err(|e| Error::io("write suggestions.json", e))
}

/// The summary's `## Reviewer suggestions not applied` lines: `- Mn: SUGGESTION: …`, milestones
/// in the order given, rounds in order.
pub fn suggestions_section(orch_dir: &Path, tasks: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for t in tasks {
        let Some(all) = std::fs::read(suggestions_path(orch_dir, t))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Map<String, Value>>(&b).ok())
        else {
            continue;
        };
        let mut rounds: Vec<(u32, &Value)> = all.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v))).collect();
        rounds.sort_by_key(|(k, _)| *k);
        for (_, v) in rounds {
            out.extend(v.as_array().into_iter().flatten().filter_map(Value::as_str).map(|s| format!("- {t}: {s}")));
        }
    }
    out
}

impl Store {
    /// Take back one review round (the reviewer vanished, or the prompt never arrived): the same
    /// round is asked again. Never below zero.
    pub fn drop_review_round(&mut self, task: &str) -> crate::store::Res<()> {
        self.conn.execute("UPDATE tasks SET review_rounds=MAX(0, review_rounds-1) WHERE id=?1", [task])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"outcome":"approved-with-warnings",
        "criteria":[{"id":"AC1","met":true,"note":"n"}],
        "blockers":[],
        "suggestions":[{"file":"a.rs","line":3,"problem":"rename","why":"clarity"}]}"#;

    fn with(outcome: &str, blockers: &str, met: bool) -> String {
        format!(r#"{{"outcome":"{outcome}","criteria":[{{"id":"AC1","met":{met}}}],"blockers":[{blockers}],"suggestions":[]}}"#)
    }

    const B1: &str = r#"{"file":"a.rs","line":3,"problem":"bad","why":"breaks","criterion":"AC1"}"#;

    #[test]
    fn a_valid_file_parses_each_outcome() {
        let r = parse(GOOD).unwrap();
        assert_eq!(r.outcome, Outcome::ApprovedWithWarnings);
        assert_eq!(r.suggestion_lines(), vec!["SUGGESTION: a.rs:3 — rename — clarity"]);
        assert_eq!(parse(&with("approved", "", true)).unwrap().outcome, Outcome::Approved);
        let c = parse(&with("changes-requested", B1, true)).unwrap();
        assert_eq!((c.outcome, c.blocker_count()), (Outcome::ChangesRequested, 1));
        assert_eq!(c.blocker_lines(), vec!["BLOCKER: a.rs:3 — bad — breaks (AC1)"]);
        assert_eq!(parse(&with("blocked", "", false)).unwrap().blocker_lines(), vec!["BLOCKER: AC1 not met — "]);
    }

    #[test]
    fn unusable_files_say_why() {
        for (text, why) in [
            ("", "empty"),
            ("   \n", "empty"),
            ("not json", "not valid JSON"),
            ("[1]", "JSON object"),
            (r#"{"outcome":"fine","criteria":[],"blockers":[],"suggestions":[]}"#, "`outcome`"),
            (r#"{"outcome":"approved","blockers":[],"suggestions":[]}"#, "`criteria`"),
            (r#"{"outcome":"approved","criteria":[{"id":"AC1"}],"blockers":[],"suggestions":[]}"#, "`met`"),
            (r#"{"outcome":"approved","criteria":[],"suggestions":[]}"#, "`blockers`"),
            (r#"{"outcome":"approved","criteria":[],"blockers":[{"file":"a"}],"suggestions":[]}"#, "`problem`"),
            (r#"{"outcome":"approved","criteria":[],"blockers":[],"suggestions":[{"file":"a","problem":"p","line":{}}]}"#, "`line`"),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(why), "{text:?}: {e}");
        }
        // self-contradicting files
        assert!(parse(&with("approved", B1, true)).unwrap_err().contains("lists blockers"));
        assert!(parse(&with("approved", "", false)).unwrap_err().contains("lists blockers or unmet"));
        assert!(parse(&with("changes-requested", "", true)).unwrap_err().contains("no blocker"));
        assert!(parse(&with("blocked", "", true)).unwrap_err().contains("no blocker"));
    }

    #[test]
    fn read_round_reports_a_missing_file_and_names_it() {
        let d = tempfile::tempdir().unwrap();
        let e = read_round(d.path(), "M1", 1).unwrap_err();
        assert!(e.contains("review-1.json is missing"), "{e}");
        let rd = review_dir(d.path(), "M1");
        std::fs::create_dir_all(&rd).unwrap();
        std::fs::write(rd.join("review-1.json"), "").unwrap();
        assert!(read_round(d.path(), "M1", 1).unwrap_err().contains("is empty"));
        std::fs::write(rd.join("review-1.json"), GOOD).unwrap();
        assert!(read_round(d.path(), "M1", 1).is_ok());
    }

    #[test]
    fn unjudged_criteria_are_found() {
        let r = parse(GOOD).unwrap();
        let wanted = vec!["AC1".to_string(), "AC2".to_string()];
        assert_eq!(unjudged(&r, &wanted), vec!["AC2"]);
    }

    #[test]
    fn the_prompt_carries_the_rules_and_the_paths() {
        let p = prompt("M1", "Thing", 2, ".orch-task/M1/done-3", false);
        for want in [
            "round 2", ".orchestrate/review/M1/diff.patch", ".orchestrate/review/M1/review-2.json",
            "BLOCKER: file:line — what — why", "Do NOT modify", "not solutions", "Do not prescribe fixes",
            "security", "missing tests", "architecture", "functional", ".orch-task/M1/done-3",
        ] {
            assert!(p.contains(want), "{want}\n{p}");
        }
        assert!(!p.contains("resolution.patch"));
        assert!(prompt("M1", "T", 1, "d", true).contains("resolution.patch"));
        // the example in the prompt is itself a shape the parser understands (modulo placeholders)
        assert!(SCHEMA_EXAMPLE.contains("\"outcome\"") && SCHEMA_EXAMPLE.contains("\"suggestions\""));
    }

    #[test]
    fn suggestions_are_kept_per_round_and_listed_for_the_summary() {
        let d = tempfile::tempdir().unwrap();
        store_suggestions(d.path(), "M1", 1, &["SUGGESTION: a".into()]).unwrap();
        store_suggestions(d.path(), "M1", 2, &["SUGGESTION: b".into()]).unwrap();
        store_suggestions(d.path(), "M1", 1, &["SUGGESTION: a2".into()]).unwrap(); // re-read: replaced
        let s = suggestions_section(d.path(), &["M1".into(), "M2".into()]);
        assert_eq!(s, vec!["- M1: SUGGESTION: a2", "- M1: SUGGESTION: b"]);
    }

    #[test]
    fn a_reviewer_that_edits_the_folder_is_noticed_but_a_landing_is_not() {
        let (_t, root) = git::testutil::repo();
        std::fs::create_dir_all(root.join(".orchestrate")).unwrap();
        write_snapshot(&root, "M1", 1).unwrap();
        assert!(changed_since(&root, "M1", 1).unwrap().is_empty());
        // a commit landing on the branch meanwhile moves HEAD only
        std::fs::write(root.join("landed.txt"), "x").unwrap();
        git::testutil::g(&root, &["add", "landed.txt"]);
        git::testutil::g(&root, &["commit", "-q", "-m", "landed"]);
        assert!(changed_since(&root, "M1", 1).unwrap().is_empty());
        // the reviewer edits a tracked file and adds one
        std::fs::write(root.join("src/a.txt"), "edited\n").unwrap();
        std::fs::write(root.join("new.txt"), "n").unwrap();
        assert_eq!(changed_since(&root, "M1", 1).unwrap(), vec!["new.txt", "src/a.txt"]);
        // no snapshot, nothing to compare
        assert!(changed_since(&root, "M1", 9).unwrap().is_empty());
    }

    #[test]
    fn the_diff_is_against_the_merge_base_and_includes_new_files_from_a_folder() {
        let (_t, root) = git::testutil::repo();
        let g = |a: &[&str]| git::testutil::g(&root, a);
        g(&["switch", "-q", "-c", "work"]);
        std::fs::write(root.join("mine.txt"), "mine\n").unwrap();
        g(&["add", "."]);
        g(&["commit", "-q", "-m", "mine"]);
        let src = g(&["rev-parse", "HEAD"]);
        g(&["switch", "-q", "main"]);
        std::fs::write(root.join("theirs.txt"), "t\n").unwrap();
        g(&["add", "."]);
        g(&["commit", "-q", "-m", "theirs"]);
        let d = diff_of_ref(&root, "main", &src).unwrap();
        assert!(d.contains("mine.txt") && !d.contains("theirs.txt"), "{d}");
        // a folder that still holds its work: a new untracked file shows up, the index is untouched
        g(&["switch", "-q", "work"]);
        std::fs::write(root.join("loose.txt"), "loose\n").unwrap();
        let d = snapshot_diff(&root, "main").unwrap();
        assert!(d.contains("loose.txt") && d.contains("mine.txt") && !d.contains("theirs.txt"), "{d}");
        assert!(g(&["status", "--porcelain"]).contains("?? loose.txt"), "the real index is untouched");
    }

    #[test]
    fn a_review_round_can_be_taken_back_but_not_below_zero() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open_in(&d.path().join(".orchestrate")).unwrap();
        s.add_task(&crate::store::NewTask { id: "M1".into(), title: "t".into(), deps: vec![] }).unwrap();
        s.bump("M1", crate::store::Counter::Review).unwrap();
        s.drop_review_round("M1").unwrap();
        s.drop_review_round("M1").unwrap();
        assert_eq!(s.get_task("M1").unwrap().review_rounds, 0);
    }
}
