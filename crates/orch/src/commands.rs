//! The command table: every command name, its help line, and the usage text.

/// Exit code for a command that exists but is not built yet. Python uses 1, 2, 3, 4, 5, 6, 7
/// and 99, and `orch poll` waits with 10, so 70 (EX_SOFTWARE) is free.
pub const EXIT_NOT_IMPLEMENTED: i32 = 70;

/// Every top-level command name (the Python v4.10.1 `COMMANDS` map plus the no-repo
/// commands `lead`, `agents`, `doctor`, `models`, `version`).
pub const COMMAND_NAMES: &[&str] = &[
    "sync",
    "start",
    "ready",
    "pick",
    "prepare",
    "setup-done",
    "send",
    "review",
    "poll",
    "bump",
    "commit",
    "merge",
    "park",
    "cooldown",
    "set",
    "status",
    "cleanup",
    "write-task",
    "nudge",
    "autopilot",
    "send-job",
    "handoff",
    "needs-fix",
    "review-ok",
    "requeue-review",
    "finish",
    "resend",
    "approve",
    "report",
    "events",
    "dispatch",
    "answer",
    "escalate",
    "fix",
    "team",
    "lead",
    "agents",
    "doctor",
    "models",
    "version",
];

/// The variables a Hermes-style agent shell must pass through to see its pane.
const HERDR_PANE_ENV: [&str; 4] = ["HERDR_ENV", "HERDR_PANE_ID", "HERDR_SOCKET_PATH", "HERDR_BIN_PATH"];

/// How long `lead check` waits for `herdr pane current`, as Python does.
const LEAD_CHECK_TIMEOUT_S: u64 = 20;

/// Run `<herdr> pane current` bounded by a timeout: `Ok(detail)` when it succeeds,
/// `Err(detail)` when it fails, cannot start or hangs (the child is killed).
fn pane_current(herdr: &str, timeout_s: u64) -> Result<String, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new(herdr)
        .args(["pane", "current"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let reader = |p: Option<&mut dyn Read>| {
        let mut s = String::new();
        if let Some(p) = p {
            let _ = p.read_to_string(&mut s);
        }
        s
    };
    let out_t = std::thread::spawn(move || reader(out.as_mut().map(|p| p as &mut dyn Read)));
    let err_t = std::thread::spawn(move || reader(err.as_mut().map(|p| p as &mut dyn Read)));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_s);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {timeout_s}s"));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let text = format!("{}{}", out_t.join().unwrap_or_default(), err_t.join().unwrap_or_default());
    if status.success() { Ok(text.trim().to_string()) } else { Err(text.trim().to_string()) }
}

/// `orch lead check`: preflight, is this agent driving Herdr? `HERDR_ENV=1` is the fast
/// path; otherwise ask herdr. Returns the exit code (0 ok, 2 not detected, as Python's `die`).
pub fn lead_check(herdr: &str) -> i32 {
    let fix = format!(
        "If the agent is Hermes, its tools run in the Hermes gateway, which does not see the pane's env. \
Add this to ~/.hermes/config.yaml, then restart Hermes:\n    terminal:\n      client_env_passthrough: [{}]",
        HERDR_PANE_ENV.join(", ")
    );
    if std::env::var("HERDR_ENV").is_ok_and(|v| v == "1") {
        let pane = std::env::var("HERDR_PANE_ID").ok().filter(|p| !p.is_empty());
        println!("OK inside a Herdr pane (HERDR_ENV=1, pane {})", pane.as_deref().unwrap_or("?"));
        return 0;
    }
    match pane_current(herdr, LEAD_CHECK_TIMEOUT_S) {
        Ok(_) => {
            println!(
                "OK herdr is reachable, but this shell has no HERDR_ENV, so herdr cannot tell which pane is yours.\n  \
Carry on; `orch lead whoami` will show the focused pane — confirm it is yours.\n  {fix}"
            );
            0
        }
        Err(detail) => {
            let last = detail.lines().last().map(|l| format!(" ({l})")).unwrap_or_default();
            eprintln!(
                "ERROR: Herdr not detected: HERDR_ENV is not 1 and `herdr pane current` failed{last}.\n  \
Start this agent from a Herdr pane, and make sure `herdr` is on PATH (or set HERDR=/path/to/herdr).\n  {fix}"
            );
            2
        }
    }
}

