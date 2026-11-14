//! Fix jobs (§13, Python `fix_round`, `cmd_needs_fix`): the prompt a coder gets when its check
//! fails, when its own result says a criterion is not met, when it strayed outside its `Likely
//! files` under `Scope: strict`, or when the reviewer found blockers. A fix prompt says what is
//! wrong and never how to fix it — the coder owns that — and names the result file to write
//! next. Pure text: the event manager and `orch needs-fix` decide where it goes.

use std::path::Path;

/// Why a milestone goes back to its coder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixKind {
    /// The result file could not be read: write it again, in the same shape.
    ResultFile,
    /// The check command failed; `tail` is the last lines of its output.
    Check { tail: String },
    /// The coder's own result says these criteria (id, evidence) are not met.
    Criteria { unmet: Vec<(String, String)> },
    /// `Scope: strict`: these files are outside the milestone's `Likely files`.
    Scope { allowed: Vec<String>, files: Vec<String> },
    /// The reviewer's `BLOCKER:` lines of review round `round`.
    Blockers { round: u32, lines: Vec<String> },
}

/// What a fix prompt needs to know about the milestone.
#[derive(Debug, Clone, Copy)]
pub struct FixContext<'a> {
    pub task: &'a str,
    pub title: &'a str,
    /// The plan's check command.
    pub check: &'a str,
    /// The fix round (1-based).
    pub round: u32,
    /// The result file the coder writes when done, e.g. `result-2.json`.
    pub result_file: &'a str,
}

/// The fix prompt's body (the `fix-N.md` / `criteria-fix-N.md` / `scope-fix-N.md` /
/// `review-fix-N.md` file the coder is told to read).
pub fn body(kind: &FixKind, cx: &FixContext<'_>) -> String {
    let FixContext { task, title, check, round, result_file } = *cx;
    let dir = format!(".orch-task/{task}");
    let run = if check.trim().is_empty() {
        String::new()
    } else {
        format!(", run `{check}`")
    };
    let finish = format!(
        "When you are done, write {dir}/{result_file} (the `result_format` shape from {dir}/task.json) \
         and your short report, then create the done file as the last thing you do."
    );
    match kind {
        FixKind::ResultFile => format!(
            "# {task}: your result file was not usable\n\n\
             The file {dir}/{result_file} is missing or is not valid JSON in the `result_format` shape \
             from {dir}/task.json. Do not redo the work: write the file again, as valid JSON with \
             `status`, `commit`, `files_changed`, `check`, `criteria` (one entry per acceptance \
             criterion: `id`, `met`, `evidence`).\n\n{finish}\n"
        ),
        FixKind::Check { tail } => format!(
            "# Checks failing for {task} (round {round})\n\nThe check command `{check}` failed. \
             Diagnose the cause and fix it; you decide how. Don't change unrelated code, and don't \
             weaken or skip tests to make them pass.\n\n```\n{tail}\n```\n\n{finish}\n"
        ),
        FixKind::Criteria { unmet } => {
            let items: Vec<String> = unmet.iter().map(|(id, ev)| format!("- {id}: {ev}")).collect();
            format!(
                "# {task}: acceptance criteria not met yet (round {round})\n\n\
                 Your result says these are not met:\n{}\n\n\
                 Finish them (you decide how){run}, and write a new result.\n\n{finish}\n",
                items.join("\n")
            )
        }
        FixKind::Scope { allowed, files } => format!(
            "# {task} changed files outside its scope (round {round})\n\n\
             This milestone may change only: {}. It also changed: {}.\n\
             Undo those changes, or, if the task truly can't be done without them, leave them and \
             explain why in your report{run}.\n\n{finish}\n",
            allowed.join(", "),
            files.join(", ")
        ),
        FixKind::Blockers { round, lines } => format!(
            "# Review findings for {task}: {title} (round {round})\n\n\
             The reviewer found the problems below. You own the fix: investigate each one and decide \
             how to solve it. Keep every acceptance criterion in {dir}/task.md{run}.\n\
             If you think a finding is wrong, explain why in your report instead of changing code \
             for it.\n\n{}\n\n{finish}\n",
            lines.join("\n")
        ),
    }
}

/// The prompt-file prefix of a fix kind (`fix-2.md`, `criteria-fix-2.md`, …).
pub fn file_prefix(kind: &FixKind) -> &'static str {
    match kind {
        FixKind::ResultFile => "result-fix",
        FixKind::Check { .. } => "fix",
        FixKind::Criteria { .. } => "criteria-fix",
        FixKind::Scope { .. } => "scope-fix",
        FixKind::Blockers { .. } => "review-fix",
    }
}

