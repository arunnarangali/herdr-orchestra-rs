//! Handlers for `orch team` and the `team up` engine (§5). `orch team <kind>` renders
//! Python's `cmd_team`; the `team up` engine opens a herdr pane per missing agent
//! through the `RuntimeAdapter`. `orch team up` itself stays a stub (returning `None`,
//! which the caller reports as "not implemented", exit 70) until Phase 5 can hand the
//! engine the plan's team. `args` are the words after the command (for `team up`:
//! after `up`).

use orch_config::agents::{self, Registry};
use orch_config::launch::{launch_command, repr, shell_quote};
use orch_config::models::Catalog;
use orch_config::team::render_team;
use orch_herdr::RuntimeAdapter;
use orch_herdr::adapter::{SplitDirection, SplitPane};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::listing::die;

pub const TEAM_USAGE: &str = "usage: orch team <kind> [--coders N] [--models a,b] [--no-reviewer]  |  \
orch team up [--dry-run]";

/// A failed load: `ERROR: msg` on stderr with the error's exit code.
fn fail(e: orch_core::Error) -> i32 {
    eprintln!("ERROR: {e}");
    e.exit_code()
}

/// `orch team <kind> [--coders N] [--models a,b] [--no-reviewer]`.
pub fn team(args: &[String]) -> Option<i32> {
    let parsed = match parse_team_args(args) {
        Ok(p) => p,
        Err(e) => return Some(die(e)),
    };
    let reg = match agents::load(None) {
        Ok(r) => r,
        Err(e) => return Some(fail(e)),
    };
    let cat = match orch_config::models::load(None) {
        Ok(c) => c,
        Err(e) => return Some(fail(e)),
    };
    match render_team(
        &reg,
        &cat,
        &parsed.kind,
        parsed.coders,
        parsed.models,
        parsed.reviewer,
    ) {
        Ok(out) => {
            for w in &out.warnings {
                eprintln!("{w}");
            }
            print!("{}", out.stdout);
            Some(0)
        }
        Err(e) => Some(die(e)),
    }
}

/// The parsed words after `team`.
struct ParsedTeam {
    kind: String,
    coders: Option<usize>,
    models: Option<Vec<String>>,
    reviewer: bool,
}

/// The words after `team`, as Python's `cmd_team` reads them: at most one kind and the
/// three flags. Errors are Python's `die` messages.
fn parse_team_args(args: &[String]) -> std::result::Result<ParsedTeam, String> {
    let mut rest: Vec<&String> = args.iter().collect();
    let mut reviewer = true;
    if let Some(i) = rest.iter().position(|a| a.as_str() == "--no-reviewer") {
        rest.remove(i);
        reviewer = false;
    }
    let mut coders = None;
    let mut models = None;
    for flag in ["--coders", "--models"] {
        let Some(i) = rest.iter().position(|a| a.as_str() == flag) else {
            continue;
        };
        let Some(val) = rest.get(i + 1).map(|v| (*v).clone()) else {
            return Err(TEAM_USAGE.to_string());
        };
        rest.drain(i..=i + 1);
        if flag == "--coders" {
            match val
                .chars()
                .all(|c| c.is_ascii_digit())
                .then(|| val.parse().ok())
                .flatten()
            {
                Some(n) if n >= 1 => coders = Some(n),
                _ => return Err("--coders needs a number from 1".to_string()),
            }
        } else {
            models = Some(
                val.split(',')
                    .map(str::trim)
                    .filter(|m| !m.is_empty())
                    .map(str::to_lowercase)
                    .collect(),
            );
        }
    }
    if rest.len() != 1 || rest[0].starts_with('-') {
        return Err(TEAM_USAGE.to_string());
    }
    Ok(ParsedTeam {
        kind: rest[0].to_lowercase(),
        coders,
        models,
        reviewer,
    })
}

/// `orch team up`: still a stub — Phase 5 hands the engine the plan's team.
pub fn team_up(_args: &[String]) -> Option<i32> {
    None
}

