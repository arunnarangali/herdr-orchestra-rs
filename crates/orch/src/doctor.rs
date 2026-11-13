//! `orch doctor`, `orch agents check` and `orch agents sync` (Python's `cmd_doctor`,
//! `cmd_agents check` and `agents_sync`, with authentication added). Herdr is reached
//! only through the `RuntimeAdapter`. Each handler returns `Some(exit code)`.

use crate::listing::die;
use orch_config::agents::{self, AgentKind, Registry};
use orch_config::discovery::skills::{self, AssetState, home, short};
use orch_config::discovery::{self, AuthState, FlagResult};
use orch_config::tunables::Tunables;
use orch_herdr::RuntimeAdapter;
use orch_herdr::cli::CliHerdr;
use std::path::PathBuf;

const DEFAULT_INVOKE: &str = "Run `orch lead plan` and follow it. Feature: <feature>";

fn herdr() -> CliHerdr {
    match Tunables::from_env() {
        Ok(t) => CliHerdr::from_tunables(&t),
        Err(_) => CliHerdr::new("herdr", 30),
    }
}

fn load_registry() -> Result<Registry, i32> {
    agents::load(None).map_err(|e| {
        eprintln!("ERROR: {e}");
        e.exit_code()
    })
}

fn unknown_kinds(reg: &Registry, names: &[String]) -> String {
    format!(
        "unknown agent kind {} — known: {} (add it in {})",
        names.join(", "),
        reg.kinds().join(", "),
        agents::user_agents_file().display()
    )
}

/// One line of a doctor report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub ok: bool,
    pub message: String,
}

fn check(ok: bool, message: impl Into<String>) -> Check {
    Check { ok, message: message.into() }
}

/// What herdr says about detectable kinds, asked at most once per run.
enum HerdrKinds {
    Unasked,
    Known(Vec<String>),
    NotReachable,
}

impl HerdrKinds {
    fn get(&mut self, runtime: &dyn RuntimeAdapter) -> Option<&[String]> {
        if matches!(self, HerdrKinds::Unasked) {
            *self = match runtime.agent_kinds() {
                Ok(k) => HerdrKinds::Known(k),
                Err(_) => HerdrKinds::NotReachable,
            };
        }
        match self {
            HerdrKinds::Known(k) => Some(k),
            _ => None,
        }
    }
}

/// The checks for one installed kind, in Python's order: version, launch flags, skills
/// (then authentication), herdr detection.
fn doctor_kind(kind: &str, ak: &AgentKind, runtime: &dyn RuntimeAdapter, herdr_kinds: &mut HerdrKinds) -> Vec<Check> {
    let home = home();
    let timeout = discovery::probe_timeout();
    let mut out = Vec::new();
    out.push(match discovery::version(ak, timeout) {
        Ok(v) => check(true, format!("version: {v}")),
        Err(cmd) => check(true, format!("version: unknown (`{cmd}` failed)")),
    });
    let hcmd = discovery::help_cmd(ak);
    for f in discovery::launch_flags(ak, timeout) {
        out.push(match f.result {
            FlagResult::Listed => check(true, format!("launch flag {} is listed in `{hcmd}`", f.flag)),
            FlagResult::Missing => check(
                false,
                format!(
                    "launch flag {} is NOT in `{hcmd}` — the launch command in the registry may be out of date (override it in {})",
                    f.flag,
                    short(&agents::user_agents_file(), &home)
                ),
            ),
            FlagResult::HelpFailed(why) => check(false, format!("launch flag {}: `{hcmd}` did not run ({why})", f.flag)),
        });
    }
    for a in skills::agent_assets(ak, &home) {
        let what = format!("skill {}", short(&a.path, &home));
        out.push(match a.state {
            AssetState::Current => check(true, what),
            AssetState::Outdated => check(true, format!("{what} is not the current text — run `orch agents sync`")),
            AssetState::Missing => check(false, format!("{what} missing — run `orch agents sync`")),
        });
    }
    let auth = discovery::auth(ak, &home, timeout);
    out.push(match auth.state {
        AuthState::Authenticated => check(true, format!("auth: authenticated ({})", auth.reason)),
        AuthState::Unknown => check(true, format!("auth: unknown ({})", auth.reason)),
        AuthState::NotAuthenticated => check(
            false,
            format!("auth: not authenticated ({}) — log in with the tool itself", auth.reason),
        ),
    });
    if ak.herdr_detect {
        out.push(match herdr_kinds.get(runtime) {
            Some(known) if known.iter().any(|k| k == kind) => check(true, "herdr knows this kind"),
            Some(_) => check(
                false,
                "herdr does not list this kind (`herdr agent`): set herdr_detect = false for it",
            ),
            None => check(true, "herdr not reachable here: detection not checked"),
        });
    }
    out
}

/// `orch doctor [kind...]`: check each installed agent against the real tool. Read-only;
/// exit 1 if any kind is broken.
pub fn doctor(args: &[String]) -> Option<i32> {
    Some(run_doctor(args, &herdr()))
}

