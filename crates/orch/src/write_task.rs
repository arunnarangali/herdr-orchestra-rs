//! `orch write-task <Mn>`: generate the milestone's coder contract (`task.md` and
//! `task.json`), as Python v4.10.1's `cmd_write_task` and `task_contract`.
//!
//! Phase-5 notes (the pieces Python takes from `state.json` that this build does not have
//! yet): the milestone, its fields and its dependencies come from the parsed plan; the
//! attempt counters come from the store's `tasks` table when it has a row; the contract is
//! written into the main repository root's `.orch-task/<Mn>/` (shared-isolation layout,
//! until Phase 7 gives each milestone a worktree of its own).

use crate::listing::die;
use orch_core::error::{Error, EXIT_USAGE};
use orch_core::paths;
use orch_core::plan::{parse_plan, Milestone, Plan};
use orch_core::store::Store;
use orch_core::task_contract::{self, ContractInput};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The generic coder rules Python appends to every `task.md` (`WORKER_RULES`).
const WORKER_RULES: &str = "\
## Rules for you (the coder)
- You own the HOW: the approach, the design inside this milestone, the code, the tests and every fix. This file says what and why; it deliberately doesn't say how.
- Follow the repo's existing patterns (AGENTS.md / CLAUDE.md) and any Interfaces listed above exactly; other milestones rely on them.
- Change only what this task needs. No unrelated refactors.
- Do NOT commit, push, or switch branches. orch commits your work for you.
- Stay inside the files listed under Likely files (`files_allowed` in task.json) unless the task can't be done otherwise; say why in your report if you leave them. orch compares your diff with that list.
- Finish with a result file (the prompt names it) in the `result_format` shape from task.json: `status` done / question / blocked, `commit`, `files_changed`, the `check` you ran, one entry per acceptance criterion (`id`, `met`, `evidence`), and `usage` if your tool shows tokens or cost. orch reads it, so keep it valid JSON.
- The commit message (`commit` in the result, and the last report line `COMMIT: <commit message>`) says what changed, in the repo's commit style (for example `feat(api): add categories endpoint`). It becomes the commit message, so describe the change, not the milestone.
- If anything is ambiguous, do not guess: set status `question`, put the question in `questions` and on a report line starting `QUESTION:`, finish the files and the done file, and stop.
- Before finishing, run the check command below and record the result (pass/fail + the failing lines).
";

/// How many non-blank report lines of a dependency's reports go into the task.md context.
const REPORT_LINES: usize = 25;

/// Run `orch write-task <Mn>`; the exit code. Extra arguments are ignored, as in Python.
pub fn run(args: &[String]) -> i32 {
    if args.is_empty() {
        return die("usage: orch write-task <Mn>");
    }
    match generate(&args[0]) {
        Ok(paths) => {
            for p in paths {
                println!("WROTE {}", p.display());
            }
            0
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            e.exit_code()
        }
    }
}

/// A plain failure message with Python's `die` exit code.
#[derive(Debug)]
struct Msg(String);

impl std::fmt::Display for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Msg {}

