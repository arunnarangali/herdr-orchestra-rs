//! The typed model of `.orchestrate/plan.md` and its pure parser (§8, Phase 5).
//!
//! The parser mirrors Python v4.10.1's `parse_plan`: header `Key: value` lines before the
//! first `## ` heading, `## [x] Mn: title` milestone sections, and `Key: value` lines inside a
//! milestone that start a field whose following lines continue it. Unlike Python it keeps
//! everything: raw values for M2 to validate, the Global instructions and Feature breakdown
//! text, nested bullets and blank lines inside multi-line items. It does no validation
//! against the registry, catalog or herdr, and touches no files.

use crate::Error;

/// A parse failure. Always names the 1-based line it was found on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("plan line {line}: {message}")]
pub struct PlanError {
    pub line: usize,
    pub message: String,
}

impl From<PlanError> for Error {
    fn from(e: PlanError) -> Self {
        // Python's `die` exits 2.
        Error::wrap(e, 2)
    }
}

/// One agent from a `Workers:`, `Reviewer:` or `Lead:` line, e.g.
/// `claude-1 (claude: sonnet)` or `hermes-1 (hermes @pane:w1:p3)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub name: String,
    /// Lower-cased, as Python's `parse_agents`.
    pub kind: String,
    /// The model the pane was started with, lower-cased, when given.
    pub model: Option<String>,
    /// The herdr pane for `@pane:<id>`.
    pub pane: Option<String>,
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.' || c == '-'
}

/// Python's `AGENT_RE.findall`:
/// `name ( kind [: model] [@pane:id] )`, scanned anywhere in the text.
pub fn parse_agents(s: &str) -> Vec<Agent> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match agent_at(&chars, i) {
            Some((agent, end)) => {
                out.push(agent);
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

fn run(chars: &[char], mut i: usize, pred: impl Fn(char) -> bool) -> (String, usize) {
    let start = i;
    while i < chars.len() && pred(chars[i]) {
        i += 1;
    }
    (chars[start..i].iter().collect(), i)
}

fn skip_ws(chars: &[char], i: usize) -> usize {
    run(chars, i, char::is_whitespace).1
}

/// Try to match one agent starting exactly at `start`.
fn agent_at(chars: &[char], start: usize) -> Option<(Agent, usize)> {
    let (name, mut i) = run(chars, start, is_name_char);
    if name.is_empty() {
        return None;
    }
    i = skip_ws(chars, i);
    if chars.get(i) != Some(&'(') {
        return None;
    }
    i = skip_ws(chars, i + 1);
    let (kind, next) = run(chars, i, is_name_char);
    if kind.is_empty() {
        return None;
    }
    i = next;
    // Optional `: model`.
    let mut model = None;
    let j = skip_ws(chars, i);
    if chars.get(j) == Some(&':') {
        let k = skip_ws(chars, j + 1);
        let (m, end) = run(chars, k, |c| !c.is_whitespace() && c != '@' && c != ')');
        if !m.is_empty() {
            model = Some(m.to_lowercase());
            i = end;
        }
    }
    // Optional `@pane:id`, which needs whitespace before it.
    let mut pane = None;
    let j = skip_ws(chars, i);
    if j > i && chars[j..].starts_with(&['@', 'p', 'a', 'n', 'e', ':']) {
        let (p, end) = run(chars, j + 6, |c| !c.is_whitespace() && c != ')');
        if !p.is_empty() {
            pane = Some(p);
            i = end;
        }
    }
    i = skip_ws(chars, i);
    if chars.get(i) != Some(&')') {
        return None;
    }
    Some((
        Agent {
            name,
            kind: kind.to_lowercase(),
            model,
            pane,
        },
        i + 1,
    ))
}

/// The plan header: every raw value is kept exactly as Python reads it (continuation lines
/// joined by a space, trimmed). `None` means the line is absent; M2 validates the values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    pub branch: Option<String>,
    pub base: Option<String>,
    pub check_command: Option<String>,
    pub setup_command: Option<String>,
    pub isolation: Option<String>,
    pub max_parallel: Option<String>,
    pub scope: Option<String>,
    pub budget: Option<String>,
    pub commits: Option<String>,
    pub lead_raw: Option<String>,
    pub lead_mode: Option<String>,
    pub workers_raw: Option<String>,
    pub reviewer_raw: Option<String>,
    pub risk_hold: Option<String>,
    pub setup_inputs: Option<String>,
    /// `Lead:` parsed into agents.
    pub lead: Vec<Agent>,
    /// `Workers:` parsed into agents.
    pub workers: Vec<Agent>,
    /// `Reviewer:` parsed into agents.
    pub reviewer: Vec<Agent>,
    /// Any other header line, as (lower-cased key, raw value), in file order.
    pub extra: Vec<(String, String)>,
}

/// The lines of a free-text section, kept as written (indentation included, trailing blank
/// lines dropped).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Section {
    pub lines: Vec<String>,
}

