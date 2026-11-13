//! The task contract (§8): `task.md` / `task.json` generated from a milestone. Created empty
//! by the plan-model milestone so the task-contract milestone fills it in without touching
//! the module list or a manifest.
//!
//! Everything here is a faithful rendering of Python v4.10.1's `task_contract`, `field` and
//! `items` (`plan_sections` included): the plan stays the human-edited source and the
//! contract is derived from it, so a value's shape — including the per-line reading of list
//! fields and the numbering of acceptance criteria — is Python's, byte for byte.

use crate::plan::{Milestone, Section};
use serde::Serialize;
use std::collections::BTreeMap;

/// The shape of a coder's result file (Python's `RESULT_FORMAT`), handed to every coder
/// inside `task.json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResultFormat {
    pub status: &'static str,
    pub commit: &'static str,
    pub files_changed: Vec<&'static str>,
    pub check: ResultCheck,
    pub criteria: Vec<ResultCriterion>,
    pub questions: Vec<&'static str>,
    pub usage: ResultUsage,
}

/// The `check` entry of [`ResultFormat`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResultCheck {
    pub ran: bool,
    pub passed: bool,
    pub summary: &'static str,
}

/// One entry of [`ResultFormat`]'s `criteria`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResultCriterion {
    pub id: &'static str,
    pub met: bool,
    pub evidence: &'static str,
}

/// The `usage` entry of [`ResultFormat`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResultUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cost_usd: Option<f64>,
}

/// Python's `RESULT_FORMAT` object, fresh each call.
pub fn result_format() -> ResultFormat {
    ResultFormat {
        status: "done | question | blocked",
        commit: "feat(api): add the categories endpoint",
        files_changed: vec!["src/routes/categories.ts", "test/categories.test.ts"],
        check: ResultCheck {
            ran: true,
            passed: true,
            summary: "lint ok, 42 tests pass",
        },
        criteria: vec![ResultCriterion {
            id: "AC1",
            met: true,
            evidence: "test 'rejects a missing amount column' passes",
        }],
        questions: vec!["only when status is question: what you need decided, with 2-3 options"],
        usage: ResultUsage {
            input_tokens: None,
            output_tokens: None,
            cost_usd: None,
        },
    }
}

/// One numbered acceptance criterion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Criterion {
    pub id: String,
    pub text: String,
}

/// One entry of `depends_on`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dependency {
    pub task: String,
    pub title: String,
    /// The merge commit found by `git log --grep="^Milestone: <task>$"`; `None` until the
    /// dependency has landed (Phase 7 commits it).
    pub commit: Option<String>,
}

/// Where an earlier attempt left its work, when there is one. `by` is the last worker's
/// name, `None` when the run does not know who it was (Python writes `null` and says
/// "another coder" in task.md).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviousWork {
    pub by: Option<String>,
}

/// `task.json`: the milestone as data (Python's `task_contract`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskContract {
    pub schema: u8,
    pub task_id: String,
    pub title: String,
    pub kind: &'static str,
    pub goal: String,
    pub why: String,
    pub acceptance_criteria: Vec<Criterion>,
    pub files_allowed: Vec<String>,
    pub scope: String,
    pub tier: Option<String>,
    pub needs: Vec<String>,
    pub interfaces: Vec<String>,
    pub constraints: Vec<String>,
    pub out_of_scope: Vec<String>,
    pub depends_on: Vec<Dependency>,
    pub check: String,
    pub attempt: u32,
    pub lost_attempts: u32,
    pub previous_work: Option<PreviousWork>,
    pub result_format: ResultFormat,
}