/// One agent of a team: what Phase 5 will build from the parsed plan.
// Phase 5's `orch team up` handler calls the engine; until then nothing in this
// binary reaches it, and the dead-code lint would fire on the only copy of it.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamAgent {
    /// The agent's plan name (`codex-1`, `reviewer`).
    pub name: String,
    /// Its agent kind.
    pub kind: String,
    /// The model to start it on, when the plan gives one.
    pub model: Option<String>,
    /// The folder the pane starts in (its worktree, or the repo).
    pub folder: PathBuf,
}

/// The engine's input: a team plus the pane to split from.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct TeamUpPlan {
    /// Every worker and the reviewer, in plan order.
    pub agents: Vec<TeamAgent>,
    /// The pane the first split comes from (the lead's pane).
    pub split_from: String,
    /// Report the actions instead of taking them.
    pub dry_run: bool,
    /// Seconds a new agent has to become ready (`ORCH_TEAM_WAIT_S`, default 90).
    pub wait_s: f64,
    /// The herdr binary's name, for the `WOULD` lines.
    pub herdr_bin: String,
}

/// One output line: stdout, or a warning for stderr.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A stdout line.
    Out(String),
    /// A stderr line (`WARN: …`).
    Warn(String),
}

/// What the engine did: its lines in order, and how many agents are not ready.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamUpReport {
    /// The lines, in the order Python's `team_up` prints them.
    pub lines: Vec<Line>,
    /// Agents that ended `WAITING` or `FAILED` (or `SKIP`).
    pub failed: u32,
    /// Every agent was already running, so nothing was opened.
    pub all_running: bool,
}

impl TeamUpReport {
    /// The command's exit code, as Python's `team_up`: 0, or 3 when agents are not
    /// ready (never in a dry run).
    #[allow(dead_code)]
    pub fn exit_code(&self, dry_run: bool) -> i32 {
        if !dry_run && self.failed > 0 { 3 } else { 0 }
    }
}