impl Section {
    /// The section as one text.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The section as a list. A new item starts at each unindented `- ` / `* ` line; nested
    /// bullets, indented lines and plain continuation lines stay inside the item (with their
    /// indentation and interior blank lines), joined by `\n`. Text before the first bullet is
    /// one item.
    pub fn items(&self) -> Vec<String> {
        let mut items: Vec<Vec<String>> = Vec::new();
        let mut blanks = 0;
        for line in &self.lines {
            if line.trim().is_empty() {
                blanks += 1;
                continue;
            }
            let bullet = line
                .strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .or_else(|| (line == "-" || line == "*").then_some(""));
            match (bullet, items.last_mut()) {
                (Some(text), _) => items.push(vec![text.trim_end().to_string()]),
                (None, Some(cur)) => {
                    cur.extend(std::iter::repeat_n(String::new(), blanks));
                    cur.push(line.clone());
                }
                (None, None) => items.push(vec![line.trim().to_string()]),
            }
            blanks = 0;
        }
        items.into_iter().map(|i| i.join("\n")).collect()
    }
}

/// One `Depends on` entry: `M2 (needs the table)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub id: String,
    /// The parenthesised reason, when there is one.
    pub reason: Option<String>,
}

/// One `## [x] Mn: title` milestone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Milestone {
    pub id: String,
    pub title: String,
    /// `[x]` is enabled, `[ ]` is skipped.
    pub enabled: bool,
    /// Line of the heading.
    pub line: usize,
    /// The raw `Depends on` value (`None` when the line is absent).
    pub depends_on_raw: Option<String>,
    /// `Depends on`, each dependency with its reason; empty for `none`, `-`, `n/a`.
    pub depends_on: Vec<Dependency>,
    pub why_separate: Option<String>,
    pub prefer: Option<String>,
    pub tier: Option<String>,
    pub needs: Option<String>,
    pub model_why: Option<String>,
    pub review: Option<String>,
    pub likely_files: Option<String>,
    pub parallel: Option<String>,
    pub goal: Section,
    pub why: Section,
    /// The Acceptance criteria as a list (see [`Section::items`]).
    pub acceptance_criteria: Vec<String>,
    pub constraints: Section,
    pub interfaces: Section,
    pub context: Section,
    pub out_of_scope: Section,
    pub open_questions: Section,
    pub notes_from_user: Section,
    /// Any other field of the milestone, as (lower-cased key, section), in file order.
    pub extra: Vec<(String, Section)>,
}

impl Milestone {
    /// Python's `has_open_questions`: something other than `none` under Open questions.
    pub fn has_open_questions(&self) -> bool {
        let vals: Vec<String> = self
            .open_questions
            .lines
            .iter()
            .map(|v| v.trim_start_matches(['-', ' ']).trim().to_lowercase())
            .filter(|v| !v.is_empty())
            .collect();
        !vals.is_empty() && vals != ["none"]
    }

    /// `Likely files` split on commas and whitespace, backticks stripped.
    pub fn likely_file_list(&self) -> Vec<String> {
        self.likely_files
            .as_deref()
            .unwrap_or("")
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(|f| f.trim().trim_matches('`').to_string())
            .filter(|f| !f.is_empty())
            .collect()
    }

    /// `Parallel: allow...`.
    pub fn parallel_allowed(&self) -> bool {
        self.parallel
            .as_deref()
            .is_some_and(|p| p.to_lowercase().starts_with("allow"))
    }
}

/// A whole plan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// The `# Plan: <title>` line, without the `# ` and an optional `Plan:`.
    pub title: Option<String>,
    pub header: Header,
    pub global_instructions: Section,
    pub feature_breakdown: Section,
    pub milestones: Vec<Milestone>,
    /// `## ` sections other than the two above, as (heading, section), in file order.
    pub other_sections: Vec<(String, Section)>,
}