/// What [`build`] needs: the milestone from the parsed plan, the run values the state
/// carries (scope, check command, counters, dependencies) and the field values Python
/// derives from the plan.
pub struct ContractInput<'a> {
    pub task_id: &'a str,
    /// The milestone from the plan (Python reads it back out of `plan.md`).
    pub milestone: &'a Milestone,
    /// The run scope (plan header `Scope:`, default `warn`).
    pub scope: &'a str,
    /// The check command (plan header `Check command:`).
    pub check: &'a str,
    /// The task's started attempts so far (the store's `tasks` table); the contract names
    /// the attempt it starts, so Python's `attempt + 1` is applied here.
    pub attempt: u32,
    pub lost_attempts: u32,
    /// The coder of the last attempt, when its work is still parked on a hidden ref. `None`
    /// means no parked work; an empty string means parked work with no known coder (Python
    /// writes `by: null` and says "another coder" in task.md).
    pub previous_work: Option<&'a str>,
    /// `(task id, title, commit)` per dependency, in plan order.
    pub deps: Vec<(String, String, Option<String>)>,
}

/// Python's `field(t, key)`: the section's non-empty lines, trimmed, joined by one space.
pub fn field_text(section: &Section) -> String {
    section
        .lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Python's `items(key)` over a field's lines: every non-empty line is one value, its
/// leading `-`/space bullet stripped; empty values and `none` drop out. (A bullet's
/// continuation lines are values of their own, as in Python.)
pub fn items<'a>(lines: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    lines
        .into_iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.trim_start_matches(['-', ' ']).trim().to_string())
        .filter(|v| !v.is_empty() && v.to_lowercase() != "none")
        .collect()
}

/// The milestone's acceptance criteria as Python's `items("acceptance criteria")` reads
/// them: one value per physical line, nested bullets included.
pub fn acceptance_criteria(m: &Milestone) -> Vec<Criterion> {
    let lines = m
        .acceptance_criteria
        .iter()
        .flat_map(|item| item.lines());
    items(lines)
        .into_iter()
        .enumerate()
        .map(|(i, text)| Criterion {
            id: format!("AC{}", i + 1),
            text,
        })
        .collect()
}

/// Python's `plan_sections`: the milestone sections of plan.md keyed by task id (the text
/// from the heading line to the next `## ` heading, trimmed) and the Global instructions
/// text (without its heading). Later sections with the same id win, as in Python.
pub fn plan_sections(text: &str) -> (BTreeMap<String, String>, String) {
    let mut secs = BTreeMap::new();
    let mut glob = String::new();
    let mut cur: Option<String> = None;
    let mut buf: Vec<&str> = Vec::new();

    for line in text.lines() {
        let head = head_id(line);
        if head.is_some() || line.starts_with("## ") {
            match cur.as_deref() {
                Some("__global__") => glob = buf.join("\n").trim().to_string(),
                Some(id) => {
                    secs.insert(id.to_string(), buf.join("\n").trim().to_string());
                }
                None => {}
            }
            buf = vec![line];
            cur = Some(match head {
                Some(id) => id,
                None if line.to_lowercase().starts_with("## global") => "__global__".to_string(),
                None => String::new(),
            });
            if cur.as_deref() == Some("__global__") {
                buf = Vec::new();
            }
            continue;
        }
        buf.push(line);
    }
    match cur.as_deref() {
        Some("__global__") => glob = buf.join("\n").trim().to_string(),
        Some(id) if !id.is_empty() => {
            secs.insert(id.to_string(), buf.join("\n").trim().to_string());
        }
        _ => {}
    }
    (secs, glob)
}

/// Python's `HEAD_RE`: `## [ |x|X] Mn: title` — the task id of a milestone heading.
fn head_id(line: &str) -> Option<String> {
    let rest = line.strip_prefix("## [")?;
    let mut it = rest.chars();
    match it.next()? {
        ' ' | 'x' | 'X' => {}
        _ => return None,
    }
    let rest = it.as_str().strip_prefix(']')?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('M')?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let id = format!("M{}", &rest[..digits]);
    let rest = rest[digits..].trim_start();
    let rest = rest.strip_prefix(':')?;
    if rest.trim().is_empty() {
        return None;
    }
    Some(id)
}

