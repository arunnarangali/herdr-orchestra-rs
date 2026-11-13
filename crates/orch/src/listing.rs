//! `orch agents`, `orch agents launch` and `orch models [show|check]`: the read-only
//! listing commands, as Python v4.10.1's `cmd_agents` and `cmd_models`.

use orch_config::agents::{self, Registry};
use orch_config::launch::{launch_command, repr};
use orch_config::models::{self, Catalog, RATINGS, TIERS};
use orch_core::Error;
use orch_core::error::EXIT_USAGE;

/// Python's `die`: `ERROR: msg` on stderr, exit 2.
pub fn die(msg: impl std::fmt::Display) -> i32 {
    eprintln!("ERROR: {msg}");
    EXIT_USAGE
}

fn fail(e: Error) -> i32 {
    eprintln!("ERROR: {e}");
    e.exit_code()
}

pub const AGENTS_USAGE: &str = "usage: orch agents [launch <kind> | check | sync | dirs]";
const LAUNCH_USAGE: &str = "usage: orch agents launch <kind> [--model <id>]";

/// `orch agents` with no subcommand: the registry table.
pub fn agents_table() -> i32 {
    let reg = match agents::load(None) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    print!("{}", render_agents(&reg));
    0
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

pub fn render_agents(reg: &Registry) -> String {
    let mut s = format!(
        "{:<9} {:<9} {:<6} {:<8} {:<22} launch\n",
        "kind", "installed", "detect", "verified", "roles"
    );
    for (k, v) in &reg.0 {
        let roles: Vec<_> = v.roles.iter().map(|r| r.as_str()).collect();
        let launch = if v.launch.is_empty() {
            &v.binary
        } else {
            &v.launch
        };
        s.push_str(&format!(
            "{k:<9} {:<9} {:<6} {:<8} {:<22} {launch}\n",
            yes(v.is_installed()),
            yes(v.herdr_detect),
            yes(v.verified),
            roles.join(",")
        ));
    }
    s.push_str(&format!(
        "\nOverride or add kinds in {}\n",
        agents::user_agents_file().display()
    ));
    s
}

/// `orch agents launch <kind> [--model <id>]` (`args` are the words after `launch`).
pub fn agents_launch(args: &[String]) -> i32 {
    let mut rest: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut model = None;
    if let Some(i) = rest.iter().position(|a| *a == "--model") {
        let Some(m) = rest.get(i + 1).copied() else {
            return die(LAUNCH_USAGE);
        };
        model = Some(m);
        rest.drain(i..i + 2);
    }
    let Some(kind) = rest.first().copied() else {
        return die(LAUNCH_USAGE);
    };
    let reg = match agents::load(None) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    let cat = match models::load(None) {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    match launch_command(&reg, &cat, kind, model, true) {
        Ok(l) => {
            for w in &l.warnings {
                eprintln!("{w}");
            }
            println!("{}", l.command);
            0
        }
        Err(e) => die(e),
    }
}

/// `orch models [show|check <id>]` (`args` are the words after `models`).
pub fn models(args: &[String]) -> i32 {
    let cat = match models::load(None) {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    match args.first().map(String::as_str) {
        None => {
            print!("{}", render_catalog(&cat));
            0
        }
        Some(sub @ ("show" | "check")) => {
            let Some(id) = args.get(1) else {
                return die(format!("usage: orch models {sub} <model id>"));
            };
            model_one(&cat, sub == "show", &id.to_lowercase())
        }
        Some(_) => die("usage: orch models [check <id> | show <id>]"),
    }
}

fn model_one(cat: &Catalog, show: bool, mid: &str) -> i32 {
    if let Some(pat) = cat.denied(mid) {
        let why = cat
            .policy
            .deny_why
            .as_deref()
            .unwrap_or("not allowed in runs");
        println!("DENIED {mid}: matches policy pattern {} — {why}", repr(pat));
        return 1;
    }
    let Some(m) = cat.models.get(mid) else {
        println!(
            "UNRATED {mid}: not in the catalog — allowed, but it has no tier; add it to {}",
            models::user_models_file().display()
        );
        return if show { 1 } else { 0 };
    };
    println!("OK {mid}: tier {}", cat.model_tier(mid).unwrap_or("None"));
    if show {
        let num = |v: Option<i64>| v.map(|n| n.to_string());
        let rows = [
            ("kinds", (!m.kinds.is_empty()).then(|| m.kinds.join(", "))),
            ("reasoning", num(m.reasoning)),
            ("coding", num(m.coding)),
            ("architecture", num(m.architecture)),
            ("instructions", num(m.instructions)),
            ("tools", num(m.tools)),
            ("planning", num(m.planning)),
            ("speed", num(m.speed)),
            ("context_k", num(m.context_k)),
            (
                "open_weights",
                m.open_weights
                    .map(|b| if b { "True" } else { "False" }.to_string()),
            ),
            ("evidence", m.evidence.clone()),
        ];
        debug_assert_eq!(RATINGS.len(), 7);
        for (k, v) in rows {
            if let Some(v) = v {
                println!("  {k:<13} {v}");
            }
        }
    }
    0
}

pub fn render_catalog(cat: &Catalog) -> String {
    let dash = |v: Option<i64>| v.map_or_else(|| "-".to_string(), |n| n.to_string());
    let mut s = format!(
        "{:<18} {:<7} {:<17} {:>4} {:>4} {:>4} {:>4} {:>4} {:>4} {:>5} {:>6}  open\n",
        "model", "tier", "kinds", "reas", "code", "arch", "inst", "tool", "plan", "speed", "ctx k"
    );
    for tier in TIERS.iter().rev() {
        for (mid, m) in &cat.models {
            if cat.model_tier(mid) != Some(*tier) || cat.denied(mid).is_some() {
                continue;
            }
            s.push_str(&format!(
                "{mid:<18} {tier:<7} {:<17} {:>4} {:>4} {:>4} {:>4} {:>4} {:>4} {:>5} {:>6}  {}\n",
                m.kinds.join(","),
                dash(m.reasoning),
                dash(m.coding),
                dash(m.architecture),
                dash(m.instructions),
                dash(m.tools),
                dash(m.planning),
                dash(m.speed),
                dash(m.context_k.filter(|n| *n != 0)),
                yes(m.open_weights == Some(true)),
            ));
        }
    }
    s.push_str("\nTiers (a model needs this reasoning rating to serve the tier):\n");
    for tier in TIERS {
        match cat.tiers.get(tier) {
            Some(t) => s.push_str(&format!(
                "  {tier:<7} reasoning >= {}: {}\n",
                t.reasoning,
                t.use_.as_deref().unwrap_or("")
            )),
            None => s.push_str(&format!("  {tier:<7} reasoning >= -: \n")),
        }
    }
    let deny = &cat.policy.deny;
    s.push_str(&format!(
        "\nNot allowed in runs: {}",
        if deny.is_empty() {
            "nothing".to_string()
        } else {
            deny.join(", ")
        }
    ));
    if let (false, Some(why)) = (
        deny.is_empty(),
        cat.policy.deny_why.as_deref().filter(|w| !w.is_empty()),
    ) {
        s.push_str(&format!(" — {why}"));
    }
    s.push_str(&format!(
        "\nRatings are starting points (`orch models show <id>` prints the evidence). Override or add models in {}\n",
        models::user_models_file().display()
    ));
    s
}