/// Usage lines: (invocation, help). The first word of the invocation is the command name.
pub const HELP: &[(&str, &str)] = &[
    ("version", "print the installed herdr-orchestra version"),
    (
        "lead plan|run",
        "print the lead playbook (any agent can lead)",
    ),
    (
        "lead check",
        "preflight: is this agent in a Herdr pane / can it reach herdr?",
    ),
    (
        "lead whoami",
        "show the lead's own herdr pane and how to name it `lead`",
    ),
    (
        "team <kind> [--coders N] [--models a,b] [--no-reviewer]",
        "the plan's team built from one installed tool",
    ),
    (
        "team up [--dry-run]",
        "open a herdr pane for every agent in the plan herdr can't find",
    ),
    (
        "agents",
        "the agent registry: kinds, installed?, roles, herdr detection, launch commands",
    ),
    (
        "agents launch <kind> [--model <id>]",
        "print the command that starts <kind> in a pane",
    ),
    (
        "models",
        "the model catalog: tiers, ratings, and what policy forbids",
    ),
    (
        "models show|check <id>",
        "one model's ratings and evidence / is it allowed?",
    ),
    (
        "events [--peek]",
        "(lead) what you were woken for; reading acknowledges it",
    ),
    (
        "dispatch <Mn>... [--to <worker>] [--tier <tier>]",
        "(lead) start or restart milestones; starts the event manager",
    ),
    (
        "answer <Mn> <text>",
        "(lead) the developer's answer: into plan.md, and back to the same coder",
    ),
    (
        "fix <Mn> <text> [--to <worker>]",
        "a change to something Mn already landed: a small follow-up milestone",
    ),
    (
        "agents sync [kind...]",
        "install the orch-plan / orch-run skills for every installed agent",
    ),
    (
        "agents check",
        "check the orch-plan / orch-run skills are installed for every installed agent",
    ),
    (
        "doctor [kind...]",
        "check installed agents against the real tool: version, launch flags, skills, herdr",
    ),
    (
        "sync",
        "parse .orchestrate/plan.md -> state.json (validates deps, cycles, overlaps)",
    ),
    (
        "start",
        "create/switch to the feature branch; create worker worktrees",
    ),
    (
        "status",
        "table of workers + tasks; prints ALL_DONE or STUCK when relevant",
    ),
    (
        "report [--json]",
        "time, rounds, first-pass rate, wasted work",
    ),
    ("ready", "free coder slots, FIX jobs, then READY milestones"),
    (
        "pick <Mn>",
        "first available worker for the task, or NONE (exit 3)",
    ),
    (
        "escalate <Mn> [reason]",
        "out of rounds: one more go on a stronger model",
    ),
    (
        "prepare <Mn> <worker>",
        "reserve worker, put its worktree on the milestone's saved work",
    ),
    (
        "setup-done <worker>",
        "mark the worker's worktree as dependency-installed",
    ),
    (
        "send <Mn> <file>",
        "prompt the task's worker to execute TASKDIR/<file>",
    ),
    (
        "poll [secs]",
        "wait for the next event: DONE / REVIEWED / BLOCKED / IDLE / TIMEOUT / GONE / ORPHAN",
    ),
    (
        "bump <Mn> fix|review",
        "count a fix/review round; exit 3 with CAP when the limit is hit",
    ),
    (
        "review <Mn>",
        "snapshot the diff and send it to the reviewer",
    ),
    ("commit <Mn>", "save the task's work"),
    (
        "merge <Mn>",
        "squash the milestone, run the check, commit it ONLY if green",
    ),
    (
        "park <Mn> paused|failed|pending [--lost] [reason]",
        "free the worker, keep its work in a hidden ref",
    ),
    (
        "cooldown <agent> <min> [reason]",
        "bench a worker or the reviewer",
    ),
    (
        "set <Mn> <status> [reason] [--force]",
        "manual override of a milestone status",
    ),
    (
        "approve <Mn>",
        "let a milestone held for risky changes land",
    ),
    ("cleanup", "remove clean worker worktrees"),
    (
        "finish [--no-push]",
        "final check, drop hidden refs + worktrees, push the feature branch, open a PR",
    ),
    ("write-task <Mn>", "generate TASKDIR/task.md from plan.md"),
    (
        "nudge <Mn>",
        "re-send the current prompt once to an agent that stopped early",
    ),
    (
        "autopilot",
        "the event manager: watches agents, checks, merges, wakes the lead",
    ),
    (
        "handoff <Mn>",
        "(worktree) save the milestone for review and free its coder",
    ),
    (
        "needs-fix <Mn>",
        "queue the latest review's BLOCKERs as a fix job",
    ),
    (
        "send-job <Mn>",
        "send a queued fix/conflict job to the task's coder",
    ),
    ("review-ok <Mn>", "record that the reviewer approved Mn"),
    (
        "requeue-review <Mn>",
        "the reviewer vanished mid-review: queue the same round again",
    ),
    (
        "resend <Mn>",
        "re-send a recorded but unconfirmed prompt to the same agent",
    ),
];