/// Build the contract (Python's `task_contract`).
pub fn build(input: ContractInput<'_>) -> TaskContract {
    let m = input.milestone;
    TaskContract {
        schema: 1,
        task_id: input.task_id.to_string(),
        title: m.title.clone(),
        kind: "task",
        goal: field_text(&m.goal),
        why: field_text(&m.why),
        acceptance_criteria: acceptance_criteria(m),
        files_allowed: {
            let mut files = m.likely_file_list();
            files.sort();
            files.dedup();
            files
        },
        scope: input.scope.to_string(),
        tier: tier(m.tier.as_deref()),
        needs: needs(m.needs.as_deref()),
        interfaces: items(m.interfaces.lines.iter().map(String::as_str)),
        constraints: items(m.constraints.lines.iter().map(String::as_str)),
        out_of_scope: items(m.out_of_scope.lines.iter().map(String::as_str)),
        depends_on: input
            .deps
            .into_iter()
            .map(|(task, title, commit)| Dependency {
                task,
                title,
                commit,
            })
            .collect(),
        check: input.check.to_string(),
        attempt: input.attempt + 1,
        lost_attempts: input.lost_attempts,
        previous_work: input
            .previous_work
            .map(|by| PreviousWork {
                by: (!by.is_empty()).then(|| by.to_string()),
            }),
        result_format: result_format(),
    }
}

/// The run's `tier` value for a milestone (Python's sync normalisation): the first word,
/// lower-cased; `any` or nothing is no tier.
fn tier(raw: Option<&str>) -> Option<String> {
    let first = raw?
        .split_whitespace()
        .next()?
        .to_lowercase();
    (first != "any").then_some(first)
}