/// The `team up` engine: for every agent herdr cannot find, split a pane from
/// `split_from` (from the previously opened pane for the later ones), in the agent's
/// folder, run the launch command for its kind and model, and name it. Agents herdr
/// already finds are left alone, so a second run opens nothing new.
///
/// `sleep` waits between rename attempts (real sleeps in production; tests pass a
/// no-op) so the WAITING timeout of `wait_s` stays testable.
#[allow(dead_code)]
pub fn run_team_up(
    runtime: &dyn RuntimeAdapter,
    reg: &Registry,
    cat: &Catalog,
    plan: &TeamUpPlan,
    sleep: &mut dyn FnMut(Duration),
) -> TeamUpReport {
    let mut lines = Vec::new();
    let mut todo = Vec::new();
    for a in &plan.agents {
        if runtime.agent(&a.name).is_ok() {
            lines.push(Line::Out(format!("ok      {}: already running", a.name)));
        } else {
            todo.push(a);
        }
    }
    if todo.is_empty() {
        lines.push(Line::Out(
            "TEAM UP: every agent in the plan is running".to_string(),
        ));
        return TeamUpReport {
            lines,
            failed: 0,
            all_running: true,
        };
    }
    let mut failed = 0;
    let mut last: Option<String> = None;
    for a in todo {
        let kind = &a.kind;
        let Some(v) = reg.get(kind) else {
            lines.push(Line::Out(format!(
                "SKIP    {}: kind {} is not in the agent registry — start it yourself in {}",
                a.name,
                repr(kind),
                a.folder.display()
            )));
            failed += 1;
            continue;
        };
        let cmd = match launch_command(reg, cat, kind, a.model.as_deref(), false) {
            Ok(l) => {
                for w in l.warnings {
                    lines.push(Line::Warn(w));
                }
                l.command
            }
            Err(e) => {
                lines.push(Line::Out(format!(
                    "FAILED  {}: herdr could not open a pane ({e})",
                    a.name
                )));
                failed += 1;
                continue;
            }
        };
        let from = last.as_deref().unwrap_or(&plan.split_from);
        let direction = if last.is_some() {
            SplitDirection::Down
        } else {
            SplitDirection::Right
        };
        let req = SplitPane {
            from_pane: Some(from.to_string()),
            direction,
            ratio: None,
            cwd: Some(a.folder.clone()),
            focus: false,
        };
        if plan.dry_run {
            lines.push(Line::Out(format!(
                "WOULD   {}: {} pane split {} --direction {} --cwd {} --no-focus  then  herdr pane run \
<new pane> {}  then  herdr agent rename <new pane> {}",
                a.name,
                plan.herdr_bin,
                shell_quote(from),
                if last.is_some() { "down" } else { "right" },
                shell_quote(&a.folder.display().to_string()),
                shell_quote(&cmd),
                a.name
            )));
            continue;
        }
        let pane = match runtime.split_pane(&req) {
            Ok(p) => p,
            Err(e) => {
                lines.push(Line::Out(format!(
                    "FAILED  {}: herdr could not open a pane ({e})",
                    a.name
                )));
                failed += 1;
                continue;
            }
        };
        let pid = pane.pane_id.clone();
        last = Some(pid.clone());
        if let Err(e) = runtime.run_command(&pid, &cmd) {
            let msg = e.to_string();
            let tail: String = msg
                .chars()
                .rev()
                .take(120)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            lines.push(Line::Out(format!(
                "FAILED  {}: pane {pid} opened, but `herdr pane run` failed ({tail})",
                a.name
            )));
            failed += 1;
            continue;
        }
        if a.model.is_some() && v.model_flag.as_deref().is_none_or(str::is_empty) {
            lines.push(Line::Out(format!(
                "NOTE    {}: {} has no model option — switch pane {pid} to {} inside {kind}",
                a.name,
                a.kind,
                a.model.as_deref().unwrap_or_default()
            )));
        }
        if !v.herdr_detect {
            let _ = runtime.rename_agent(&pid, &a.name);
            lines.push(Line::Out(format!(
                "OPENED  {}: pane {pid} in {} — herdr can't watch {}: write it as \
`{} ({} @pane:{pid})` in plan.md, then `orch sync`",
                a.name,
                short(&a.folder),
                a.kind,
                a.name,
                a.kind
            )));
            continue;
        }
        let step = (plan.wait_s / 30.0).clamp(0.2, 2.0);
        let tries = if plan.wait_s > 0.0 {
            (plan.wait_s / step).ceil() as u32 + 1
        } else {
            1
        };
        let mut named = false;
        for attempt in 0..tries {
            if attempt > 0 {
                sleep(Duration::from_secs_f64(step));
            }
            if runtime.rename_agent(&pid, &a.name).is_ok() {
                named = true;
                break;
            }
        }
        if named {
            lines.push(Line::Out(format!(
                "OPENED  {}: pane {pid} in {} running `{cmd}`",
                a.name,
                short(&a.folder)
            )));
        } else {
            lines.push(Line::Out(format!(
                "WAITING {}: pane {pid} runs `{cmd}`, but herdr has not seen the agent start yet — if it \
shows a first-run question (trust this folder, sign in), answer it, then: herdr agent rename \
{pid} {}",
                a.name, a.name
            )));
            failed += 1;
        }
    }
    lines.push(Line::Out(if failed == 0 {
        "TEAM UP: done — run `orch start` again; every agent should show herdr=found".to_string()
    } else {
        format!("TEAM UP: {failed} agent(s) not ready — see above, then `orch team up` again")
    }));
    TeamUpReport {
        lines,
        failed,
        all_running: false,
    }
}