/// `(blockers, suggestions, known)` of review round `round`, from
/// `.orchestrate/review/<task>/review-<round>.json`, else the `BLOCKER:` / `SUGGESTION:` lines
/// of `review-<round>.md`. `known` is false when neither file is usable (Python
/// `review_findings`). Blocker lines come back prefixed `BLOCKER: `.
pub fn review_findings(orch_dir: &Path, task: &str, round: u32) -> (Vec<String>, Vec<String>, bool) {
    if let Ok(r) = crate::review::read_round(orch_dir, task, round) {
        return (r.blocker_lines(), r.suggestion_lines(), true);
    }
    let dir = orch_dir.join("review").join(task);
    if let Some(v) = std::fs::read(dir.join(format!("review-{round}.json")))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        && v.is_object()
        && v.get("findings").is_none_or(|f| f.is_array())
    {
        let text = |f: &serde_json::Value, k: &str, d: &str| match f.get(k) {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Null) | None => d.to_string(),
            Some(x) => x.to_string(),
        };
        let (mut blockers, mut sugg) = (Vec::new(), Vec::new());
        for f in v.get("findings").and_then(|f| f.as_array()).into_iter().flatten() {
            if !f.is_object() {
                continue;
            }
            let mut line = format!(
                "{}:{} — {} — {}",
                text(f, "file", "?"),
                text(f, "line", "?"),
                text(f, "problem", ""),
                text(f, "why", "")
            );
            if let Some(c) = f.get("criterion").and_then(|c| c.as_str()).filter(|c| !c.is_empty()) {
                line.push_str(&format!(" ({c})"));
            }
            if text(f, "severity", "").to_lowercase().starts_with("block") {
                blockers.push(line);
            } else {
                sugg.push(line);
            }
        }
        for c in v.get("criteria").and_then(|c| c.as_array()).into_iter().flatten() {
            if c.get("met").and_then(|m| m.as_bool()) == Some(false) {
                blockers.push(format!("{} not met — {}", text(c, "id", "?"), text(c, "note", "")));
            }
        }
        return (
            blockers.into_iter().map(|b| format!("BLOCKER: {b}")).collect(),
            sugg.into_iter().map(|s| format!("SUGGESTION: {s}")).collect(),
            true,
        );
    }
    let md = std::fs::read_to_string(dir.join(format!("review-{round}.md"))).unwrap_or_default();
    let lines: Vec<&str> = md.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let starts = |l: &str, p: &str| l.to_uppercase().starts_with(p);
    let blockers: Vec<String> = lines.iter().filter(|l| starts(l, "BLOCKER")).map(|l| l.to_string()).collect();
    let sugg: Vec<String> = lines.iter().filter(|l| starts(l, "SUGGESTION")).map(|l| l.to_string()).collect();
    let known = !blockers.is_empty() || !sugg.is_empty() || lines.iter().any(|l| starts(l, "NO FINDINGS"));
    (blockers, sugg, known)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx(round: u32, result_file: &str) -> FixContext<'_> {
        FixContext { task: "M1", title: "Thing", check: "make test", round, result_file }
    }

    #[test]
    fn a_failed_check_shows_the_tail_and_names_the_next_result_file() {
        let b = body(&FixKind::Check { tail: "boom line".into() }, &cx(2, "result-3.json"));
        assert!(b.contains("round 2") && b.contains("`make test`") && b.contains("boom line"));
        assert!(b.contains(".orch-task/M1/result-3.json"), "{b}");
        assert!(b.contains("don't weaken"), "never says how, only what: {b}");
    }

    #[test]
    fn unmet_criteria_name_their_ids_and_evidence() {
        let kind = FixKind::Criteria { unmet: vec![("AC2".into(), "no test yet".into()), ("AC5".into(), "".into())] };
        let b = body(&kind, &cx(1, "result-2.json"));
        assert!(b.contains("- AC2: no test yet") && b.contains("- AC5: "), "{b}");
        assert!(b.contains("run `make test`") && b.contains("result-2.json"));
    }

    #[test]
    fn a_stray_prompt_lists_allowed_and_extra_files() {
        let kind = FixKind::Scope { allowed: vec!["src/".into()], files: vec!["Cargo.toml".into()] };
        let b = body(&kind, &cx(1, "result-2.json"));
        assert!(b.contains("may change only: src/") && b.contains("also changed: Cargo.toml"), "{b}");
    }

    #[test]
    fn blockers_are_listed_verbatim() {
        let kind = FixKind::Blockers { round: 1, lines: vec!["BLOCKER: a.rs:3 — bad — why".into()] };
        let b = body(&kind, &cx(1, "result-2.json"));
        assert!(b.contains("BLOCKER: a.rs:3 — bad — why") && b.contains("round 1") && b.contains("Thing"));
    }

    #[test]
    fn a_bad_result_file_asks_to_rewrite_the_same_one() {
        let b = body(&FixKind::ResultFile, &cx(1, "result-1.json"));
        assert!(b.contains("result-1.json") && b.contains("valid JSON"));
        assert_eq!(file_prefix(&FixKind::ResultFile), "result-fix");
    }

    #[test]
    fn findings_come_from_json_else_markdown() {
        let d = tempfile::tempdir().unwrap();
        let rd = d.path().join("review/M1");
        std::fs::create_dir_all(&rd).unwrap();
        std::fs::write(
            rd.join("review-1.json"),
            r#"{"findings":[{"severity":"blocker","file":"a.rs","line":3,"problem":"p","why":"w","criterion":"AC1"},
                {"severity":"suggestion","file":"b.rs","line":1,"problem":"q","why":"v"}],
               "criteria":[{"id":"AC2","met":false,"note":"n"},{"id":"AC3","met":true}]}"#,
        )
        .unwrap();
        let (b, s, known) = review_findings(d.path(), "M1", 1);
        assert!(known);
        assert_eq!(b, vec!["BLOCKER: a.rs:3 — p — w (AC1)", "BLOCKER: AC2 not met — n"]);
        assert_eq!(s, vec!["SUGGESTION: b.rs:1 — q — v"]);
        std::fs::write(rd.join("review-2.md"), "intro\nblocker: x is wrong\nSuggestion: y\n").unwrap();
        let (b, s, known) = review_findings(d.path(), "M1", 2);
        assert!(known && b == vec!["blocker: x is wrong"] && s == vec!["Suggestion: y"]);
        std::fs::write(rd.join("review-3.md"), "NO FINDINGS\n").unwrap();
        assert!(review_findings(d.path(), "M1", 3).2);
        assert!(!review_findings(d.path(), "M1", 4).2);
    }
}