/// The run's `needs` for a milestone: lower-cased, split on commas and spaces.
fn needs(raw: Option<&str>) -> Vec<String> {
    let raw = raw.unwrap_or_default().to_lowercase();
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|n| !n.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::parse_plan;

    const PLAN: &str = "\
# Plan: Sample Feature
Branch: feature/sample
Check command: cargo test
Scope: warn

## Global instructions
- Be tidy.
  - Indented note.
- Never break the build.

## [x] M1: Foundation
Depends on: none
Why separate: the base others build on
Prefer: any
Tier: small
Needs: coding
Model why: mechanical work on a known pattern
Review: no
Likely files: src/a.rs, `src/b/`
Goal:
- Do the first thing well.
- Keep it small.
Why: it is the base the rest builds on.
Acceptance criteria:
- the first thing works
  - in every case
- the second thing is covered
Constraints:
- stay in src/
Interfaces:
- none
Context:
- the code lives in src/
Out of scope:
- nothing else
Open questions:
- none
Notes from user:

## [x] M2: Second
Depends on: M1 (needs the types)
Why separate: it can run only once the base has landed
Prefer: any
Tier: medium
Needs: coding, tool-use
Model why: normal coding with tool use
Review: yes
Likely files: src/c.rs
Goal:
- Do the second thing.
Why: it completes the feature.
Acceptance criteria:
- the second thing works
Constraints:
- stay in src/
Interfaces:
- Uses M1's types unchanged.
Context:
- the types are in src/
Out of scope:
- nothing
Open questions:
- none
Notes from user:
";

    fn m1() -> Milestone {
        parse_plan(PLAN).unwrap().milestones[0].clone()
    }

    // AC2-AC7: the contract carries Python's fields with Python's values, including the
    // per-line reading of the list fields and the AC numbering across nested bullets.
    #[test]
    fn the_contract_matches_python_shape_and_values() {
        let m = m1();
        let c = build(ContractInput {
            task_id: "M1",
            milestone: &m,
            scope: "warn",
            check: "cargo test",
            attempt: 1,
            lost_attempts: 0,
            previous_work: None,
            deps: vec![],
        });
        assert_eq!(c.schema, 1);
        assert_eq!((c.task_id.as_str(), c.title.as_str(), c.kind), ("M1", "Foundation", "task"));
        assert_eq!(c.goal, "- Do the first thing well. - Keep it small.");
        assert_eq!(c.why, "it is the base the rest builds on.");
        assert_eq!(
            c.acceptance_criteria,
            vec![
                Criterion { id: "AC1".into(), text: "the first thing works".into() },
                Criterion { id: "AC2".into(), text: "in every case".into() },
                Criterion { id: "AC3".into(), text: "the second thing is covered".into() },
            ]
        );
        // `src/b/` keeps its slash: Python's sync stores `f.strip().strip("`")`, and the
        // production archive shows trailing slashes (`crates/orch-herdr/src/bin/`).
        assert_eq!(c.files_allowed, ["src/a.rs", "src/b/"]);
        assert_eq!(c.scope, "warn");
        assert_eq!(c.tier.as_deref(), Some("small"));
        assert_eq!(c.needs, ["coding"]);
        assert!(c.interfaces.is_empty(), "`none` drops out");
        assert_eq!(c.constraints, ["stay in src/"]);
        assert_eq!(c.out_of_scope, ["nothing else"]);
        assert!(c.depends_on.is_empty());
        assert_eq!(c.check, "cargo test");
        assert_eq!(c.attempt, 2, "the contract names the attempt it starts");
        assert_eq!(c.lost_attempts, 0);
        assert_eq!(c.previous_work, None);
        assert_eq!(c.result_format, result_format());
    }

    // AC7: result_format is Python's RESULT_FORMAT object.
    #[test]
    fn result_format_serialises_like_python() {
        let json = serde_json::to_string_pretty(&result_format()).unwrap();
        let want = r#"{
  "status": "done | question | blocked",
  "commit": "feat(api): add the categories endpoint",
  "files_changed": [
    "src/routes/categories.ts",
    "test/categories.test.ts"
  ],
  "check": {
    "ran": true,
    "passed": true,
    "summary": "lint ok, 42 tests pass"
  },
  "criteria": [
    {
      "id": "AC1",
      "met": true,
      "evidence": "test 'rejects a missing amount column' passes"
    }
  ],
  "questions": [
    "only when status is question: what you need decided, with 2-3 options"
  ],
  "usage": {
    "input_tokens": null,
    "output_tokens": null,
    "cost_usd": null
  }
}"#;
        assert_eq!(json, want);
    }

    #[test]
    fn a_milestone_with_no_list_fields_gets_empty_lists() {
        let p = parse_plan("# Plan: x\nBranch: b\n## [x] M1: Bare\n").unwrap();
        let c = build(ContractInput {
            task_id: "M1",
            milestone: &p.milestones[0],
            scope: "warn",
            check: "",
            attempt: 0,
            lost_attempts: 0,
            previous_work: None,
            deps: vec![],
        });
        assert_eq!(c.goal, "");
        assert!(c.acceptance_criteria.is_empty());
        assert_eq!(c.files_allowed, Vec::<String>::new());
        assert_eq!(c.tier, None);
        assert_eq!(c.needs, Vec::<String>::new());
        assert_eq!(c.check, "");
        assert_eq!(c.attempt, 1);
    }

    #[test]
    fn tier_and_needs_are_normalised_like_sync() {
        let mut m = m1();
        m.tier = Some("MEDIUM".into());
        m.needs = Some("Coding,  tool-use".into());
        let c = build(ContractInput {
            task_id: "M1",
            milestone: &m,
            scope: "warn",
            check: "",
            attempt: 0,
            lost_attempts: 0,
            previous_work: None,
            deps: vec![],
        });
        assert_eq!(c.tier.as_deref(), Some("medium"));
        assert_eq!(c.needs, ["coding", "tool-use"]);
        m.tier = Some("any".into());
        let c = build(ContractInput {
            task_id: "M1",
            milestone: &m,
            scope: "warn",
            check: "",
            attempt: 0,
            lost_attempts: 0,
            previous_work: None,
            deps: vec![],
        });
        assert_eq!(c.tier, None);
    }

    #[test]
    fn depends_on_keeps_a_missing_commit_as_none() {
        let m = parse_plan(PLAN).unwrap().milestones[1].clone();
        let c = build(ContractInput {
            task_id: "M2",
            milestone: &m,
            scope: "warn",
            check: "cargo test",
            attempt: 2,
            lost_attempts: 1,
            previous_work: Some("claude-1"),
            deps: vec![("M1".into(), "Foundation".into(), None)],
        });
        assert_eq!(
            c.depends_on,
            vec![Dependency { task: "M1".into(), title: "Foundation".into(), commit: None }]
        );
        assert_eq!(c.attempt, 3);
        assert_eq!(c.lost_attempts, 1);
        assert_eq!(
            c.previous_work,
            Some(PreviousWork { by: Some("claude-1".into()) })
        );
    }

    #[test]
    fn parked_work_without_a_known_coder_says_another_coder() {
        let m = m1();
        let c = build(ContractInput {
            task_id: "M1",
            milestone: &m,
            scope: "warn",
            check: "cargo test",
            attempt: 0,
            lost_attempts: 0,
            previous_work: Some(""),
            deps: vec![],
        });
        assert_eq!(c.previous_work, Some(PreviousWork { by: None }));
        let json = serde_json::to_string(&c.previous_work).unwrap();
        assert_eq!(json, r#"{"by":null}"#);
    }

    // plan_sections: each milestone's section text runs from its heading to the next `## `
    // heading; the Global instructions text excludes its heading.
    #[test]
    fn plan_sections_split_on_every_heading() {
        let (secs, glob) = plan_sections(PLAN);
        let m1 = &secs["M1"];
        assert!(m1.starts_with("## [x] M1: Foundation\n"));
        assert!(m1.trim_end().ends_with("Notes from user:"), "{m1}");
        assert!(!m1.contains("## [x] M2"));
        assert!(glob.starts_with("- Be tidy."));
        assert!(glob.contains("  - Indented note."));
        assert!(glob.ends_with("- Never break the build."));
        let m2 = &secs["M2"];
        assert!(m2.starts_with("## [x] M2: Second\nDepends on: M1 (needs the types)\n"));
        assert!(m2.trim_end().ends_with("Notes from user:"));
    }

    #[test]
    fn the_heading_line_replacement_is_literal() {
        let (secs, _) = plan_sections(PLAN);
        assert!(secs["M1"].replacen("## [x] M1:", "Title:", 1).starts_with("Title: Foundation"));
        // A differently-spaced heading is left as Python's replace leaves it.
        let text = "Branch: b\n## [x]  M1: spaced\nGoal:\n- x\n";
        let (secs, _) = plan_sections(text);
        assert!(!secs["M1"].replacen("## [x] M1:", "Title:", 1).starts_with("Title:"));
    }

    #[test]
    fn sections_run_to_the_next_heading_of_any_kind() {
        let text = "Branch: b\n## [x] M1: A\n- a\n## Random\n- r\n## [x] M2: B\n- b\n";
        let (secs, _) = plan_sections(text);
        let m1 = &secs["M1"];
        assert!(m1.contains("- a") && !m1.contains("- b") && !m1.contains("Random"), "{m1}");
        assert!(secs["M2"].contains("- b"));
    }

    #[test]
    fn a_duplicated_id_keeps_the_last_section() {
        let text = "Branch: b\n## [x] M1: First\n- a\n## [x] M1: Second\n- b\n";
        let (secs, _) = plan_sections(text);
        assert!(secs["M1"].contains("## [x] M1: Second"), "{:#?}", secs);
    }
}