impl Plan {
    /// The milestone with this id (the last one, as Python's `by_id`).
    pub fn milestone(&self, id: &str) -> Option<&Milestone> {
        self.milestones.iter().rev().find(|m| m.id == id)
    }
}

/// Python's `HEAD_RE`: `## [ |x|X] Mn: title`. `None` when the line is not of that shape.
fn parse_heading(line: &str) -> Option<(bool, String, String)> {
    let rest = line.strip_prefix("## [")?;
    let mut it = rest.chars();
    let enabled = match it.next()? {
        ' ' => false,
        'x' | 'X' => true,
        _ => return None,
    };
    let rest = it.as_str().strip_prefix(']')?.trim_start();
    let rest = rest.strip_prefix('M')?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let (num, rest) = rest.split_at(digits);
    let rest = rest.trim_start().strip_prefix(':')?.trim();
    if rest.is_empty() {
        return None;
    }
    Some((enabled, format!("M{num}"), rest.to_string()))
}

/// A line that is trying to be a milestone heading.
fn looks_like_milestone(line: &str) -> bool {
    line.starts_with("## [")
        || line
            .strip_prefix("## M")
            .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
}

/// Python's `FIELD_RE` on a stripped line: `Key: value` with a capitalised, letters-and-spaces
/// key. Returns (lower-cased key, value).
fn parse_field(stripped: &str) -> Option<(String, String)> {
    let mut chars = stripped.char_indices();
    let (_, first) = chars.next()?;
    if !first.is_ascii_uppercase() {
        return None;
    }
    let mut end = first.len_utf8();
    for (i, c) in chars {
        if c.is_ascii_alphabetic() || c == ' ' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if end < 2 || !stripped[end..].starts_with(':') {
        return None;
    }
    Some((
        stripped[..end].trim().to_lowercase(),
        stripped[end + 1..].trim().to_string(),
    ))
}

/// Raw lines of one field while parsing; later replaced as a whole by a repeated key.
#[derive(Default)]
struct RawFields(Vec<(String, Vec<String>)>);

impl RawFields {
    /// Python: a repeated key replaces the earlier one (keeping its position).
    fn start(&mut self, key: String, first: Vec<String>) {
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = first,
            None => self.0.push((key, first)),
        }
    }

    fn last_mut(&mut self, key: &str) -> Option<&mut Vec<String>> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// Python's `field()`: the lines joined by a space.
fn scalar(lines: &[String]) -> String {
    lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn section(lines: &[String]) -> Section {
    let mut lines: Vec<String> = lines.to_vec();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    // The inline value after `Key:` may be empty and leave a blank first line.
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    Section { lines }
}

/// Python's `split_ids` over `re.sub(r"\([^)]*\)", "", raw)`, keeping each reason.
fn parse_dependencies(raw: &str) -> Vec<Dependency> {
    let chars: Vec<char> = raw.chars().collect();
    let mut outside = String::new();
    let mut reasons: Vec<(usize, String)> = Vec::new(); // (position in `outside`, reason)
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '('
            && let Some(close) = chars[i + 1..].iter().position(|&c| c == ')')
        {
            let reason: String = chars[i + 1..i + 1 + close].iter().collect();
            reasons.push((outside.chars().count(), reason.trim().to_string()));
            i += close + 2;
            continue;
        }
        outside.push(chars[i]);
        i += 1;
    }
    let trimmed = outside.trim();
    if trimmed.is_empty() || ["none", "-", "n/a"].contains(&trimmed.to_lowercase().as_str()) {
        return Vec::new();
    }
    let oc: Vec<char> = outside.chars().collect();
    let mut deps = Vec::new();
    let mut i = 0;
    while i < oc.len() {
        if oc[i] == 'M' && oc.get(i + 1).is_some_and(char::is_ascii_digit) {
            let mut j = i + 1;
            while j < oc.len() && oc[j].is_ascii_digit() {
                j += 1;
            }
            // A reason is the parenthesis right after the id (whitespace allowed).
            let reason = reasons
                .iter()
                .find(|(pos, _)| *pos >= j && oc[j..*pos].iter().all(|c| c.is_whitespace()))
                .map(|(_, r)| r.clone());
            deps.push(Dependency {
                id: oc[i..j].iter().collect(),
                reason,
            });
            i = j;
        } else {
            i += 1;
        }
    }
    deps
}

fn build_milestone(
    id: String,
    title: String,
    enabled: bool,
    line: usize,
    fields: RawFields,
) -> Milestone {
    let mut m = Milestone {
        id,
        title,
        enabled,
        line,
        ..Milestone::default()
    };
    for (key, lines) in fields.0 {
        match key.as_str() {
            "depends on" => {
                let raw = scalar(&lines);
                m.depends_on = parse_dependencies(&raw);
                m.depends_on_raw = Some(raw);
            }
            "why separate" => m.why_separate = Some(scalar(&lines)),
            "prefer" => m.prefer = Some(scalar(&lines)),
            "tier" => m.tier = Some(scalar(&lines)),
            "needs" => m.needs = Some(scalar(&lines)),
            "model why" => m.model_why = Some(scalar(&lines)),
            "review" => m.review = Some(scalar(&lines)),
            "likely files" => m.likely_files = Some(scalar(&lines)),
            "parallel" => m.parallel = Some(scalar(&lines)),
            "goal" => m.goal = section(&lines),
            "why" => m.why = section(&lines),
            "acceptance criteria" => m.acceptance_criteria = section(&lines).items(),
            "constraints" => m.constraints = section(&lines),
            "interfaces" => m.interfaces = section(&lines),
            "context" => m.context = section(&lines),
            "out of scope" => m.out_of_scope = section(&lines),
            "open questions" => m.open_questions = section(&lines),
            "notes from user" => m.notes_from_user = section(&lines),
            _ => m.extra.push((key, section(&lines))),
        }
    }
    m
}

fn build_header(fields: RawFields) -> Header {
    let mut h = Header::default();
    for (key, lines) in fields.0 {
        let raw = scalar(&lines);
        match key.as_str() {
            "branch" => h.branch = Some(raw),
            "base" => h.base = Some(raw),
            "check command" => h.check_command = Some(raw),
            "setup command" => h.setup_command = Some(raw),
            "isolation" => h.isolation = Some(raw),
            "max parallel" => h.max_parallel = Some(raw),
            "scope" => h.scope = Some(raw),
            "budget" => h.budget = Some(raw),
            "commits" => h.commits = Some(raw),
            "lead" => {
                h.lead = parse_agents(&raw);
                h.lead_raw = Some(raw);
            }
            "lead mode" => h.lead_mode = Some(raw),
            "workers" => {
                h.workers = parse_agents(&raw);
                h.workers_raw = Some(raw);
            }
            "reviewer" => {
                h.reviewer = parse_agents(&raw);
                h.reviewer_raw = Some(raw);
            }
            "risk hold" => h.risk_hold = Some(raw),
            "setup inputs" => h.setup_inputs = Some(raw),
            _ => h.extra.push((key, raw)),
        }
    }
    h
}

/// Where the parser is.
enum Ctx {
    Header,
    Milestone,
    Global,
    Feature,
    Other,
}

/// Parse plan text. Errors name the line; nothing here panics.
pub fn parse_plan(text: &str) -> Result<Plan, PlanError> {
    let mut plan = Plan::default();
    let mut ctx = Ctx::Header;
    let mut header_fields = RawFields::default();
    let mut header_end: Option<usize> = None;
    // The open milestone and the open field (key) in it or in the header.
    let mut cur: Option<(String, String, bool, usize, RawFields)> = None;
    let mut cur_key: Option<String> = None;
    let mut free: Vec<String> = Vec::new(); // lines of the open Global / Feature / other section
    let mut other_name = String::new();

    // Close the open free-text section into the plan.
    fn flush_free(plan: &mut Plan, ctx: &Ctx, free: &mut Vec<String>, name: &str) {
        let s = section(free);
        match ctx {
            Ctx::Global => plan.global_instructions = s,
            Ctx::Feature => plan.feature_breakdown = s,
            Ctx::Other => plan.other_sections.push((name.to_string(), s)),
            _ => {}
        }
        free.clear();
    }
    fn flush_milestone(
        plan: &mut Plan,
        cur: &mut Option<(String, String, bool, usize, RawFields)>,
    ) {
        if let Some((id, title, enabled, line, fields)) = cur.take() {
            plan.milestones
                .push(build_milestone(id, title, enabled, line, fields));
        }
    }

    for (idx, raw) in text.lines().enumerate() {
        let lineno = idx + 1;
        let line = raw.trim_end();

        if line.starts_with("## ") || looks_like_milestone(line) {
            if header_end.is_none() {
                header_end = Some(lineno);
            }
            flush_milestone(&mut plan, &mut cur);
            flush_free(&mut plan, &ctx, &mut free, &other_name);
            cur_key = None;
            if let Some((enabled, id, title)) = parse_heading(line) {
                cur = Some((id, title, enabled, lineno, RawFields::default()));
                ctx = Ctx::Milestone;
            } else if looks_like_milestone(line) {
                return Err(PlanError {
                    line: lineno,
                    message: format!(
                        "broken milestone heading {line:?}: expected `## [x] M<number>: <title>`"
                    ),
                });
            } else {
                let name = line[3..].trim().to_string();
                ctx = match name.to_lowercase().as_str() {
                    "global instructions" => Ctx::Global,
                    "feature breakdown" => Ctx::Feature,
                    _ => {
                        other_name = name;
                        Ctx::Other
                    }
                };
            }
            continue;
        }

        match ctx {
            Ctx::Global | Ctx::Feature | Ctx::Other => free.push(line.to_string()),
            Ctx::Header | Ctx::Milestone => {
                let is_header = matches!(ctx, Ctx::Header);
                if is_header
                    && plan.title.is_none()
                    && cur_key.is_none()
                    && let Some(t) = line.strip_prefix("# ")
                {
                    let t = t.trim();
                    plan.title = Some(t.strip_prefix("Plan:").unwrap_or(t).trim().to_string());
                    continue;
                }
                let field = if line.starts_with([' ', '\t', '-']) {
                    None
                } else {
                    parse_field(line.trim())
                };
                let fields = match cur.as_mut() {
                    Some(c) => &mut c.4,
                    None => &mut header_fields,
                };
                if let Some((key, val)) = field {
                    let first = if val.is_empty() { vec![] } else { vec![val] };
                    fields.start(key.clone(), first);
                    cur_key = Some(key);
                } else if let Some(key) = &cur_key
                    && let Some(lines) = fields.last_mut(key)
                {
                    // Header continuations are trimmed as Python does; milestone ones
                    // keep their indentation. Blank lines are kept (trailing ones go).
                    lines.push(if is_header {
                        line.trim().to_string()
                    } else {
                        line.to_string()
                    });
                }
            }
        }
    }
    flush_milestone(&mut plan, &mut cur);
    flush_free(&mut plan, &ctx, &mut free, &other_name);

    plan.header = build_header(header_fields);
    if plan.header.branch.as_deref().is_none_or(str::is_empty) {
        return Err(PlanError {
            line: header_end.unwrap_or_else(|| text.lines().count().max(1)),
            message: "plan header needs `Branch: feature/<slug>`".into(),
        });
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: &str = "# Plan: Demo\nBranch: feature/demo\n\n";

    fn parse(body: &str) -> Plan {
        parse_plan(&format!("{MIN}{body}")).unwrap()
    }

    #[test]
    fn title_and_header_raw_values() {
        let p = parse_plan(
            "# Plan: Phase 5 — Planning\nBranch: feature/x\nBase: main\nCheck command: cargo test && echo hi\n\
Setup command: cargo fetch\nIsolation: worktree\nMax parallel: 2\nScope: warn\nBudget: 3h\n\
Commits: per-milestone\nLead mode: events\nRisk hold: off\nSetup inputs: Cargo.lock\n\
Lead: lead (claude: opus)\nWorkers: a-1 (claude: sonnet),\n  b-1 (codex)\nReviewer: rev (agy: gemini-3.8-flash)\n",
        )
        .unwrap();
        assert_eq!(p.title.as_deref(), Some("Phase 5 — Planning"));
        let h = &p.header;
        assert_eq!(h.branch.as_deref(), Some("feature/x"));
        assert_eq!(h.base.as_deref(), Some("main"));
        assert_eq!(h.check_command.as_deref(), Some("cargo test && echo hi"));
        assert_eq!(h.setup_command.as_deref(), Some("cargo fetch"));
        assert_eq!(h.isolation.as_deref(), Some("worktree"));
        assert_eq!(h.max_parallel.as_deref(), Some("2"));
        assert_eq!(h.scope.as_deref(), Some("warn"));
        assert_eq!(h.budget.as_deref(), Some("3h"));
        assert_eq!(h.commits.as_deref(), Some("per-milestone"));
        assert_eq!(h.lead_mode.as_deref(), Some("events"));
        assert_eq!(h.risk_hold.as_deref(), Some("off"));
        assert_eq!(h.setup_inputs.as_deref(), Some("Cargo.lock"));
        assert_eq!(
            h.workers_raw.as_deref(),
            Some("a-1 (claude: sonnet), b-1 (codex)")
        );
        assert_eq!(h.workers.len(), 2);
        assert_eq!(h.lead[0].kind, "claude");
        assert_eq!(h.reviewer[0].model.as_deref(), Some("gemini-3.8-flash"));
    }

    #[test]
    fn unknown_header_lines_are_kept() {
        let p = parse("");
        assert!(p.header.extra.is_empty());
        let p = parse_plan("Branch: b\nColour: red\n").unwrap();
        assert_eq!(
            p.header.extra,
            vec![("colour".to_string(), "red".to_string())]
        );
        assert_eq!(p.title, None);
    }

    #[test]
    fn agent_forms() {
        let a = parse_agents(
            "claude-1 (claude: Sonnet), hermes-1 (hermes @pane:w1:p3), \
x.y_z (Codex : gpt-5 @pane:7), plain (agy)",
        );
        assert_eq!(a.len(), 4);
        assert_eq!(
            a[0],
            Agent {
                name: "claude-1".into(),
                kind: "claude".into(),
                model: Some("sonnet".into()),
                pane: None
            }
        );
        assert_eq!(
            a[1],
            Agent {
                name: "hermes-1".into(),
                kind: "hermes".into(),
                model: None,
                pane: Some("w1:p3".into())
            }
        );
        assert_eq!(a[2].kind, "codex");
        assert_eq!(a[2].model.as_deref(), Some("gpt-5"));
        assert_eq!(a[2].pane.as_deref(), Some("7"));
        assert_eq!(a[3].model, None);
        assert!(parse_agents("").is_empty());
        assert!(parse_agents("no agents here (: )").is_empty());
        // A model needs a kind, and `@pane` needs whitespace before it.
        assert!(parse_agents("a (b@pane:1)").is_empty());
    }

    #[test]
    fn milestone_fields_and_sections() {
        let p = parse(
            "## Global instructions\n- one\n  - nested\nDone when: it works\n\n## Feature breakdown\n- f\n\n\
## [x] M1: First thing\nDepends on: none\nWhy separate: because\nPrefer: claude-1\nTier: high\n\
Needs: architecture, reasoning\nModel why: hard\nReview: yes\nLikely files: a.rs, `b/`\nParallel: allow\n\
Goal:\n- do it\nWhy: text\nAcceptance criteria:\n- first\n  - nested a\n  - nested b\n- second line one\n  continues here\n\n  after blank\n- third\n\
Constraints:\n- c\nInterfaces:\n- i\nContext:\n- x\nOut of scope:\n- y\nOpen questions:\n- none\nNotes from user:\n\n\
## [ ] M2: Skipped\nDepends on: M1 (needs the types), M3\nTier: small\n",
        );
        assert_eq!(p.global_instructions.lines.len(), 3);
        assert_eq!(p.global_instructions.items().len(), 1);
        assert_eq!(p.feature_breakdown.lines, vec!["- f"]);
        assert_eq!(p.milestones.len(), 2);
        let m = &p.milestones[0];
        assert_eq!(
            (m.id.as_str(), m.title.as_str(), m.enabled),
            ("M1", "First thing", true)
        );
        assert_eq!(m.line, 12);
        assert!(m.depends_on.is_empty());
        assert_eq!(m.why_separate.as_deref(), Some("because"));
        assert_eq!(m.prefer.as_deref(), Some("claude-1"));
        assert_eq!(m.tier.as_deref(), Some("high"));
        assert_eq!(m.needs.as_deref(), Some("architecture, reasoning"));
        assert_eq!(m.model_why.as_deref(), Some("hard"));
        assert_eq!(m.review.as_deref(), Some("yes"));
        assert_eq!(m.likely_file_list(), vec!["a.rs", "b/"]);
        assert!(m.parallel_allowed());
        assert_eq!(m.goal.lines, vec!["- do it"]);
        assert_eq!(m.why.lines, vec!["text"]);
        assert_eq!(m.constraints.items(), vec!["c"]);
        assert_eq!(m.interfaces.items(), vec!["i"]);
        assert_eq!(m.context.items(), vec!["x"]);
        assert_eq!(m.out_of_scope.items(), vec!["y"]);
        assert!(!m.has_open_questions());
        assert!(m.notes_from_user.is_empty());
        // Nested bullets and multi-line items stay intact.
        assert_eq!(
            m.acceptance_criteria,
            vec![
                "first\n  - nested a\n  - nested b".to_string(),
                "second line one\n  continues here\n\n  after blank".to_string(),
                "third".to_string(),
            ]
        );
        let m2 = &p.milestones[1];
        assert!(!m2.enabled);
        assert_eq!(
            m2.depends_on,
            vec![
                Dependency {
                    id: "M1".into(),
                    reason: Some("needs the types".into())
                },
                Dependency {
                    id: "M3".into(),
                    reason: None
                },
            ]
        );
        assert_eq!(
            m2.depends_on_raw.as_deref(),
            Some("M1 (needs the types), M3")
        );
    }

    #[test]
    fn dependency_forms() {
        for none in ["none", "None", "-", "n/a", ""] {
            assert!(parse_dependencies(none).is_empty(), "{none}");
        }
        let d = parse_dependencies("M2 (a fix to M1; see M9), M3(x)  M4 ");
        let ids: Vec<_> = d.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["M2", "M3", "M4"]);
        assert_eq!(d[0].reason.as_deref(), Some("a fix to M1; see M9"));
        assert_eq!(d[1].reason.as_deref(), Some("x"));
        assert_eq!(d[2].reason, None);
    }

    #[test]
    fn open_questions() {
        let p = parse(
            "## [x] M1: A\nOpen questions:\n- Which one?\n## [x] M2: B\nOpen questions: none\n",
        );
        assert!(p.milestones[0].has_open_questions());
        assert!(!p.milestones[1].has_open_questions());
    }

    #[test]
    fn key_value_lines_in_global_instructions_are_text() {
        let p = parse("## Global instructions\nMust not: do things\n## [x] M1: A\n");
        assert_eq!(p.global_instructions.lines, vec!["Must not: do things"]);
        assert_eq!(p.milestones.len(), 1);
        assert_eq!(p.header.extra.len(), 0);
    }

    #[test]
    fn repeated_field_keeps_the_last() {
        let p = parse("## [x] M1: A\nTier: small\nTier: high\n");
        assert_eq!(p.milestones[0].tier.as_deref(), Some("high"));
    }

    #[test]
    fn missing_branch_names_a_line() {
        let e = parse_plan("# Plan: x\nBase: main\n\n## [x] M1: A\n").unwrap_err();
        assert_eq!(e.line, 4);
        assert!(e.message.contains("Branch"));
        assert!(e.to_string().starts_with("plan line 4:"));
        let e = parse_plan("").unwrap_err();
        assert_eq!(e.line, 1);
        let e = parse_plan("Branch:\n").unwrap_err();
        assert_eq!(e.line, 1);
    }

    #[test]
    fn broken_milestone_headings_name_the_line() {
        for bad in [
            "## [x] M1 no colon",
            "## [y] M1: bad box",
            "## [x] Mx: no number",
            "## [x] M1:",
            "## M1: no checkbox",
            "## [x]M: x",
        ] {
            let e = parse_plan(&format!("Branch: b\n\n{bad}\n")).unwrap_err();
            assert_eq!(e.line, 3, "{bad}");
            assert!(e.message.contains("milestone heading"), "{bad}");
        }
        // Spacing Python's regex allows.
        let p = parse_plan("Branch: b\n## [X]M2 :  spaced title  \n").unwrap();
        assert_eq!(
            (p.milestones[0].id.as_str(), p.milestones[0].title.as_str()),
            ("M2", "spaced title")
        );
        assert!(p.milestones[0].enabled);
    }

    #[test]
    fn garbage_never_panics() {
        for s in [
            "\u{0}\n##\n## [\n## [x",
            "## [x] M1: é\n:::\nGoal:\n  \n",
            "Éa: b\n- \n*",
        ] {
            let _ = parse_plan(s);
        }
    }

    #[test]
    fn other_sections_are_kept() {
        let p = parse("## Notes\nsome text\n## [x] M1: A\n");
        assert_eq!(p.other_sections[0].0, "Notes");
        assert_eq!(p.other_sections[0].1.lines, vec!["some text"]);
    }

    #[test]
    fn plan_error_converts_to_exit_2() {
        let e: Error = parse_plan("").unwrap_err().into();
        assert_eq!(e.exit_code(), 2);
    }
}