/// The paths written, in Python's print order (task.md then task.json).
fn generate(tid: &str) -> Result<Vec<PathBuf>, Error> {
    let start = std::env::current_dir().map_err(|e| Error::io("reading the current folder", e))?;
    let root = paths::main_root(&start)?;
    let orch = paths::orchestrate_dir(&start)?;

    let plan_path = orch.join("plan.md");
    if !plan_path.exists() {
        return Err(Error::wrap(Msg("missing .orchestrate/plan.md".into()), EXIT_USAGE));
    }
    let text = std::fs::read_to_string(&plan_path)
        .map_err(|e| Error::io(format!("reading {}", plan_path.display()), e))?;
    let plan = parse_plan(&text)?;
    let (secs, glob) = task_contract::plan_sections(&text);

    // Python's `task(st, tid)` fires before the plan check, so a task the state knows but
    // the plan lost reads "not found in plan.md", anything else is "unknown task".
    let store = open_store(&orch)?;
    let store_ids: Vec<String> = match &store {
        Some(s) => s
            .list_tasks()
            .map_err(Error::from)?
            .into_iter()
            .map(|t| t.id)
            .collect(),
        None => Vec::new(),
    };
    let known: Vec<String> = if store_ids.is_empty() {
        plan.milestones.iter().map(|m| m.id.clone()).collect()
    } else {
        store_ids
    };
    if !known.iter().any(|id| id == tid) {
        // Python's `{tid!r}`: single quotes around the id.
        return Err(Error::wrap(
            Msg(format!("unknown task '{tid}' — known: {}", known.join_or_none())),
            EXIT_USAGE,
        ));
    }
    if plan.milestones.iter().all(|m| m.id != tid) {
        return Err(Error::wrap(Msg(format!("{tid} not found in plan.md")), EXIT_USAGE));
    }

    // Python reads the task's fields from the plan (`ptasks`): the first milestone with the id.
    let milestone = first(&plan, tid).expect("checked above");
    let row = store.as_ref().and_then(|s| s.get_task(tid).ok());
    let attempt = row.as_ref().map_or(0, |t| t.attempt);
    let lost = row.as_ref().map_or(0, |t| t.lost);
    let last_worker = row.as_ref().and_then(|t| t.last_worker.clone());

    let branch = branch(&plan);
    let mut deps = Vec::new();
    for dep in &milestone.depends_on {
        let title = first(&plan, &dep.id).map_or_else(|| dep.id.clone(), |m| m.title.clone());
        let commit = dependency_commit(&root, &branch, &dep.id);
        deps.push((dep.id.clone(), title, commit));
    }
    let has_parked_work = work_ref(&root, &branch, tid);

    let contract = task_contract::build(ContractInput {
        task_id: tid,
        milestone,
        scope: &scope(&plan),
        check: plan.header.check_command.as_deref().unwrap_or_default(),
        attempt,
        lost_attempts: lost,
        previous_work: has_parked_work.then_some(last_worker.as_deref()).flatten(),
        deps,
    });

    let mut parts = vec![
        format!("# Task {tid}: {}", milestone.title),
        String::new(),
        secs.get(tid)
            .map(|s| s.replacen(&format!("## [x] {tid}:"), "Title:", 1))
            .unwrap_or_default(),
        String::new(),
    ];
    let glob = glob.trim();
    if !glob.is_empty()
        && !matches!(glob.to_lowercase().as_str(), "none" | "<empty — user may fill in>")
    {
        parts.push("## Global instructions".to_string());
        parts.push(glob.to_string());
        parts.push(String::new());
    }
    let mut ctx = Vec::new();
    for dep in &milestone.depends_on {
        let title = first(&plan, &dep.id).map_or_else(|| dep.id.clone(), |m| m.title.clone());
        if let Some(report) = dependency_report(&root, &start, &dep.id) {
            ctx.push(format!("### {}: {} (already merged into your branch)", dep.id, title));
            ctx.push(report);
            ctx.push(String::new());
        }
    }
    if !ctx.is_empty() {
        parts.push("## Context: what the tasks you depend on built".to_string());
        parts.extend(ctx);
    }
    if !contract.acceptance_criteria.is_empty() {
        parts.push("## Acceptance criteria ids (for your result file)".to_string());
        parts.extend(
            contract
                .acceptance_criteria
                .iter()
                .map(|c| format!("- {}: {}", c.id, c.text)),
        );
        parts.push(String::new());
    }
    if let Some(by) = &contract.previous_work {
        let by = by.by.as_deref().unwrap_or("another coder");
        parts.push("## Earlier work".to_string());
        parts.push(format!(
            "An earlier attempt ({by}) left work in this folder. Look at it first and continue from it unless it is wrong."
        ));
        parts.push(String::new());
    }
    parts.push(format!("## Check command\n`{}`", contract.check));
    parts.push(String::new());
    parts.push(WORKER_RULES.to_string());

    let out = root.join(".orch-task").join(tid);
    std::fs::create_dir_all(&out)
        .map_err(|e| Error::io(format!("creating {}", out.display()), e))?;
    let md = out.join("task.md");
    std::fs::write(&md, parts.join("\n"))
        .map_err(|e| Error::io(format!("writing {}", md.display()), e))?;
    let js = out.join("task.json");
    std::fs::write(&js, to_python_json(&contract))
        .map_err(|e| Error::io(format!("writing {}", js.display()), e))?;
    Ok(vec![md, js])
}

