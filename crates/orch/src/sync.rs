//! `orch sync`: validate `.orchestrate/plan.md` exactly as Python v4.10.1's `cmd_sync` does and
//! store the validated run in SQLite. `validate` is pure (no files, no herdr); `run_sync`
//! reads the plan, validates, checks the team against herdr through the `RuntimeAdapter`
//! and writes the store.

use orch_config::agents::Registry;
use orch_config::launch::repr;
use orch_config::models::{Catalog, user_models_file};
use orch_core::graph::DepGraph;
use orch_core::paths;
use orch_core::plan::{Agent, Milestone, Plan, parse_agents, parse_plan};
use orch_core::run_store::{AgentRole, PlanTask, RunAgent, RunSettings, SyncInput};
use orch_core::store::Store;
use orch_herdr::RuntimeAdapter;
use std::collections::{BTreeSet, HashSet};
use std::io::Write;
use std::path::Path;

pub const TIERS: [&str; 3] = ["small", "medium", "high"];
pub const NEEDS: [&str; 7] = [
    "reasoning",
    "coding",
    "architecture",
    "long-context",
    "tool-use",
    "planning",
    "speed",
];

/// What validation needs from the outside world.
pub struct Env<'a> {
    pub registry: &'a Registry,
    pub catalog: &'a Catalog,
    /// `ORCH_MAX_TASKS` (0 = no limit).
    pub max_tasks: i64,
    /// The user's model catalog file, named in the "not in the catalog" warning.
    pub models_file: std::path::PathBuf,
    /// Is this a valid git branch name (`git check-ref-format --branch`)?
    pub branch_ok: &'a dyn Fn(&str) -> bool,
}

/// What validation found.
#[derive(Debug, Default)]
pub struct Validated {
    pub errors: Vec<String>,
    pub warns: Vec<String>,
    /// What to store; set only when there are no errors.
    pub input: Option<SyncInput>,
    /// The final `OK: ...` line.
    pub ok_line: String,
}

/// Python's `repr` of a list of strings.
fn py_list(items: &[String]) -> String {
    let q: Vec<String> = items.iter().map(|i| repr(i)).collect();
    format!("[{}]", q.join(", "))
}

fn first_token(s: &str) -> String {
    s.split_whitespace().next().unwrap_or("").to_lowercase()
}

/// `{**a, **b}` keeping first-seen order, later values winning.
fn put(models: &mut Vec<(String, Option<String>)>, agents: &[Agent]) {
    for a in agents {
        match models.iter_mut().find(|(n, _)| *n == a.name) {
            Some(slot) => slot.1 = a.model.clone(),
            None => models.push((a.name.clone(), a.model.clone())),
        }
    }
}

/// Python's `dict(parse_agents(..))`: first position, last value.
fn uniq(agents: Vec<Agent>) -> Vec<Agent> {
    let mut out: Vec<Agent> = Vec::new();
    for a in agents {
        match out.iter_mut().find(|o| o.name == a.name) {
            Some(slot) => *slot = a,
            None => out.push(a),
        }
    }
    out
}