pub fn run_doctor(args: &[String], runtime: &dyn RuntimeAdapter) -> i32 {
    let reg = match load_registry() {
        Ok(r) => r,
        Err(c) => return c,
    };
    let home = home();
    let kinds: Vec<String> = if args.is_empty() {
        reg.kinds().iter().map(|k| k.to_string()).collect()
    } else {
        args.iter().map(|a| a.to_lowercase()).collect()
    };
    let (mut broken, mut herdr_kinds) = (0, HerdrKinds::Unasked);
    for k in kinds {
        let Some(ak) = reg.get(&k) else {
            println!(
                "BROKEN  {k}: not in the agent registry (add it in {})",
                short(&agents::user_agents_file(), &home)
            );
            broken += 1;
            continue;
        };
        if !ak.is_installed() {
            println!("skip    {k}: `{}` is not installed", ak.binary);
            continue;
        }
        let res = doctor_kind(&k, ak, runtime, &mut herdr_kinds);
        let bad = res.iter().any(|c| !c.ok);
        broken += i32::from(bad);
        println!(
            "{:<7} {k}{}",
            if bad { "BROKEN" } else { "ok" },
            if ak.verified { "" } else { "  (registry entry unverified)" }
        );
        for c in res {
            println!("    {} {}", if c.ok { "ok " } else { "BAD" }, c.message);
        }
    }
    i32::from(broken > 0)
}

/// `orch agents check`: are the orch skills and commands installed and current for
/// every installed kind?
pub fn agents_check(_args: &[String]) -> Option<i32> {
    Some(run_agents_check())
}

fn run_agents_check() -> i32 {
    let reg = match load_registry() {
        Ok(r) => r,
        Err(c) => return c,
    };
    let home = home();
    let mut bad = 0;
    for (k, ak) in &reg.0 {
        if !ak.is_installed() {
            continue;
        }
        for a in skills::agent_assets(ak, &home) {
            let label = if a.is_skill { "skill   " } else { "command " };
            let word = match a.state {
                AssetState::Current => "ok  ",
                AssetState::Outdated => "OUTDATED",
                AssetState::Missing => "MISSING",
            };
            bad += i32::from(a.state != AssetState::Current);
            println!("{word} {k:<9} {label}{}", a.path.display());
        }
    }
    if bad > 0 {
        println!("{bad} missing or outdated — run: orch agents sync");
        return 1;
    }
    0
}

/// `orch agents sync [kind...]`: copy the orch skills and slash commands into the shared
/// folders and every installed kind's folders. Idempotent; records written paths.
pub fn agents_sync(args: &[String]) -> Option<i32> {
    Some(run_agents_sync(args))
}

struct Row {
    label: String,
    items: Vec<(&'static str, PathBuf, Option<String>)>,
    invoke: String,
}

fn run_agents_sync(args: &[String]) -> i32 {
    let reg = match load_registry() {
        Ok(r) => r,
        Err(c) => return c,
    };
    let home = home();
    let want: Option<Vec<String>> = (!args.is_empty()).then(|| args.iter().map(|a| a.to_lowercase()).collect());
    if let Some(w) = &want {
        let unknown: Vec<String> = {
            let mut u: Vec<String> = w.iter().filter(|k| reg.get(k).is_none()).cloned().collect();
            u.sort();
            u.dedup();
            u
        };
        if !unknown.is_empty() {
            return die(unknown_kinds(&reg, &unknown));
        }
    }
    let opencode = "opencode-commands".to_string();
    let mut rows = vec![Row {
        label: "base".into(),
        items: vec![
            ("skills", home.join(".agents/skills"), None),
            ("commands", home.join(".config/opencode/commands"), Some(opencode.clone())),
            ("commands", home.join(".config/opencode/command"), Some(opencode)),
        ],
        invoke: String::new(),
    }];
    for (k, ak) in &reg.0 {
        if want.as_ref().is_some_and(|w| !w.contains(k)) {
            continue;
        }
        if !ak.is_installed() {
            if want.is_some() {
                println!("skip {k:<9} not installed");
            }
            continue;
        }
        let flavor = ak.commands_from.clone().unwrap_or_else(|| skills::DEFAULT_FLAVOR.to_string());
        let mut items: Vec<_> = ak.skills_dirs.iter().map(|d| ("skills", skills::expand(d, &home), None)).collect();
        items.extend(ak.commands_dirs.iter().map(|d| ("commands", skills::expand(d, &home), Some(flavor.clone()))));
        rows.push(Row {
            label: k.clone(),
            items,
            invoke: ak.invoke.clone().unwrap_or_else(|| DEFAULT_INVOKE.to_string()),
        });
    }
    let (mut new, mut same) = (0, 0);
    for row in &rows {
        let (mut changed, mut recs, mut whats) = (false, Vec::new(), Vec::new());
        for (what, d, flavor) in &row.items {
            let r = match flavor {
                None => skills::sync_skills(d),
                Some(f) => skills::sync_commands(d, f),
            };
            match r {
                Ok((c, r)) => {
                    changed |= c;
                    recs.extend(r);
                }
                Err(e) => return die(format!("cannot write {}: {e}", d.display())),
            }
            whats.push(format!("{what}: {}", short(d, &home)));
        }
        if let Err(e) = skills::manifest_record(&home, &recs) {
            return die(format!("cannot write {}: {e}", skills::manifest_path(&home).display()));
        }
        new += usize::from(changed);
        same += usize::from(!changed);
        let invoke = if row.invoke.is_empty() { String::new() } else { format!("  {}", row.invoke) };
        println!("  {} {:<8} {}{invoke}", if changed { "NEW" } else { "ok  " }, row.label, whats.join("  "));
    }
    println!(
        "synced {} row(s): {new} new, {same} unchanged — restart running agents so they pick up the skills",
        rows.len()
    );
    0
}

/// The names of installed kinds whose orch skills or commands are missing (for the
/// `orch agents` footer).
pub fn kinds_missing_skills(reg: &Registry) -> Vec<String> {
    let home = home();
    reg.0
        .iter()
        .filter(|(_, ak)| ak.is_installed() && skills::agent_assets(ak, &home).iter().any(|a| a.state == AssetState::Missing))
        .map(|(k, _)| k.clone())
        .collect()
}