/// `~/path` for display (Python's `short`).
#[allow(dead_code)]
fn short(p: &Path) -> String {
    let p = p.display().to_string();
    match std::env::var_os("HOME").map(std::path::PathBuf::from) {
        Some(home) if p.starts_with(&home.display().to_string()) => {
            format!("~{}", &p[home.display().to_string().len()..])
        }
        _ => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orch_config::agents;
    use orch_config::models;
    use orch_herdr::fake::{CallKind, FakeHerdr};
    use orch_herdr::model::{AgentInfo, PaneInfo};
    use orch_herdr::status::AgentStatus;
    use std::path::Path;

    fn none() -> PathBuf {
        PathBuf::from("/nonexistent/m3")
    }

    fn reg() -> Registry {
        agents::registry(agents::DEFAULT_TOML, &none()).unwrap()
    }

    fn cat() -> Catalog {
        models::catalog(models::DEFAULT_TOML, &none()).unwrap()
    }

    fn out(lines: Vec<Line>) -> Vec<String> {
        lines
            .into_iter()
            .filter_map(|l| match l {
                Line::Out(s) => Some(s),
                Line::Warn(_) => None,
            })
            .collect()
    }

    fn no_sleep(_: Duration) {}

    fn agent(name: &str, kind: &str, model: Option<&str>, folder: &str) -> TeamAgent {
        TeamAgent {
            name: name.to_string(),
            kind: kind.to_string(),
            model: model.map(str::to_string),
            folder: PathBuf::from(folder),
        }
    }

    fn plan(agents: Vec<TeamAgent>, dry: bool) -> TeamUpPlan {
        TeamUpPlan {
            agents,
            split_from: "w1:p1".to_string(),
            dry_run: dry,
            wait_s: 90.0,
            herdr_bin: "herdr".to_string(),
        }
    }

    #[test]
    fn a_fully_running_team_is_left_alone() {
        let fake = FakeHerdr::new();
        fake.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        fake.add_agent(AgentInfo::new(
            "claude-1",
            "claude",
            "w1:p1",
            AgentStatus::Idle,
        ));
        fake.add_agent(AgentInfo::new(
            "reviewer",
            "claude",
            "w1:p1",
            AgentStatus::Working,
        ));
        let r = run_team_up(
            &fake,
            &reg(),
            &cat(),
            &plan(
                vec![agent("claude-1", "claude", Some("opus"), "/repo/wt")],
                false,
            ),
            &mut no_sleep,
        );
        assert_eq!(
            out(r.lines),
            [
                "ok      claude-1: already running",
                "TEAM UP: every agent in the plan is running",
            ]
        );
        assert!(r.all_running);
        assert_eq!(r.failed, 0);
        assert_eq!(fake.panes(None).unwrap().len(), 1, "no new pane");
        assert!(fake.sent().is_empty());
    }

    #[test]
    fn a_missing_agent_gets_a_pane_its_command_and_its_name() {
        let fake = FakeHerdr::new();
        fake.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        // herdr sees the agent in the new pane once it starts: the fake keys it by the
        // pane id the split will create.
        fake.add_agent(AgentInfo::new(
            "w1:p3",
            "claude",
            "w1:p3",
            AgentStatus::Idle,
        ));
        let r = run_team_up(
            &fake,
            &reg(),
            &cat(),
            &plan(
                vec![agent("claude-1", "claude", Some("opus"), "/repo/wt-1")],
                false,
            ),
            &mut no_sleep,
        );
        assert_eq!(
            out(r.lines),
            [
                "OPENED  claude-1: pane w1:p3 in /repo/wt-1 running \
`claude --permission-mode acceptEdits --model opus`",
                "TEAM UP: done — run `orch start` again; every agent should show herdr=found",
            ]
        );
        assert!(!r.all_running);
        assert_eq!(r.failed, 0);
        assert_eq!(
            fake.sent(),
            vec![orch_herdr::fake::Sent::Run {
                pane: "w1:p3".into(),
                command: "claude --permission-mode acceptEdits --model opus".into(),
            }]
        );
        let pane = fake.pane("w1:p3").unwrap();
        assert_eq!(pane.cwd.as_deref(), Some("/repo/wt-1"));
        assert_eq!(fake.agent("claude-1").unwrap().pane_id, "w1:p3");
        // A second run opens nothing new: the agent is already there.
        let r2 = run_team_up(
            &fake,
            &reg(),
            &cat(),
            &plan(
                vec![agent("claude-1", "claude", Some("opus"), "/repo/wt-1")],
                false,
            ),
            &mut no_sleep,
        );
        assert_eq!(out(r2.lines)[0], "ok      claude-1: already running");
        assert!(r2.all_running);
        assert_eq!(fake.panes(None).unwrap().len(), 2);
        assert_eq!(fake.sent().len(), 1);
    }

    #[test]
    fn a_pane_that_stays_unnamed_reports_waiting() {
        let fake = FakeHerdr::new();
        fake.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        let r = run_team_up(
            &fake,
            &reg(),
            &cat(),
            &plan(vec![agent("claude-1", "claude", None, "/repo/wt")], false),
            &mut no_sleep,
        );
        assert_eq!(
            out(r.lines.clone()),
            [
                "WAITING claude-1: pane w1:p3 runs `claude --permission-mode acceptEdits`, but herdr has \
not seen the agent start yet — if it shows a first-run question (trust this folder, sign in), \
answer it, then: herdr agent rename w1:p3 claude-1",
                "TEAM UP: 1 agent(s) not ready — see above, then `orch team up` again",
            ]
        );
        assert_eq!(r.failed, 1);
        assert_eq!(r.exit_code(false), 3);
    }

    #[test]
    fn a_split_failure_reports_failed_with_herdrs_message() {
        let fake = FakeHerdr::new();
        fake.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        fake.fail_next(
            CallKind::PaneSplit,
            1,
            orch_herdr::HerdrError::StalePane {
                pane: "w1:p1".into(),
                message: "STALE pane w1:p1".into(),
            },
        );
        let r = run_team_up(
            &fake,
            &reg(),
            &cat(),
            &plan(vec![agent("claude-1", "claude", None, "/repo/wt")], false),
            &mut no_sleep,
        );
        assert_eq!(
            out(r.lines)[0],
            "FAILED  claude-1: herdr could not open a pane (stale or unknown pane \"w1:p1\": STALE pane \
w1:p1)"
        );
        assert_eq!(r.failed, 1);
    }

    #[test]
    fn a_dry_run_prints_would_lines_and_changes_nothing() {
        let fake = FakeHerdr::new();
        fake.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        let r = run_team_up(
            &fake,
            &reg(),
            &cat(),
            &plan(
                vec![
                    agent("codex-1", "codex", Some("gpt-6-sol"), "/repo/wt-1"),
                    agent("codex-2", "codex", Some("gpt-6-luna"), "/repo/wt-2"),
                ],
                true,
            ),
            &mut no_sleep,
        );
        assert_eq!(
            out(r.lines.clone()),
            [
                "WOULD   codex-1: herdr pane split w1:p1 --direction right --cwd /repo/wt-1 --no-focus  \
then  herdr pane run <new pane> 'codex --approve-for-me --model gpt-6-sol'  then  herdr agent \
rename <new pane> codex-1",
                "WOULD   codex-2: herdr pane split w1:p1 --direction right --cwd /repo/wt-2 --no-focus  \
then  herdr pane run <new pane> 'codex --approve-for-me --model gpt-6-luna'  then  herdr agent \
rename <new pane> codex-2",
                "TEAM UP: done — run `orch start` again; every agent should show herdr=found",
            ]
        );
        assert!(fake.sent().is_empty());
        assert_eq!(fake.panes(None).unwrap().len(), 1);
        assert_eq!(r.exit_code(true), 0);
    }

    #[test]
    fn a_kind_missing_from_the_registry_is_skipped() {
        let fake = FakeHerdr::new();
        fake.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        let r = run_team_up(
            &fake,
            &reg(),
            &cat(),
            &plan(vec![agent("mystery-1", "mystery", None, "/repo/wt")], false),
            &mut no_sleep,
        );
        assert_eq!(
            out(r.lines)[0],
            "SKIP    mystery-1: kind 'mystery' is not in the agent registry — start it yourself in \
/repo/wt"
        );
        assert_eq!(r.failed, 1);
    }

    #[test]
    fn an_unwatchable_kind_is_opened_and_named_once() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("agents.toml");
        std::fs::write(
            &p,
            "[solo]\nbinary = \"solo\"\nlaunch = \"solo\"\nmodel_flag = \"-m\"\nherdr_detect = false\nroles = \
[\"coder\"]\n",
        )
        .unwrap();
        let r0 = agents::registry(agents::DEFAULT_TOML, &p).unwrap();
        let fake = FakeHerdr::new();
        fake.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        let r = run_team_up(
            &fake,
            &r0,
            &cat(),
            &plan(vec![agent("solo-1", "solo", Some("m1"), "/repo/wt")], false),
            &mut no_sleep,
        );
        assert_eq!(
            out(r.lines)[0],
            "OPENED  solo-1: pane w1:p3 in /repo/wt — herdr can't watch solo: write it as \
`solo-1 (solo @pane:w1:p3)` in plan.md, then `orch sync`"
        );
        assert_eq!(r.failed, 0);
        assert_eq!(
            fake.sent(),
            vec![orch_herdr::fake::Sent::Run {
                pane: "w1:p3".into(),
                command: "solo -m m1".into(),
            }]
        );
    }

    #[test]
    fn a_kind_without_a_model_option_gets_the_note_and_its_default_model() {
        let fake = FakeHerdr::new();
        fake.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        let r = run_team_up(
            &fake,
            &reg(),
            &cat(),
            &plan(vec![agent("amp-1", "amp", Some("m1"), "/repo/wt")], false),
            &mut no_sleep,
        );
        let lines = out(r.lines);
        assert_eq!(
            lines[0],
            "NOTE    amp-1: amp has no model option — switch pane w1:p3 to m1 inside amp"
        );
        assert!(
            lines[1].starts_with("WAITING amp-1: pane w1:p3 runs `amp`,"),
            "{}",
            lines[1]
        );
    }

    #[test]
    fn the_later_agents_split_down_from_the_pane_before() {
        let fake = FakeHerdr::new();
        fake.add_pane(PaneInfo::new("w1:p1", "w1", "w1:t1"));
        fake.add_agent(AgentInfo::new(
            "w1:p3",
            "claude",
            "w1:p3",
            AgentStatus::Idle,
        ));
        fake.add_agent(AgentInfo::new(
            "w1:p5",
            "claude",
            "w1:p5",
            AgentStatus::Idle,
        ));
        run_team_up(
            &fake,
            &reg(),
            &cat(),
            &plan(
                vec![
                    agent("claude-1", "claude", None, "/repo/wt-1"),
                    agent("claude-2", "claude", None, "/repo/wt-2"),
                ],
                false,
            ),
            &mut no_sleep,
        );
        assert_eq!(
            fake.pane("w1:p3").unwrap().cwd.as_deref(),
            Some("/repo/wt-1")
        );
        assert_eq!(
            fake.pane("w1:p5").unwrap().cwd.as_deref(),
            Some("/repo/wt-2")
        );
        assert_eq!(fake.agent("claude-2").unwrap().pane_id, "w1:p5");
    }

    #[test]
    fn short_shortens_the_home_prefix() {
        unsafe { std::env::set_var("HOME", "/home/dev") };
        assert_eq!(short(Path::new("/home/dev/repo/wt")), "~/repo/wt");
        assert_eq!(short(Path::new("/srv/other")), "/srv/other");
        unsafe { std::env::remove_var("HOME") };
        assert_eq!(short(Path::new("/srv/other")), "/srv/other");
    }
}