fn tier_rank(tier: Option<&str>) -> usize {
    tier.and_then(|t| TIERS.iter().position(|x| *x == t))
        .map_or(0, |i| i + 1)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Python's `HOTSPOT_RE`: files nearly every milestone wants to touch.
pub fn is_hotspot(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    const EXACT: &[&str] = &[
        "package.json",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "bun.lock",
        "bun.lockb",
        "pubspec.yaml",
        "pubspec.lock",
        "go.mod",
        "go.sum",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "cargo.toml",
        "cargo.lock",
        "requirements.txt",
        "pyproject.toml",
        "poetry.lock",
        "uv.lock",
        "docker-compose.yaml",
        "docker-compose.yml",
        "compose.yaml",
        "compose.yml",
        "makefile",
        ".env.example",
        "app.tsx",
        "app.jsx",
        "app.vue",
        "app.svelte",
        "main.ts",
        "main.tsx",
        "main.js",
        "main.dart",
        "main.go",
    ];
    if EXACT.contains(&name.as_str()) {
        return true;
    }
    let word_ext = |prefix: &str| {
        name.strip_prefix(prefix)
            .is_some_and(|e| !e.is_empty() && e.chars().all(is_word))
    };
    word_ext("route.") || word_ext("routes.") || word_ext("router.")
}

/// Python's `plan_sections()`: each milestone's text from its heading to the next `## `.
fn milestone_texts(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut cur: Option<(String, Vec<&str>)> = None;
    let flush = |cur: &mut Option<(String, Vec<&str>)>, out: &mut Vec<(String, String)>| {
        if let Some((id, buf)) = cur.take() {
            let t = buf.join("\n").trim().to_string();
            match out.iter_mut().find(|(i, _)| *i == id) {
                Some(slot) => slot.1 = t,
                None => out.push((id, t)),
            }
        }
    };
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.starts_with("## ") {
            flush(&mut cur, &mut out);
            cur = heading_id(line).map(|id| (id, vec![line]));
            continue;
        }
        if let Some((_, buf)) = cur.as_mut() {
            buf.push(line);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// The `Mn` of a `## [x] Mn: title` heading.
fn heading_id(line: &str) -> Option<String> {
    let rest = line.strip_prefix("## [")?;
    let rest = rest.get(1..)?.strip_prefix(']')?.trim_start();
    let rest = rest.strip_prefix('M')?;
    let n = rest.chars().take_while(char::is_ascii_digit).count();
    (n > 0).then(|| format!("M{}", &rest[..n]))
}

/// Python's step-by-step regex: `step 3`, or `1. create ...`.
fn reads_like_steps(sec: &str) -> bool {
    const VERBS: [&str; 10] = [
        "create",
        "add",
        "implement",
        "write",
        "call",
        "refactor",
        "update",
        "use",
        "install",
        "modify",
    ];
    let boundary = |s: &str| !s.chars().next().is_some_and(is_word);
    sec.lines().any(|l| {
        let t = l.trim_start().to_lowercase();
        if let Some(rest) = t.strip_prefix("step") {
            let rest = rest.trim_start();
            let n = rest.chars().take_while(char::is_ascii_digit).count();
            if n > 0 && boundary(&rest[n..]) {
                return true;
            }
        }
        let n = t.chars().take_while(char::is_ascii_digit).count();
        if n > 0
            && let Some(rest) = t[n..].strip_prefix('.')
        {
            let rest = rest.trim_start();
            return VERBS
                .iter()
                .any(|v| rest.strip_prefix(v).is_some_and(boundary));
        }
        false
    })
}

/// Does `raw` mention `id` followed by `(` (Python: `\bMn\s*\(`)?
fn has_reason(raw: &str, id: &str) -> bool {
    let mut from = 0;
    while let Some(p) = raw[from..].find(id) {
        let at = from + p;
        let before_ok = !raw[..at].chars().next_back().is_some_and(is_word);
        let after = raw[at + id.len()..].trim_start();
        if before_ok && after.starts_with('(') {
            return true;
        }
        from = at + id.len();
    }
    false
}

/// Python's budget regex `\s*(\d+(?:\.\d+)?)\s*(h|m|min|hours?|minutes?)?\s*`, in seconds.
fn parse_budget(s: &str) -> Option<i64> {
    let s = s.trim();
    let int_len = s.chars().take_while(char::is_ascii_digit).count();
    if int_len == 0 {
        return None;
    }
    let mut num_end = int_len;
    if let Some(frac) = s[int_len..].strip_prefix('.') {
        let f = frac.chars().take_while(char::is_ascii_digit).count();
        if f > 0 {
            num_end = int_len + 1 + f;
        }
    }
    let n: f64 = s[..num_end].parse().ok()?;
    let unit = s[num_end..].trim().to_lowercase();
    let mult = match unit.as_str() {
        "" | "h" | "hour" | "hours" => 3600.0,
        "m" | "min" | "minute" | "minutes" => 60.0,
        _ => return None,
    };
    Some(((n * mult) as i64).max(1))
}

/// Python's `\d+` first match as an int (saturating).
fn first_number(s: &str) -> Option<i64> {
    let start = s.find(|c: char| c.is_ascii_digit())?;
    let digits: String = s[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    Some(digits.parse().unwrap_or(i64::MAX))
}

fn split_list(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .map(str::to_string)
        .collect()
}

/// Validate a plan the way Python's `cmd_sync` does. `text` is the plan text (for the
/// title and each milestone's section); `old_base` is the base the last sync stored.
pub fn validate(plan: &Plan, text: &str, env: &Env, old_base: Option<&str>) -> Validated {
    let mut errors: Vec<String> = Vec::new();
    let mut warns: Vec<String> = Vec::new();
    let h = &plan.header;
    let get = |v: &Option<String>| v.as_deref().unwrap_or("").trim().to_string();
    let get_or = |v: &Option<String>, d: &str| {
        let g = get(v);
        if g.is_empty() { d.to_string() } else { g }
    };

    let branch = get(&h.branch)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    if branch.starts_with("orch/") {
        warns.push(format!(
            "Branch {branch}: runs now use one feature branch — prefer `Branch: feature/<slug>` \
(orch no longer makes per-milestone branches either way)"
        ));
    }
    if !(env.branch_ok)(&branch) {
        errors.push(format!(
            "Branch {} is not a valid git branch name",
            repr(&branch)
        ));
    }
    let commits = first_token(&get_or(&h.commits, "per-milestone"));
    if commits != "per-milestone" && commits != "single" {
        errors.push(format!(
            "Commits must be per-milestone or single, got {}",
            repr(&commits)
        ));
    }
    let title = text
        .lines()
        .find_map(|l| l.strip_prefix("# Plan:"))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| branch.clone());
    let isolation = first_token(&get_or(&h.isolation, "worktree"));
    if isolation != "worktree" && isolation != "shared" {
        errors.push(format!(
            "Isolation must be worktree or shared, got {}",
            repr(&isolation)
        ));
    }
    let mut max_par = first_number(&get_or(&h.max_parallel, "2")).unwrap_or(2);
    if isolation == "shared" {
        max_par = 1;
    }
    let scope = first_token(&get_or(&h.scope, "warn"));
    if scope != "warn" && scope != "strict" {
        errors.push(format!(
            "Scope must be warn or strict, got {}",
            repr(&scope)
        ));
    }
    let risk_hold = !get_or(&h.risk_hold, "on").to_lowercase().starts_with("off");
    let mut budget_s = None;
    let budget = get(&h.budget);
    if !budget.is_empty() {
        match parse_budget(&budget) {
            Some(s) => budget_s = Some(s),
            None => errors.push(format!(
                "Budget must look like `3h` or `90m`, got {}",
                repr(&budget)
            )),
        }
    }
    let workers = uniq(parse_agents(&get(&h.workers_raw)));
    if workers.is_empty() {
        errors.push("plan header needs `Workers: claude-1 (claude), codex-1 (codex)`".into());
    }
    let rev = get_or(&h.reviewer_raw, "none");
    let rev_agents = uniq(parse_agents(&rev));
    let rev_name = {
        let name: String = rev
            .chars()
            .take_while(|c| is_word(*c) || *c == '.' || *c == '-')
            .collect();
        let after = rev[name.len()..].trim_start();
        (!name.is_empty() && after.starts_with('(')).then_some(name)
    };
    let reviewer = if rev.to_lowercase().starts_with("none") {
        None
    } else {
        rev_name
    };
    let rev_agent = reviewer
        .as_ref()
        .and_then(|r| rev_agents.iter().find(|a| &a.name == r));
    let lead_agents = uniq(parse_agents(&get(&h.lead_raw)));
    let lead = lead_agents.first().cloned();
    if !get(&h.lead_raw).is_empty() && lead.is_none() {
        errors.push("`Lead:` must look like `Lead: lead (claude)`".into());
    }
    if let Some(l) = &lead {
        if workers.iter().any(|w| w.name == l.name) {
            errors.push(format!(
                "lead {} is also listed in Workers — the lead never codes in its own run",
                repr(&l.name)
            ));
        }
        if reviewer.as_deref() == Some(l.name.as_str()) {
            errors.push(format!(
                "lead {} is also the Reviewer — the lead never reviews in its own run",
                repr(&l.name)
            ));
        }
    }
    let mut models: Vec<(String, Option<String>)> = Vec::new();
    put(&mut models, &workers);
    put(&mut models, &rev_agents);
    put(&mut models, &lead_agents);
    let model_of = |name: &str| {
        models
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, m)| m.clone())
    };
    for (name, mid) in &models {
        let Some(mid) = mid else { continue };
        if let Some(why) = env.catalog.denied(mid) {
            errors.push(format!(
                "{name} runs {}, which the model policy does not allow (pattern {}: {}) — pick another model (`orch models`)",
                repr(mid),
                repr(why),
                env.catalog
                    .policy
                    .deny_why
                    .as_deref()
                    .unwrap_or("not allowed in runs")
            ));
        } else if env.catalog.model_tier(mid).is_none() {
            warns.push(format!(
                "{name} runs {}, which is not in the model catalog, so it has no tier — add it to {} (`orch models` shows the format)",
                repr(mid),
                env.models_file.display()
            ));
        }
    }
    let tier_of = |name: &str| {
        model_of(name)
            .and_then(|m| env.catalog.model_tier(&m))
            .map(str::to_string)
    };
    let lead_mode = first_token(&get_or(
        &h.lead_mode,
        if lead.is_some() { "events" } else { "off" },
    ));
    if lead_mode != "events" && lead_mode != "off" {
        errors.push(format!(
            "Lead mode must be events or off, got {}",
            repr(&lead_mode)
        ));
    }
    if lead_mode == "events" && lead.is_none() {
        errors.push("`Lead mode: events` needs a `Lead: lead (<kind>)` header line".into());
    }

    // ---- milestones
    let ms = &plan.milestones;
    let ids: Vec<&str> = ms.iter().map(|m| m.id.as_str()).collect();
    let graph = DepGraph::new(plan);
    let dupes = graph.duplicate_ids(plan);
    if !dupes.is_empty() {
        let mut d = dupes;
        d.sort();
        errors.push(format!("duplicate task ids: {}", py_list(&d)));
    }
    for m in ms {
        let raw = m.depends_on_raw.as_deref().unwrap_or("none");
        for d in &m.depends_on {
            if m.enabled && !has_reason(raw, &d.id) {
                warns.push(format!(
                    "{} depends on {} without a reason — write `{} (what it needs from {})`",
                    m.id, d.id, d.id, d.id
                ));
            }
        }
    }
    errors.extend(graph.issues().iter().map(ToString::to_string));

    let by_id = |id: &str| plan.milestone(id);
    let files_of = |id: &str| -> BTreeSet<String> {
        by_id(id)
            .map(|m| m.likely_file_list().into_iter().collect())
            .unwrap_or_default()
    };
    let enabled: Vec<&str> = ms
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.id.as_str())
        .collect();
    let mut parallel_pairs: Vec<(&str, &str)> = Vec::new();
    let mut hot_warned: HashSet<&str> = HashSet::new();
    let mut any_parallel = false;
    for (i, a) in enabled.iter().enumerate() {
        for b in &enabled[i + 1..] {
            if !graph.independent(a, b) {
                continue;
            }
            any_parallel = true;
            let (fa, fb) = (files_of(a), files_of(b));
            let common: Vec<String> = fa.intersection(&fb).cloned().collect();
            let par = [a, b]
                .iter()
                .any(|x| by_id(x).is_some_and(Milestone::parallel_allowed));
            if !common.is_empty() && isolation == "worktree" {
                warns.push(format!(
                    "{a} and {b} both touch {}: {}",
                    py_list(&common),
                    if par {
                        "`Parallel: allow` runs them together — expect a merge conflict"
                    } else {
                        "orch runs them one after the other (add `Depends on` to fix the order, or \
`Parallel: allow` to run them together)"
                    }
                ));
            }
            if isolation == "shared" {
                parallel_pairs.push((a, b));
            }
            for (x, other, files) in [(*a, *b, &fa), (*b, *a, &fb)] {
                let hot: Vec<String> = files.iter().filter(|f| is_hotspot(f)).cloned().collect();
                if !hot.is_empty() && isolation == "worktree" && hot_warned.insert(x) {
                    warns.push(format!(
                        "{x} touches shared hotspot file(s) {} but can run in parallel with {other} — move those edits into a scaffold milestone both depend on",
                        py_list(&hot)
                    ));
                }
            }
        }
    }
    if workers.len() >= 2 && enabled.len() >= 2 && !any_parallel {
        warns.push(format!(
            "{} coders but every milestone depends on the one before, so only one coder works at a time — \
if the feature spans backend and frontend, put the contract under Interfaces (or in a small contract milestone) \
so both sides can run in parallel",
            workers.len()
        ));
    }
    if workers.len() == 1 && any_parallel && isolation == "worktree" {
        warns.push(
            "1 coder: independent milestones will still run one at a time — `Isolation: shared` is simpler unless you will add coders"
                .into(),
        );
    }
    if let Some((a, b)) = parallel_pairs.first() {
        warns.push(format!(
            "{a} and {b} are independent — with `Isolation: worktree` they could run in parallel (shared mode runs one milestone at a time)"
        ));
    }

    // The lead describes WHAT and WHY; coders decide HOW.
    let secs = milestone_texts(text);
    for tid in &enabled {
        let sec = secs
            .iter()
            .find(|(i, _)| i == tid)
            .map_or("", |(_, s)| s.as_str());
        if sec.contains("```") {
            warns.push(format!(
                "{tid} contains a code block — describe what and why; the coder decides how"
            ));
        } else if reads_like_steps(sec) {
            warns.push(format!(
                "{tid} reads like step-by-step implementation — state the goal and acceptance criteria; the coder decides how"
            ));
        }
    }
    for tid in &enabled {
        let has = by_id(tid).is_some_and(|m| {
            m.acceptance_criteria
                .iter()
                .any(|c| !c.trim().is_empty() && !c.trim().eq_ignore_ascii_case("none"))
        });
        if !has {
            warns.push(format!(
                "{tid} has no Acceptance criteria — the coder's result and the review check each one"
            ));
        }
    }

    // Tiers: what each milestone asks of its coder's model, and whether the team can give it.
    let worker_ranks: Vec<usize> = workers
        .iter()
        .map(|w| tier_rank(tier_of(&w.name).as_deref()))
        .collect();
    let mut tiers: Vec<Option<String>> = Vec::new();
    let mut needs_all: Vec<Vec<String>> = Vec::new();
    for m in ms {
        let tid = &m.id;
        let mut tv = first_token(m.tier.as_deref().unwrap_or("any"));
        if tv.is_empty() {
            tv = "any".into();
        }
        if !TIERS.contains(&tv.as_str()) && tv != "any" {
            errors.push(format!(
                "{tid}: Tier must be small, medium or high, got {}",
                repr(&tv)
            ));
            tv = "any".into();
        }
        let tier = (tv != "any").then(|| tv.clone());
        tiers.push(tier.clone());
        let needs: Vec<String> = m
            .needs
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .collect();
        let bad: Vec<String> = needs
            .iter()
            .filter(|n| !NEEDS.contains(&n.as_str()))
            .cloned()
            .collect();
        if !bad.is_empty() {
            warns.push(format!(
                "{tid}: Needs has unknown value(s) {} — use any of: {}",
                py_list(&bad),
                NEEDS.join(", ")
            ));
        }
        needs_all.push(needs);
        if !m.enabled || tier.is_none() {
            continue;
        }
        let rank = tier_rank(Some(&tv));
        if m.model_why.as_deref().unwrap_or("").is_empty() {
            warns.push(format!(
                "{tid} has `Tier: {tv}` but no `Model why:` — say in one line why this tier fits"
            ));
        }
        if worker_ranks.iter().any(|r| *r != 0)
            && !worker_ranks.iter().any(|r| *r == 0 || *r >= rank)
        {
            warns.push(format!(
                "{tid} asks for a {tv}-tier model, but no coder in Workers runs one — it will go to the strongest coder you have; \
start a coder with a {tv}-tier model (`orch models`) or lower the tier"
            ));
        } else if worker_ranks.iter().all(|r| *r == 0) && !workers.is_empty() {
            warns.push(format!(
                "{tid} has `Tier: {tv}`, but Workers does not say which model each coder runs — write `name (kind: model)` so the tier can be honoured"
            ));
        }
        if let Some(r) = &reviewer
            && m.review
                .as_deref()
                .unwrap_or("no")
                .to_lowercase()
                .starts_with('y')
        {
            let rt = tier_of(r);
            let rr = tier_rank(rt.as_deref());
            if rr != 0 && rr < rank {
                warns.push(format!(
                    "{tid} is {tv}-tier work reviewed by a {}-tier model ({}) — a reviewer weaker than the coder misses design errors",
                    rt.unwrap_or_default(),
                    model_of(r).unwrap_or_default()
                ));
            }
        }
    }
    if let Some(l) = &lead
        && let Some(lm) = model_of(&l.name)
    {
        let lt = tier_of(&l.name);
        let r = tier_rank(lt.as_deref());
        if r != 0 && r != 3 {
            warns.push(format!(
                "the lead runs {} ({} tier) — planning and coordination are high-tier work",
                repr(&lm),
                lt.unwrap_or_default()
            ));
        }
    }

    // The feature decides how many milestones there are: every split has to say what it buys.
    if enabled.len() > 1 {
        for tid in &enabled {
            if by_id(tid).is_none_or(|m| m.why_separate.as_deref().unwrap_or("").is_empty()) {
                warns.push(format!(
                    "{tid} has no `Why separate:` — say why it is its own milestone (parallel work, a foundation others build on, \
too big for one sitting), or merge it into another"
                ));
            }
        }
    }
    if env.max_tasks > 0 && enabled.len() as i64 > env.max_tasks {
        warns.push(format!(
            "{} enabled milestones (> ORCH_MAX_TASKS={}). Merge coupled milestones or split the feature into separate runs",
            enabled.len(),
            env.max_tasks
        ));
    }

    if !errors.is_empty() {
        return Validated {
            errors,
            warns,
            ..Validated::default()
        };
    }

    let base = get(&h.base);
    let base = if base.is_empty() {
        old_base.map(str::to_string).unwrap_or_default()
    } else {
        base
    };
    if let Some(b) = base.split_whitespace().next() {
        if b == branch {
            errors.push(format!("Base and Branch are both {}", repr(&branch)));
        }
        if !(env.branch_ok)(b) {
            errors.push(format!(
                "Base {} is not a valid git branch name",
                repr(&base)
            ));
        }
    }
    if !errors.is_empty() {
        return Validated {
            errors,
            warns,
            ..Validated::default()
        };
    }

    // The detection the state records; its warnings come after the plan's own.
    let mut detect = |kind: &str, pane: &Option<String>, role: &str| -> bool {
        if pane.is_some() {
            return false;
        }
        match env.registry.get(kind) {
            None => {
                warns.push(format!(
                    "{role} kind {} is not in the agent registry (`orch agents`) — herdr may not watch it, so only its done file and time budget count",
                    repr(kind)
                ));
                false
            }
            Some(k) => k.herdr_detect,
        }
    };
    let mut agents: Vec<RunAgent> = Vec::new();
    if let Some(r) = &reviewer {
        let (kind, pane) =
            rev_agent.map_or((None, None), |a| (Some(a.kind.clone()), a.pane.clone()));
        let d = detect(kind.as_deref().unwrap_or("None"), &pane, "Reviewer");
        agents.push(RunAgent {
            role: AgentRole::Reviewer,
            name: r.clone(),
            kind: kind.unwrap_or_default(),
            model: model_of(r),
            tier: tier_of(r),
            detect: d,
            pane,
        });
    }
    if let Some(l) = &lead {
        let d = if lead_mode == "events" {
            detect(&l.kind, &l.pane, "Lead")
        } else {
            true
        };
        agents.push(RunAgent {
            role: AgentRole::Lead,
            name: l.name.clone(),
            kind: l.kind.clone(),
            model: model_of(&l.name),
            tier: tier_of(&l.name),
            detect: d,
            pane: l.pane.clone(),
        });
    }
    for w in &workers {
        let d = detect(&w.kind, &w.pane, "Workers");
        agents.push(RunAgent {
            role: AgentRole::Worker,
            name: w.name.clone(),
            kind: w.kind.clone(),
            model: model_of(&w.name),
            tier: tier_of(&w.name),
            detect: d,
            pane: w.pane.clone(),
        });
    }

    let tasks: Vec<PlanTask> = ms
        .iter()
        .enumerate()
        .map(|(i, m)| PlanTask {
            id: m.id.clone(),
            title: m.title.clone(),
            deps: m.depends_on.iter().map(|d| d.id.clone()).collect(),
            enabled: m.enabled,
            open_questions: m.has_open_questions(),
            tier: tiers[i].clone(),
            needs: needs_all[i].clone(),
            model_why: m.model_why.clone().unwrap_or_default(),
            prefer: m
                .prefer
                .as_deref()
                .and_then(|p| p.split_whitespace().next())
                .map_or_else(|| "any".into(), str::to_lowercase),
            review: m
                .review
                .as_deref()
                .unwrap_or("no")
                .to_lowercase()
                .starts_with('y')
                && reviewer.is_some(),
            files: files_of(&m.id).into_iter().collect(),
            parallel: m.parallel_allowed(),
        })
        .collect();
    let settings = RunSettings {
        branch: branch.clone(),
        base: base.split_whitespace().next().map(str::to_string),
        title,
        commits,
        isolation: isolation.clone(),
        max_parallel: max_par,
        setup_inputs: split_list(&get(&h.setup_inputs))
            .into_iter()
            .map(|p| p.trim_matches('`').to_string())
            .filter(|p| !p.is_empty())
            .collect(),
        scope,
        risk_hold,
        budget_s,
        check_command: get(&h.check_command),
        setup_command: get_or(&h.setup_command, "none"),
        lead_mode,
        reviewer: reviewer.clone(),
    };
    let worker_names: Vec<String> = workers.iter().map(|w| w.name.clone()).collect();
    let ok_line = format!(
        "OK: {} tasks, isolation={isolation}, max_parallel={max_par}, workers={}, reviewer={}{}",
        ids.len(),
        py_list(&worker_names),
        reviewer.as_deref().unwrap_or("None"),
        lead.as_ref()
            .map_or(String::new(), |l| format!(", lead={}", l.name))
    );
    Validated {
        errors,
        warns,
        input: Some(SyncInput {
            settings,
            agents,
            tasks,
        }),
        ok_line,
    }
}