/// The first milestone with this id, as Python's `next(x for x in ptasks ...)` reads the plan.
fn first<'a>(plan: &'a Plan, id: &str) -> Option<&'a Milestone> {
    plan.milestones.iter().find(|m| m.id == id)
}

/// The plan header's branch (the parser guarantees one).
fn branch(plan: &Plan) -> String {
    plan.header.branch.clone().unwrap_or_default()
}

/// The run scope: the header's first word, lower-cased, `warn` by default.
fn scope(plan: &Plan) -> String {
    let raw = plan.header.scope.as_deref().unwrap_or("warn");
    raw.split_whitespace().next().unwrap_or("warn").to_lowercase()
}

/// The store behind `.orchestrate/state.db`, when the run has one.
fn open_store(orch: &Path) -> Result<Option<Store>, Error> {
    let db = orch.join(orch_core::store::DB_FILE);
    if !db.exists() {
        return Ok(None);
    }
    Store::open(&db).map(Some).map_err(Error::from)
}

/// The dependency's merge commit: Python's
/// `git log -1 --format=%H --grep="^Milestone: <dep>$" <branch>`, `None` without a hit.
fn dependency_commit(root: &Path, branch: &str, dep: &str) -> Option<String> {
    git(
        root,
        &["log", "-1", "--format=%H", &format!("--grep=^Milestone: {dep}$"), branch],
    )
    .filter(|sha| !sha.is_empty())
}

/// Python's `work_ref`: is Mn's parked work on its hidden ref (or a pre-4.5 task branch)?
fn work_ref(root: &Path, branch: &str, tid: &str) -> bool {
    let slug = branch.replace('/', "-");
    for r in [format!("refs/orch/{slug}/{tid}"), format!("refs/heads/{branch}-{tid}")] {
        if git(root, &["rev-parse", "--verify", "--quiet", &format!("{r}^{{commit}}")]).is_some() {
            return true;
        }
    }
    false
}

/// The dependency's reports, as Python reads them: every non-blank line of `report-*.md`
/// under `.orch-task/<dep>/`, the first [`REPORT_LINES`]. Python reads them from the
/// dependency's own folder in the state; this build looks in the main root first, then in
/// the current folder.
fn dependency_report(root: &Path, start: &Path, dep: &str) -> Option<String> {
    let mut files = Vec::new();
    let mut dirs = vec![root.join(".orch-task").join(dep)];
    if start != root {
        dirs.push(start.join(".orch-task").join(dep));
    }
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.starts_with("report-"))
                    && p.extension().is_some_and(|x| x == "md")
            })
            .collect();
        found.sort_by_key(|p| report_number(p.as_path()));
        if found.is_empty() {
            continue;
        }
        files = found;
        break;
    }
    if files.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).ok()?;
        for l in text.lines().filter(|l| !l.trim().is_empty()) {
            lines.push(l.to_string());
        }
        if lines.len() >= REPORT_LINES {
            break;
        }
    }
    lines.truncate(REPORT_LINES);
    Some(lines.join("\n"))
}