const FOOTER: &str = "\
Exit codes: 2 error · 3 needs you / busy / cap · 4 merge conflict · 5 prompt not delivered ·
            6 check fails on the feature branch · 7 held for approval · 10 poll wait ·
            70 command not implemented in this build
Environment: ORCH_LOG=error|warn|info|debug|trace sets the log level (stderr; off by default)
";

/// The full usage text.
pub fn usage() -> String {
    let mut s = String::from("usage: orch <command> [args]\n");
    for (inv, help) in HELP {
        s.push_str(&format!("  {inv:<44} {help}\n"));
    }
    s.push('\n');
    s.push_str(FOOTER);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_command_list() {
        let expected = [
            "sync",
            "start",
            "ready",
            "pick",
            "prepare",
            "setup-done",
            "send",
            "review",
            "poll",
            "bump",
            "commit",
            "merge",
            "park",
            "cooldown",
            "set",
            "status",
            "cleanup",
            "write-task",
            "nudge",
            "autopilot",
            "send-job",
            "handoff",
            "needs-fix",
            "review-ok",
            "requeue-review",
            "finish",
            "resend",
            "approve",
            "report",
            "events",
            "dispatch",
            "answer",
            "escalate",
            "fix",
            "team",
            "lead",
            "agents",
            "doctor",
            "models",
            "version",
        ];
        let mut got = COMMAND_NAMES.to_vec();
        let mut want = expected.to_vec();
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(got, want);
    }

    #[test]
    fn every_command_has_a_help_line() {
        let u = usage();
        for name in COMMAND_NAMES {
            assert!(
                HELP.iter()
                    .any(|(inv, _)| inv.split_whitespace().next() == Some(name)),
                "no help line for {name}"
            );
            assert!(u.contains(name));
        }
    }

    #[test]
    fn subcommands_have_help() {
        for sub in [
            "lead plan|run",
            "lead check",
            "lead whoami",
            "agents launch",
            "agents sync",
            "agents check",
            "models show|check",
            "team up",
            "doctor",
        ] {
            assert!(
                HELP.iter().any(|(inv, _)| inv.starts_with(sub)),
                "missing {sub}"
            );
        }
    }

    #[test]
    fn exit_code_is_unused_by_python() {
        for c in [0, 1, 2, 3, 4, 5, 6, 7, 10, 99] {
            assert_ne!(EXIT_NOT_IMPLEMENTED, c);
        }
    }
}