/// Where each worker and the reviewer stands in herdr (Python's `start` report line).
pub fn herdr_lines(runtime: &dyn RuntimeAdapter, agents: &[RunAgent]) -> Vec<String> {
    let mut out = Vec::new();
    let order = [
        (AgentRole::Worker, "WORKER"),
        (AgentRole::Reviewer, "REVIEWER"),
    ];
    for (role, label) in order {
        for a in agents.iter().filter(|a| a.role == role) {
            let herdr = if let Some(pane) = &a.pane {
                if runtime.pane(pane).is_ok() {
                    format!("pane {pane} detect:off")
                } else {
                    format!(
                        "STALE pane {pane} — find its current id with `herdr pane list` and fix plan.md, then `orch sync`"
                    )
                }
            } else if !a.detect {
                "detect:off (relying on its done file)".to_string()
            } else if runtime.agent(&a.name).is_ok() {
                "found".to_string()
            } else {
                "NOT FOUND".to_string()
            };
            out.push(format!("{label} {} kind={} herdr={herdr}", a.name, a.kind));
        }
    }
    out
}

/// `git check-ref-format --branch <name>` in `dir`.
pub fn git_branch_ok(dir: &Path, name: &str) -> bool {
    std::process::Command::new("git")
        .args(["check-ref-format", "--branch", name])
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `orch sync` from `start` (any folder of the repository): everything to `out`.
/// Returns the exit code (0 ok, 2 errors).
pub fn run_sync(
    start: &Path,
    runtime: &dyn RuntimeAdapter,
    registry: &Registry,
    catalog: &Catalog,
    max_tasks: i64,
    out: &mut dyn Write,
) -> i32 {
    let die = |m: String| {
        eprintln!("ERROR: {m}");
        2
    };
    let root = match paths::main_root(start) {
        Ok(r) => r,
        Err(e) => return die(e.to_string()),
    };
    let plan_path = root.join(paths::ORCH_DIR).join("plan.md");
    let Ok(text) = std::fs::read_to_string(&plan_path) else {
        return die("missing .orchestrate/plan.md".into());
    };
    let plan = match parse_plan(&text) {
        Ok(p) => p,
        Err(e) => return die(e.to_string()),
    };
    let mut store = match Store::open_in(&root.join(paths::ORCH_DIR)) {
        Ok(s) => s,
        Err(e) => return die(e.to_string()),
    };
    let old_base = store.run_settings().ok().flatten().and_then(|s| s.base);
    let branch_ok = |b: &str| git_branch_ok(&root, b);
    let env = Env {
        registry,
        catalog,
        max_tasks,
        models_file: user_models_file(),
        branch_ok: &branch_ok,
    };
    let v = validate(&plan, &text, &env, old_base.as_deref());
    if !v.errors.is_empty() {
        for e in &v.errors {
            let _ = writeln!(out, "ERROR: {e}");
        }
        return 2;
    }
    let Some(input) = v.input else { return 2 };
    let stored = match store.apply_sync(&input) {
        Ok(w) => w,
        Err(e) => return die(e.to_string()),
    };
    for w in v.warns.iter().chain(&stored) {
        let _ = writeln!(out, "WARN: {w}");
    }
    for l in herdr_lines(runtime, &input.agents) {
        let _ = writeln!(out, "{l}");
    }
    let _ = writeln!(out, "{}", v.ok_line);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use orch_config::{agents, models};
    use orch_core::run_store::AgentRole;
    use orch_core::store::{Counter, TransitionRequest};
    use orch_core::task::State;
    use orch_herdr::fake::FakeHerdr;
    use orch_herdr::model::{AgentInfo, PaneInfo};
    use orch_herdr::status::AgentStatus;

    const BASE: &str = "\
# Plan: Demo
Branch: feature/demo
Check command: cargo test
Workers: claude-1 (claude: sonnet), codex-1 (codex: gpt-6-sol)
Reviewer: rev (claude: opus)
Lead: lead (claude: opus)

## Global instructions
- be nice

## [x] M1: One
Depends on: none
Why separate: a foundation
Tier: medium
Needs: coding
Model why: normal work
Review: yes
Likely files: src/a.rs
Goal:
- g
Acceptance criteria:
- works
Open questions:
- none

## [x] M2: Two
Depends on: M1 (the foundation)
Why separate: second
Tier: medium
Model why: normal work
Likely files: src/b.rs
Goal:
- g
Acceptance criteria:
- works

## [x] M3: Three
Depends on: none
Why separate: independent
Tier: medium
Model why: normal work
Likely files: src/c.rs
Goal:
- g
Acceptance criteria:
- works
";

    fn reg() -> Registry {
        agents::registry(agents::DEFAULT_TOML, Path::new("/nonexistent/m2")).unwrap()
    }

    fn cat() -> Catalog {
        models::catalog(models::DEFAULT_TOML, Path::new("/nonexistent/m2")).unwrap()
    }

    fn edit(edits: &[(&str, &str)]) -> String {
        let mut t = BASE.to_string();
        for (from, to) in edits {
            assert!(t.contains(from), "plan has no {from:?}");
            t = t.replacen(from, to, 1);
        }
        t
    }

    fn check_with(text: &str, max_tasks: i64) -> Validated {
        let (r, c) = (reg(), cat());
        let tmp = tempfile::tempdir().unwrap();
        let ok = |b: &str| git_branch_ok(tmp.path(), b);
        let env = Env {
            registry: &r,
            catalog: &c,
            max_tasks,
            models_file: "/home/dev/.config/herdr-orchestra/models.toml".into(),
            branch_ok: &ok,
        };
        validate(&parse_plan(text).unwrap(), text, &env, None)
    }

    fn check(text: &str) -> Validated {
        check_with(text, 0)
    }

    #[test]
    fn a_clean_plan_has_no_errors_or_warnings() {
        let v = check(BASE);
        assert_eq!(v.errors, Vec::<String>::new());
        assert_eq!(v.warns, Vec::<String>::new());
        assert_eq!(
            v.ok_line,
            "OK: 3 tasks, isolation=worktree, max_parallel=2, workers=['claude-1', 'codex-1'], reviewer=rev, lead=lead"
        );
    }

    /// One plan per message: (edits to the clean plan, exact message). `E` is an ERROR, `W` a WARN.
    #[test]
    fn every_message_is_reported_with_its_exact_text() {
        let user_models = "/home/dev/.config/herdr-orchestra/models.toml".to_string();
        let deny_why = "ultra-premium models are kept out of runs (edit [policy] deny in models.toml to change this)";
        type Case<'a> = (&'a str, Vec<(&'a str, &'a str)>, char, String);
        let cases: Vec<Case> = vec![
            ("branch name", vec![("feature/demo", "feature/a..b")], 'E',
             "Branch 'feature/a..b' is not a valid git branch name".into()),
            ("commits", vec![("Check command:", "Commits: weekly\nCheck command:")], 'E',
             "Commits must be per-milestone or single, got 'weekly'".into()),
            ("isolation", vec![("Check command:", "Isolation: docker\nCheck command:")], 'E',
             "Isolation must be worktree or shared, got 'docker'".into()),
            ("scope", vec![("Check command:", "Scope: loud\nCheck command:")], 'E',
             "Scope must be warn or strict, got 'loud'".into()),
            ("budget", vec![("Check command:", "Budget: soon\nCheck command:")], 'E',
             "Budget must look like `3h` or `90m`, got 'soon'".into()),
            ("no workers", vec![("Workers: claude-1 (claude: sonnet), codex-1 (codex: gpt-6-sol)\n", "")], 'E',
             "plan header needs `Workers: claude-1 (claude), codex-1 (codex)`".into()),
            ("bad lead", vec![("Lead: lead (claude: opus)", "Lead: lead")], 'E',
             "`Lead:` must look like `Lead: lead (claude)`".into()),
            ("lead is a worker", vec![("Lead: lead (claude: opus)", "Lead: claude-1 (claude: opus)")], 'E',
             "lead 'claude-1' is also listed in Workers — the lead never codes in its own run".into()),
            ("lead is the reviewer", vec![("Lead: lead (claude: opus)", "Lead: rev (claude: opus)")], 'E',
             "lead 'rev' is also the Reviewer — the lead never reviews in its own run".into()),
            ("denied model", vec![("claude-1 (claude: sonnet)", "claude-1 (claude: fable-x)")], 'E',
             format!("claude-1 runs 'fable-x', which the model policy does not allow (pattern '*fable*': {deny_why}) — pick another model (`orch models`)")),
            ("lead mode value", vec![("Check command:", "Lead mode: always\nCheck command:")], 'E',
             "Lead mode must be events or off, got 'always'".into()),
            ("events without lead", vec![("Lead: lead (claude: opus)", "Lead mode: events")], 'E',
             "`Lead mode: events` needs a `Lead: lead (<kind>)` header line".into()),
            ("duplicate ids", vec![("## [x] M3: Three", "## [x] M1: Three")], 'E',
             "duplicate task ids: ['M1']".into()),
            ("unknown dependency", vec![("M1 (the foundation)", "M9 (a ghost)")], 'E',
             "M2 depends on unknown task M9".into()),
            ("self dependency", vec![("M1 (the foundation)", "M2 (itself)")], 'E',
             "M2 depends on itself".into()),
            ("cycle", vec![("Depends on: none\nWhy separate: a foundation", "Depends on: M2 (loop)\nWhy separate: a foundation")], 'E',
             "dependency cycle: M1 -> M2 -> M1".into()),
            ("bad tier", vec![("Tier: medium\nNeeds: coding", "Tier: huge\nNeeds: coding")], 'E',
             "M1: Tier must be small, medium or high, got 'huge'".into()),
            ("base is branch", vec![("Check command:", "Base: feature/demo\nCheck command:")], 'E',
             "Base and Branch are both 'feature/demo'".into()),
            ("base name", vec![("Check command:", "Base: bad..base\nCheck command:")], 'E',
             "Base 'bad..base' is not a valid git branch name".into()),
            ("orch/ branch", vec![("feature/demo", "orch/demo")], 'W',
             "Branch orch/demo: runs now use one feature branch — prefer `Branch: feature/<slug>` (orch no longer makes per-milestone branches either way)".into()),
            ("model not in catalog", vec![("claude-1 (claude: sonnet)", "claude-1 (claude: mystery-9)")], 'W',
             format!("claude-1 runs 'mystery-9', which is not in the model catalog, so it has no tier — add it to {user_models} (`orch models` shows the format)")),
            ("kind not in registry", vec![("codex-1 (codex: gpt-6-sol)", "zed-1 (zed: gpt-6-sol)")], 'W',
             "Workers kind 'zed' is not in the agent registry (`orch agents`) — herdr may not watch it, so only its done file and time budget count".into()),
            ("dependency without a reason", vec![("M1 (the foundation)", "M1")], 'W',
             "M2 depends on M1 without a reason — write `M1 (what it needs from M1)`".into()),
            ("overlapping files", vec![("src/c.rs", "src/a.rs")], 'W',
             "M1 and M3 both touch ['src/a.rs']: orch runs them one after the other (add `Depends on` to fix the order, or `Parallel: allow` to run them together)".into()),
            ("overlap with parallel allow", vec![("src/c.rs", "src/a.rs"), ("Likely files: src/a.rs\nGoal:\n- g\nAcceptance criteria:\n- works\nOpen", "Likely files: src/a.rs\nParallel: allow\nGoal:\n- g\nAcceptance criteria:\n- works\nOpen")], 'W',
             "M1 and M3 both touch ['src/a.rs']: `Parallel: allow` runs them together — expect a merge conflict".into()),
            ("hotspot file", vec![("src/a.rs", "Cargo.toml")], 'W',
             "M1 touches shared hotspot file(s) ['Cargo.toml'] but can run in parallel with M3 — move those edits into a scaffold milestone both depend on".into()),
            ("shared mode independents", vec![("Check command:", "Isolation: shared\nCheck command:")], 'W',
             "M1 and M3 are independent — with `Isolation: worktree` they could run in parallel (shared mode runs one milestone at a time)".into()),
            ("sequential plan with idle coders", vec![("Depends on: none\nWhy separate: independent", "Depends on: M2 (after two)\nWhy separate: independent")], 'W',
             "2 coders but every milestone depends on the one before, so only one coder works at a time — if the feature spans backend and frontend, put the contract under Interfaces (or in a small contract milestone) so both sides can run in parallel".into()),
            ("one coder", vec![("Workers: claude-1 (claude: sonnet), codex-1 (codex: gpt-6-sol)", "Workers: claude-1 (claude: sonnet)")], 'W',
             "1 coder: independent milestones will still run one at a time — `Isolation: shared` is simpler unless you will add coders".into()),
            ("code block", vec![("Goal:\n- g\nAcceptance criteria:\n- works\nOpen", "Goal:\n- ```rust\nAcceptance criteria:\n- works\nOpen")], 'W',
             "M1 contains a code block — describe what and why; the coder decides how".into()),
            ("step by step", vec![("Goal:\n- g\nAcceptance criteria:\n- works\nOpen", "Goal:\n1. create the module\nAcceptance criteria:\n- works\nOpen")], 'W',
             "M1 reads like step-by-step implementation — state the goal and acceptance criteria; the coder decides how".into()),
            ("no acceptance criteria", vec![("Acceptance criteria:\n- works\nOpen", "Acceptance criteria:\n- none\nOpen")], 'W',
             "M1 has no Acceptance criteria — the coder's result and the review check each one".into()),
            ("unknown needs", vec![("Needs: coding", "Needs: coding, telepathy")], 'W',
             "M1: Needs has unknown value(s) ['telepathy'] — use any of: reasoning, coding, architecture, long-context, tool-use, planning, speed".into()),
            ("no model why", vec![("Model why: normal work\nReview", "Review")], 'W',
             "M1 has `Tier: medium` but no `Model why:` — say in one line why this tier fits".into()),
            ("no coder reaches the tier", vec![("Tier: medium\nNeeds: coding", "Tier: high\nNeeds: coding")], 'W',
             "M1 asks for a high-tier model, but no coder in Workers runs one — it will go to the strongest coder you have; start a coder with a high-tier model (`orch models`) or lower the tier".into()),
            ("workers without models", vec![("Workers: claude-1 (claude: sonnet), codex-1 (codex: gpt-6-sol)", "Workers: claude-1 (claude), codex-1 (codex)")], 'W',
             "M1 has `Tier: medium`, but Workers does not say which model each coder runs — write `name (kind: model)` so the tier can be honoured".into()),
            ("reviewer weaker than the work", vec![("Reviewer: rev (claude: opus)", "Reviewer: rev (claude: haiku)")], 'W',
             "M1 is medium-tier work reviewed by a small-tier model (haiku) — a reviewer weaker than the coder misses design errors".into()),
            ("lead not high tier", vec![("Lead: lead (claude: opus)", "Lead: lead (claude: sonnet)")], 'W',
             "the lead runs 'sonnet' (medium tier) — planning and coordination are high-tier work".into()),
            ("no why separate", vec![("Why separate: second\n", "")], 'W',
             "M2 has no `Why separate:` — say why it is its own milestone (parallel work, a foundation others build on, too big for one sitting), or merge it into another".into()),
        ];
        for (name, edits, level, msg) in cases {
            let v = check(&edit(&edits));
            let list = if level == 'E' { &v.errors } else { &v.warns };
            assert!(
                list.contains(&msg),
                "{name}: expected {level} {msg:?}\nerrors: {:#?}\nwarns: {:#?}",
                v.errors,
                v.warns
            );
            if level == 'W' {
                assert!(
                    v.errors.is_empty(),
                    "{name}: unexpected errors {:?}",
                    v.errors
                );
            }
        }
    }

    #[test]
    fn max_tasks_warns() {
        let v = check_with(BASE, 2);
        assert!(v.warns.contains(
            &"3 enabled milestones (> ORCH_MAX_TASKS=2). Merge coupled milestones or split the feature into separate runs".to_string()
        ));
    }

    #[test]
    fn errors_stop_the_run_before_any_warning_is_stored() {
        let v = check(&edit(&[("feature/demo", "feature/a..b")]));
        assert!(v.input.is_none());
    }

    #[test]
    fn header_formats() {
        let v = check(&edit(&[(
            "Check command:",
            "Budget: 1.5h\nMax parallel: about 3\nCommits: Single\nRisk hold: off\nCheck command:",
        )]));
        let s = v.input.unwrap().settings;
        assert_eq!(s.budget_s, Some(5400));
        assert_eq!(s.max_parallel, 3);
        assert_eq!(s.commits, "single");
        assert!(!s.risk_hold);
        assert_eq!(s.title, "Demo");
        assert_eq!(parse_budget("90 min"), Some(5400));
        assert_eq!(parse_budget("2 hours"), Some(7200));
        assert_eq!(parse_budget("0m"), Some(1));
        assert_eq!(parse_budget("3x"), None);
        assert_eq!(parse_budget(""), None);
        let v = check(&edit(&[(
            "Check command:",
            "Isolation: shared\nMax parallel: 4\nCheck command:",
        )]));
        assert_eq!(v.input.unwrap().settings.max_parallel, 1);
    }

    #[test]
    fn hotspots_and_step_text() {
        for f in [
            "Cargo.toml",
            "crates/x/Cargo.lock",
            "src/routes.ts",
            "Router.go",
            "App.tsx",
            "MAIN.GO",
            ".env.example",
        ] {
            assert!(is_hotspot(f), "{f}");
        }
        for f in [
            "src/lib.rs",
            "Cargo.toml.bak",
            "routes",
            "docs/main.rs",
            "xroute.ts",
        ] {
            assert!(!is_hotspot(f), "{f}");
        }
        assert!(reads_like_steps("Goal:\n  Step 2: do it"));
        assert!(reads_like_steps("a\n10. Implement x"));
        assert!(!reads_like_steps("1. creates x\nstep one\nstep 2a"));
    }

    // ---- stored runs

    fn git(dir: &Path, args: &[&str]) {
        let st = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    struct Repo {
        _t: tempfile::TempDir,
        root: std::path::PathBuf,
    }

    impl Repo {
        fn new(plan: &str) -> Repo {
            let t = tempfile::tempdir().unwrap();
            let root = t.path().canonicalize().unwrap().join("repo");
            std::fs::create_dir(&root).unwrap();
            git(&root, &["init", "-q"]);
            git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
            let r = Repo { _t: t, root };
            r.plan(plan);
            r
        }
        fn plan(&self, text: &str) {
            let d = self.root.join(".orchestrate");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("plan.md"), text).unwrap();
        }
        fn store(&self) -> Store {
            Store::open_in(&self.root.join(".orchestrate")).unwrap()
        }
        fn sync(&self, rt: &FakeHerdr) -> (i32, String) {
            let mut out = Vec::new();
            let code = run_sync(&self.root, rt, &reg(), &cat(), 0, &mut out);
            (code, String::from_utf8(out).unwrap())
        }
    }

    #[test]
    fn sync_stores_the_run() {
        let repo = Repo::new(BASE);
        let (code, out) = repo.sync(&FakeHerdr::new());
        assert_eq!(code, 0, "{out}");
        assert!(out.trim_end().ends_with("lead=lead"), "{out}");
        let run = repo.store().load_run().unwrap().unwrap();
        assert_eq!(run.settings.branch, "feature/demo");
        assert_eq!(run.settings.check_command, "cargo test");
        assert_eq!(run.settings.reviewer.as_deref(), Some("rev"));
        let w: Vec<_> = run
            .agents
            .iter()
            .filter(|a| a.role == AgentRole::Worker)
            .collect();
        assert_eq!(
            (w[0].name.as_str(), w[0].kind.as_str()),
            ("claude-1", "claude")
        );
        assert_eq!(w[0].model.as_deref(), Some("sonnet"));
        assert_eq!(w[0].tier.as_deref(), Some("medium"));
        assert!(w[0].detect && w[0].pane.is_none());
        assert_eq!(
            run.agents
                .iter()
                .filter(|a| a.role == AgentRole::Reviewer)
                .count(),
            1
        );
        assert_eq!(run.plan.len(), 3);
        let m1 = &run.plan[0];
        assert_eq!(m1.tier.as_deref(), Some("medium"));
        assert_eq!(m1.needs, ["coding"]);
        assert_eq!(m1.model_why, "normal work");
        assert!(m1.review && m1.enabled && !m1.parallel);
        assert_eq!(m1.files, ["src/a.rs"]);
        assert_eq!(m1.prefer, "any");
        let tasks = repo.store().list_tasks().unwrap();
        assert_eq!(
            tasks.iter().map(|t| t.status).collect::<Vec<_>>(),
            [State::Pending; 3]
        );
        assert_eq!(tasks[1].deps, ["M1"]);
    }

    #[test]
    fn errors_print_and_store_nothing() {
        let repo = Repo::new(&edit(&[("Check command:", "Scope: loud\nCheck command:")]));
        let (code, out) = repo.sync(&FakeHerdr::new());
        assert_eq!(code, 2);
        assert_eq!(out, "ERROR: Scope must be warn or strict, got 'loud'\n");
        assert!(repo.store().load_run().unwrap().is_none());
    }

    #[test]
    fn missing_plan_and_broken_heading_exit_2() {
        let repo = Repo::new(BASE);
        std::fs::remove_file(repo.root.join(".orchestrate/plan.md")).unwrap();
        assert_eq!(repo.sync(&FakeHerdr::new()).0, 2);
        repo.plan("Branch: b\n## [x] M1 no colon\n");
        assert_eq!(repo.sync(&FakeHerdr::new()).0, 2);
    }

    #[test]
    fn resync_keeps_progress_adds_new_and_skips_unticked() {
        let repo = Repo::new(BASE);
        let rt = FakeHerdr::new();
        repo.sync(&rt);
        {
            let mut s = repo.store();
            s.assign_worker("M1", Some("claude-1"), Some("sonnet"))
                .unwrap();
            s.transition(
                "M1",
                &TransitionRequest::auto(State::Coding, Some("dispatched")),
            )
            .unwrap();
            s.bump("M1", Counter::Attempt).unwrap();
            s.bump("M1", Counter::Fix).unwrap();
        }
        // Edit: untick M3 (pending), untick M1 (coding: stays), add M4, retitle M2.
        let edited = edit(&[
            ("## [x] M3: Three", "## [ ] M3: Three"),
            ("## [x] M1: One", "## [ ] M1: One"),
            ("## [x] M2: Two", "## [x] M2: Second"),
        ]) + "\n## [x] M4: Four\nDepends on: M2 (needs it)\nWhy separate: late\nTier: medium\nModel why: x\nGoal:\n- g\nAcceptance criteria:\n- works\n";
        repo.plan(&edited);
        let (code, out) = repo.sync(&rt);
        assert_eq!(code, 0, "{out}");
        let s = repo.store();
        let t = |id: &str| s.get_task(id).unwrap();
        // M1: in progress, so it keeps its state, attempt, counters and history.
        assert_eq!(t("M1").status, State::Coding);
        assert_eq!((t("M1").attempt, t("M1").fix_rounds), (1, 1));
        assert_eq!(t("M1").worker.as_deref(), Some("claude-1"));
        let h = s.history("M1").unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].why, "dispatched");
        // M3: unticked and idle -> skipped, with Python's reason.
        assert_eq!(t("M3").status, State::Skipped);
        assert_eq!(t("M3").reason, "unticked in plan.md");
        let h3 = s.history("M3").unwrap();
        assert!(h3[0].forced);
        // M4: new -> pending. M2: retitled, still pending.
        assert_eq!(t("M4").status, State::Pending);
        assert_eq!(t("M4").deps, ["M2"]);
        assert_eq!(t("M2").title, "Second");
        // Re-ticking brings a skipped milestone back to pending.
        repo.plan(BASE);
        repo.sync(&rt);
        let s = repo.store();
        assert_eq!(s.get_task("M3").unwrap().status, State::Pending);
        // M4 left the plan: its row stays, but it is no longer part of the run.
        let run = s.load_run().unwrap().unwrap();
        assert_eq!(
            run.plan.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["M1", "M2", "M3"]
        );
        assert_eq!(s.get_task("M1").unwrap().status, State::Coding);
    }

    #[test]
    fn open_questions_hold_a_milestone_until_answered() {
        let repo = Repo::new(&edit(&[(
            "Open questions:\n- none",
            "Open questions:\n- which db?",
        )]));
        let rt = FakeHerdr::new();
        repo.sync(&rt);
        let s = repo.store();
        let m1 = s.get_task("M1").unwrap();
        assert_eq!(m1.status, State::NeedsAnswer);
        assert_eq!(m1.reason, "open questions in plan.md");
        repo.plan(BASE);
        repo.sync(&rt);
        assert_eq!(repo.store().get_task("M1").unwrap().status, State::Pending);
    }

    #[test]
    fn a_milestone_depending_on_a_skipped_one_waits() {
        let repo = Repo::new(BASE);
        let rt = FakeHerdr::new();
        repo.sync(&rt);
        repo.plan(&edit(&[("## [x] M1: One", "## [ ] M1: One")]));
        let (code, out) = repo.sync(&rt);
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("WARN: M2 depends on skipped ['M1']\n"),
            "{out}"
        );
        let m2 = repo.store().get_task("M2").unwrap();
        assert_eq!(m2.status, State::NeedsAnswer);
        assert_eq!(m2.reason, "depends on skipped M1 — ask the developer");
    }

    #[test]
    fn a_worker_whose_milestone_is_no_longer_in_progress_is_freed() {
        let repo = Repo::new(BASE);
        let rt = FakeHerdr::new();
        repo.sync(&rt);
        {
            let mut s = repo.store();
            s.assign_worker("M1", Some("claude-1"), None).unwrap();
            s.transition("M1", &TransitionRequest::auto(State::Coding, None))
                .unwrap();
            s.transition(
                "M1",
                &TransitionRequest::auto(State::Paused, Some("stopped")),
            )
            .unwrap();
        }
        let (_, out) = repo.sync(&rt);
        assert!(
            out.contains("WARN: claude-1 was holding M1, which is no longer in progress — freed\n"),
            "{out}"
        );
        let m1 = repo.store().get_task("M1").unwrap();
        assert_eq!(m1.worker, None);
        assert_eq!(m1.last_worker.as_deref(), Some("claude-1"));
        // Nothing to free the second time.
        let (_, out) = repo.sync(&rt);
        assert!(!out.contains("freed"), "{out}");
    }

    #[test]
    fn herdr_detection_and_stale_panes_are_reported() {
        let user = tempfile::tempdir().unwrap();
        let f = user.path().join("agents.toml");
        std::fs::write(
            &f,
            "[nodetect]\nbinary = \"nodetect\"\nlaunch = \"nodetect\"\nherdr_detect = false\nroles = [\"coder\"]\n",
        )
        .unwrap();
        let registry = agents::registry(agents::DEFAULT_TOML, &f).unwrap();
        let plan = edit(&[(
            "Workers: claude-1 (claude: sonnet), codex-1 (codex: gpt-6-sol)",
            "Workers: claude-1 (claude: sonnet), codex-1 (codex: gpt-6-sol @pane:w1:p9), nd-1 (nodetect), old-1 (claude: sonnet @pane:w1:p7)",
        )]);
        let repo = Repo::new(&plan);
        let rt = FakeHerdr::new();
        rt.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        rt.add_pane(PaneInfo::new("w1:p9", "w1", "w1:t1"));
        rt.add_agent(AgentInfo::new(
            "claude-1",
            "claude",
            "w1:p1",
            AgentStatus::Idle,
        ));
        let mut out = Vec::new();
        let code = run_sync(&repo.root, &rt, &registry, &cat(), 0, &mut out);
        let out = String::from_utf8(out).unwrap();
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("WORKER claude-1 kind=claude herdr=found\n"),
            "{out}"
        );
        assert!(
            out.contains("WORKER codex-1 kind=codex herdr=pane w1:p9 detect:off\n"),
            "{out}"
        );
        assert!(
            out.contains("WORKER nd-1 kind=nodetect herdr=detect:off (relying on its done file)\n"),
            "{out}"
        );
        assert!(
            out.contains("WORKER old-1 kind=claude herdr=STALE pane w1:p7 — find its current id with `herdr pane list` and fix plan.md, then `orch sync`\n"),
            "{out}"
        );
        assert!(
            out.contains("REVIEWER rev kind=claude herdr=NOT FOUND\n"),
            "{out}"
        );
        assert!(
            out.trim_end().lines().last().unwrap().starts_with("OK: "),
            "{out}"
        );
        // A pane-addressed agent is stored with detection off.
        let run = repo.store().load_run().unwrap().unwrap();
        let by = |n: &str| run.agents.iter().find(|a| a.name == n).unwrap().clone();
        assert!(!by("codex-1").detect && by("codex-1").pane.as_deref() == Some("w1:p9"));
        assert!(!by("nd-1").detect);
        assert!(by("claude-1").detect);
    }
}