/// Python's report order: the number in the file name (`report-12.md` before `report-3.md`).
fn report_number(p: &Path) -> u64 {
    p.file_stem()
        .and_then(|s| s.to_str())
        .map_or(0, |s| {
            s.chars()
                .filter(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(root).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Python's `json.dumps(indent=2)`: pretty JSON with non-ASCII escaped (`ensure_ascii`).
/// The emitter is hand-rolled (this crate cannot add dependencies) so the bytes match
/// Python's exactly — two-space indent, entries joined with a bare `,\n`.
fn to_python_json(c: &task_contract::TaskContract) -> String {
    contract_json(c).render(0)
}

/// The contract as Python's dict, with the same key order (Python dicts keep insertion
/// order, and the goldens pin it).
fn contract_json(c: &task_contract::TaskContract) -> PyJson {
    use task_contract::{Criterion, Dependency, PreviousWork};
    PyJson::obj([
        ("schema", PyJson::Int(i64::from(c.schema))),
        ("task_id", PyJson::str(&c.task_id)),
        ("title", PyJson::str(&c.title)),
        ("kind", PyJson::str(c.kind)),
        ("goal", PyJson::str(&c.goal)),
        ("why", PyJson::str(&c.why)),
        (
            "acceptance_criteria",
            PyJson::List(
                c.acceptance_criteria
                    .iter()
                    .map(|Criterion { id, text }| {
                        PyJson::obj([
                            ("id", PyJson::str(id)),
                            ("text", PyJson::str(text)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "files_allowed",
            PyJson::List(c.files_allowed.iter().map(PyJson::str).collect()),
        ),
        ("scope", PyJson::str(&c.scope)),
        (
            "tier",
            c.tier.as_deref().map_or(PyJson::Null, PyJson::str),
        ),
        (
            "needs",
            PyJson::List(c.needs.iter().map(PyJson::str).collect()),
        ),
        (
            "interfaces",
            PyJson::List(c.interfaces.iter().map(PyJson::str).collect()),
        ),
        (
            "constraints",
            PyJson::List(c.constraints.iter().map(PyJson::str).collect()),
        ),
        (
            "out_of_scope",
            PyJson::List(c.out_of_scope.iter().map(PyJson::str).collect()),
        ),
        (
            "depends_on",
            PyJson::List(
                c.depends_on
                    .iter()
                    .map(|Dependency { task, title, commit }| {
                        PyJson::obj([
                            ("task", PyJson::str(task)),
                            ("title", PyJson::str(title)),
                            (
                                "commit",
                                commit.as_deref().map_or(PyJson::Null, PyJson::str),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("check", PyJson::str(&c.check)),
        ("attempt", PyJson::Int(i64::from(c.attempt))),
        ("lost_attempts", PyJson::Int(i64::from(c.lost_attempts))),
        (
            "previous_work",
            match &c.previous_work {
                Some(PreviousWork { by }) => PyJson::obj([(
                    "by",
                    by.as_deref().map_or(PyJson::Null, PyJson::str),
                )]),
                None => PyJson::Null,
            },
        ),
        (
            "result_format",
            contract_json_result_format(&c.result_format),
        ),
    ])
}

/// [`task_contract::result_format`] as Python's dict.
fn contract_json_result_format(rf: &task_contract::ResultFormat) -> PyJson {
    use task_contract::ResultCriterion;
    PyJson::obj([
        ("status", PyJson::str(rf.status)),
        ("commit", PyJson::str(rf.commit)),
        (
            "files_changed",
            PyJson::List(rf.files_changed.iter().map(PyJson::str).collect()),
        ),
        (
            "check",
            PyJson::obj([
                ("ran", PyJson::Bool(rf.check.ran)),
                ("passed", PyJson::Bool(rf.check.passed)),
                ("summary", PyJson::str(rf.check.summary)),
            ]),
        ),
        (
            "criteria",
            PyJson::List(
                rf.criteria
                    .iter()
                    .map(|ResultCriterion { id, met, evidence }| {
                        PyJson::obj([
                            ("id", PyJson::str(id)),
                            ("met", PyJson::Bool(*met)),
                            ("evidence", PyJson::str(evidence)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "questions",
            PyJson::List(rf.questions.iter().map(PyJson::str).collect()),
        ),
        (
            "usage",
            PyJson::obj([
                (
                    "input_tokens",
                    rf.usage
                        .input_tokens
                        .map_or(PyJson::Null, |v| PyJson::Int(i64::from(v))),
                ),
                (
                    "output_tokens",
                    rf.usage
                        .output_tokens
                        .map_or(PyJson::Null, |v| PyJson::Int(i64::from(v))),
                ),
                (
                    "cost_usd",
                    rf.usage.cost_usd.map_or(PyJson::Null, PyJson::Float),
                ),
            ]),
        ),
    ])
}

/// A tiny JSON value, rendered exactly as Python's `json.dumps(indent=2, ensure_ascii=True)`.
#[derive(Debug)]
enum PyJson {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    List(Vec<PyJson>),
    Obj(Vec<(&'static str, PyJson)>),
}

impl PyJson {
    fn str(s: impl AsRef<str>) -> PyJson {
        PyJson::Str(s.as_ref().to_string())
    }

    fn obj<const N: usize>(entries: [(&'static str, PyJson); N]) -> PyJson {
        PyJson::Obj(Vec::from(entries))
    }

    fn render(&self, indent: usize) -> String {
        let pad = "  ".repeat(indent);
        let pad_in = "  ".repeat(indent + 1);
        match self {
            PyJson::Str(s) => py_json_string(s),
            PyJson::Int(n) => n.to_string(),
            // Python prints floats via repr; every float here is a plain decimal like 0.12.
            PyJson::Float(f) => format!("{f}"),
            PyJson::Bool(b) => b.to_string(),
            PyJson::Null => "null".to_string(),
            PyJson::List(items) if items.is_empty() => "[]".to_string(),
            PyJson::List(items) => {
                let inner: Vec<String> =
                    items.iter().map(|i| format!("{pad_in}{}", i.render(indent + 1))).collect();
                format!("[\n{}\n{pad}]", inner.join(",\n"))
            }
            PyJson::Obj(entries) if entries.is_empty() => "{}".to_string(),
            PyJson::Obj(entries) => {
                let inner: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{pad_in}\"{}\": {}", k, v.render(indent + 1)))
                    .collect();
                format!("{{\n{}\n{pad}}}", inner.join(",\n"))
            }
        }
    }
}

/// One JSON string, escaped as Python does (`ensure_ascii=True`): backslash, quote and
/// control characters first, then every non-ASCII character as `\uXXXX` (lower-case hex,
/// surrogate pairs above the BMP).
fn py_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) <= 0x7f => out.push(c),
            c => {
                let cp = c as u32;
                if cp <= 0xffff {
                    out.push_str(&format!("\\u{cp:04x}"));
                } else {
                    let cp = cp - 0x1_0000;
                    out.push_str(&format!("\\u{:04x}", 0xd800 + (cp >> 10)));
                    out.push_str(&format!("\\u{:04x}", 0xdc00 + (cp & 0x3ff)));
                }
            }
        }
    }
    out.push('"');
    out
}

/// Python's `', '.join(ids) or 'none'` for the known-tasks list.
trait JoinOrNone {
    fn join_or_none(&self) -> String;
}

impl JoinOrNone for Vec<String> {
    fn join_or_none(&self) -> String {
        if self.is_empty() {
            "none".to_string()
        } else {
            self.join(", ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_escape_like_python() {
        assert_eq!(
            py_json_string("café \u{1f600} x"),
            "\"caf\\u00e9 \\ud83d\\ude00 x\""
        );
        assert_eq!(py_json_string("q\"b\\n\t"), "\"q\\\"b\\\\n\\t\"");
    }

    #[test]
    fn json_pretty_prints_like_python() {
        let v = PyJson::obj([
            ("a", PyJson::List(vec![PyJson::Int(1), PyJson::Null])),
            ("b", PyJson::Obj(vec![])),
        ]);
        assert_eq!(
            v.render(0),
            "{\n  \"a\": [\n    1,\n    null\n  ],\n  \"b\": {}\n}"
        );
    }

    #[test]
    fn report_number_orders_by_the_digits() {
        let n = |s: &str| report_number(Path::new(s));
        assert!(n("report-3.md") < n("report-12.md"));
        assert_eq!(n("report.md"), 0);
    }

    #[test]
    fn known_tasks_join_like_python() {
        assert_eq!(Vec::<String>::new().join_or_none(), "none");
        assert_eq!(vec!["M1".to_string(), "M2".to_string()].join_or_none(), "M1, M2");
    }
}
