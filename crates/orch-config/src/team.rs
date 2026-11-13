//! Building a team from one installed tool (§5, Python's `team_models`, `team_pick`
//! and `cmd_team`): the catalog models a kind may run, the coder pick over their tiers,
//! and the `TEAM from` report with the plan's `Workers:` / `Reviewer:` lines.

use crate::agents::{Registry, Role, user_agents_file};
use crate::launch::repr;
use crate::models::{Catalog, TIERS, user_models_file};
use std::collections::HashMap;

/// A tier's rank, weakest first (Python's `rank`): 1..3 for small/medium/high.
pub fn rank(tier: &str) -> u32 {
    TIERS
        .iter()
        .position(|t| *t == tier)
        .map_or(0, |i| i as u32 + 1)
}

/// Python's `round`: halves go to the even neighbour.
fn py_round(x: f64) -> f64 {
    let t = x.trunc();
    let f = x - t;
    if (f - 0.5).abs() < f64::EPSILON && t % 2.0 == 0.0 {
        t
    } else {
        x.round()
    }
}

/// The catalog models this kind can run, allowed by policy and rated, weakest first
/// (Python's `team_models`): `(id, tier)` sorted by (tier rank, id).
pub fn team_models(cat: &Catalog, kind: &str) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = cat
        .models
        .keys()
        .filter(|mid| {
            cat.models
                .get(*mid)
                .is_some_and(|m| m.kinds.iter().any(|k| k == kind))
                && cat.denied(mid).is_none()
        })
        .filter_map(|mid| {
            let tier = cat.model_tier(mid)?;
            Some((mid.clone(), tier))
        })
        .collect();
    out.sort_by_key(|(mid, tier)| (rank(tier), mid.clone()));
    out
}

/// `n` coder models spread over the tiers on offer (Python's `team_pick`): the weakest
/// and the strongest always, the middle ones spaced evenly between. `None` entries
/// stand for "its default model" when the catalog has nothing for the kind.
pub fn team_pick(cands: &[(String, &'static str)], n: usize) -> Vec<Option<String>> {
    if cands.is_empty() {
        return vec![None; n];
    }
    if n == 1 {
        return vec![Some(cands[cands.len() - 1].0.clone())];
    }
    let last = cands.len() - 1;
    (0..n)
        .map(|i| {
            let idx = py_round(i as f64 * last as f64 / (n - 1) as f64) as usize;
            Some(cands[idx.min(last)].0.clone())
        })
        .collect()
}

/// What `orch team <kind>` prints: the stdout text, and the warnings for stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamOutput {
    /// The text for stdout, lines ended with `\n`.
    pub stdout: String,
    /// The warnings for stderr, one per line (`WARN: …`).
    pub warnings: Vec<String>,
}

/// Python's `cmd_team` given its parsed options (`models` entries are already
/// lowercase): the `TEAM from` table and the plan's header lines.
pub fn render_team(
    reg: &Registry,
    cat: &Catalog,
    kind: &str,
    coders: Option<usize>,
    models: Option<Vec<String>>,
    reviewer: bool,
) -> std::result::Result<TeamOutput, String> {
    let ak = reg.get(kind).ok_or_else(|| {
        format!(
            "unknown agent kind {} — known: {} (add it in {})",
            repr(kind),
            reg.kinds().join(", "),
            user_agents_file().display()
        )
    })?;
    let mut warnings = Vec::new();
    if !ak.is_installed() {
        warnings.push(format!(
            "WARN: `{}` is not on PATH here, so {} panes would not start",
            ak.binary, kind
        ));
    }
    if !ak.roles.contains(&Role::Coder) {
        return Err(format!(
            "{kind} does not have the coder role in the agent registry"
        ));
    }
    let cands = team_models(cat, kind);
    let had_override = models.is_some();
    let picked = match models {
        Some(list) => {
            for m in &list {
                if let Some(pat) = cat.denied(m) {
                    return Err(format!(
                        "{} is not allowed by the model policy (pattern {}) — see `orch models`",
                        repr(m),
                        repr(pat)
                    ));
                }
            }
            match coders {
                None => list.into_iter().map(Some).collect(),
                Some(n) => list.iter().cycle().take(n).cloned().map(Some).collect(),
            }
        }
        None => team_pick(&cands, coders.unwrap_or(2)),
    };
    let top = cands.last().map(|(m, _)| m.as_str());
    let no_flag = ak.model_flag.as_deref().is_none_or(str::is_empty);
    let tier: HashMap<&str, &str> = cands.iter().map(|(m, t)| (m.as_str(), *t)).collect();
    let fmt = |name: &str, m: Option<&str>| match m.filter(|_| !no_flag) {
        Some(m) => format!("{name} ({kind}: {m})"),
        None => format!("{name} ({kind})"),
    };
    let workers: Vec<String> = picked
        .iter()
        .enumerate()
        .map(|(i, m)| fmt(&format!("{kind}-{}", i + 1), m.as_deref()))
        .collect();
    let with_reviewer = reviewer && ak.roles.contains(&Role::Reviewer);
    let mut out = format!(
        "TEAM from {kind}: {} coder(s){}\n",
        workers.len(),
        if with_reviewer { ", 1 reviewer" } else { "" }
    );
    let row = |name: &str, m: Option<&str>| {
        let tier_of = m.and_then(|m| tier.get(m).copied());
        format!(
            "  {name:<12} {}{}\n",
            m.unwrap_or("its default model"),
            tier_of.map_or(String::new(), |t| format!("  ({t} tier)"))
        )
    };
    for (i, m) in picked.iter().enumerate() {
        out.push_str(&row(&format!("{kind}-{}", i + 1), m.as_deref()));
    }
    if with_reviewer {
        out.push_str(&row("reviewer", top));
    }
    out.push_str("\nPlan header lines:\n");
    out.push_str(&format!(
        "Lead: lead ({kind}: <the model your own pane runs>)\n"
    ));
    out.push_str(&format!("Workers: {}\n", workers.join(", ")));
    out.push_str(&format!(
        "Reviewer: {}\n",
        if with_reviewer {
            fmt("reviewer", top)
        } else {
            "none".to_string()
        }
    ));
    if cands.is_empty() && !had_override {
        out.push_str(&format!(
            "\nNOTE: the model catalog has no allowed model for {kind}, so every pane starts on its default model \
(add models in {} to get tiers)\n",
            user_models_file().display()
        ));
    }
    if no_flag {
        out.push_str(&format!(
            "\nNOTE: {kind} has no model option in the registry: pick each pane's model inside {kind}\n"
        ));
    }
    if let Some(top) = top
        && tier.get(top).copied() != Some("high")
    {
        out.push_str(&format!(
            "\nNOTE: the strongest allowed {kind} model is {top} ({} tier): high-tier milestones go to it, and \
`orch sync` will say so\n",
            tier.get(top).copied().unwrap_or("?")
        ));
    }
    out.push_str(&format!(
        "\nCheck these are models your {kind} really reaches (`--models a,b` to choose others).\n"
    ));
    out.push_str(
        "\nThese panes don't exist yet. After `orch sync` and `orch start`, `orch team up` opens each one in its \
own folder, on its model, and names it.\n",
    );
    Ok(TeamOutput {
        stdout: out,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents;
    use crate::models;
    use std::path::Path;
    use std::sync::Mutex;

    fn reg(user: &str) -> Registry {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("agents.toml");
        std::fs::write(&p, user).unwrap();
        agents::registry(agents::DEFAULT_TOML, &p).unwrap()
    }

    fn cat(user: &str) -> Catalog {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("models.toml");
        std::fs::write(&p, user).unwrap();
        models::catalog(models::DEFAULT_TOML, &p).unwrap()
    }

    /// Keep the `user_agents_file()` / `user_models_file()` messages deterministic.
    static LOCK: Mutex<()> = Mutex::new(());

    fn with_files() -> (std::path::PathBuf, std::path::PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("agents.toml");
        let m = t.path().join("models.toml");
        unsafe { std::env::set_var("ORCH_AGENTS_FILE", &a) };
        unsafe { std::env::set_var("ORCH_MODELS_FILE", &m) };
        (a, m)
    }

    #[test]
    fn team_models_lists_only_the_kinds_allowed_rated_models_weakest_first() {
        let c = cat("[policy]\ndeny = [\"bad-*\"]\n\
[models.low]\nkinds = [\"acme\"]\ntier = \"small\"\nreasoning = 1\n\
[models.mid]\nkinds = [\"acme\"]\ntier = \"medium\"\nreasoning = 3\n\
[models.top]\nkinds = [\"acme\"]\ntier = \"high\"\nreasoning = 5\n\
[models.bad-1]\nkinds = [\"acme\"]\ntier = \"high\"\nreasoning = 5\n\
[models.unrated]\nkinds = [\"acme\"]\n\
[models.other]\nkinds = [\"claude\"]\ntier = \"high\"\nreasoning = 5\n");
        assert_eq!(
            team_models(&c, "acme"),
            [
                ("low".to_string(), "small"),
                ("mid".to_string(), "medium"),
                ("top".to_string(), "high"),
            ]
        );
        // denied wins even at the top; unrated models have no tier to serve
        assert!(!team_models(&c, "acme").iter().any(|(m, _)| m == "bad-1"));
        assert!(team_models(&c, "claude").iter().any(|(m, _)| m == "other"));
        // a reasoning rating below its tier's floor serves the tier below
        let c = cat("[models.wannabe]\nkinds = [\"acme\"]\ntier = \"high\"\nreasoning = 3\n");
        assert_eq!(team_models(&c, "acme"), [("wannabe".to_string(), "medium")]);
    }

    #[test]
    fn the_pick_takes_the_endpoints_and_spreads_the_middle() {
        let c: Vec<(String, &'static str)> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|s| (s.to_string(), "medium"))
            .collect();
        assert_eq!(team_pick(&c, 1), [Some("e".to_string())]);
        assert_eq!(
            team_pick(&c, 2),
            [Some("a".to_string()), Some("e".to_string())]
        );
        assert_eq!(
            team_pick(&c, 3),
            [
                Some("a".to_string()),
                Some("c".to_string()),
                Some("e".to_string())
            ]
        );
        assert_eq!(
            team_pick(&c, 5),
            ["a", "b", "c", "d", "e"]
                .iter()
                .map(|s| Some(s.to_string()))
                .collect::<Vec<_>>()
        );
        // Python's round-half-even: round(0.5) == 0, so a 2-model catalog with
        // --coders 3 gives weak, weak, strong.
        let two: Vec<(String, &'static str)> = ["a", "b"]
            .iter()
            .map(|s| (s.to_string(), "medium"))
            .collect();
        assert_eq!(
            team_pick(&two, 3),
            [
                Some("a".to_string()),
                Some("a".to_string()),
                Some("b".to_string())
            ]
        );
        assert_eq!(team_pick(&[], 2), [None, None]);
    }

    #[test]
    fn codex_team_matches_pythons_output_exactly() {
        let _g = LOCK.lock().unwrap();
        with_files();
        // Hermetic PATH: the codex binary is not on it, so the run carries the
        // not-on-PATH warning.
        let old_path = std::env::var_os("PATH");
        unsafe { std::env::set_var("PATH", "/nonexistent-m3") };
        let r =
            agents::registry(agents::DEFAULT_TOML, Path::new("/nonexistent/agents.toml")).unwrap();
        let c =
            models::catalog(models::DEFAULT_TOML, Path::new("/nonexistent/models.toml")).unwrap();
        let out = render_team(&r, &c, "codex", None, None, true).unwrap();
        let agents_file = agents::user_agents_file().display().to_string();
        let models_file = models::user_models_file().display().to_string();
        let expected = [
            "TEAM from codex: 2 coder(s), 1 reviewer",
            "  codex-1      gpt-6-luna  (small tier)",
            "  codex-2      gpt-6-sol  (medium tier)",
            "  reviewer     gpt-6-sol  (medium tier)",
            "",
            "Plan header lines:",
            "Lead: lead (codex: <the model your own pane runs>)",
            "Workers: codex-1 (codex: gpt-6-luna), codex-2 (codex: gpt-6-sol)",
            "Reviewer: reviewer (codex: gpt-6-sol)",
            "",
            "NOTE: the strongest allowed codex model is gpt-6-sol (medium tier): high-tier milestones go to it, and `orch sync` will say so",
            "",
            "Check these are models your codex really reaches (`--models a,b` to choose others).",
            "",
            "These panes don't exist yet. After `orch sync` and `orch start`, `orch team up` opens each one in its own folder, on its model, and names it.",
        ]
        .join("\n")
            + "\n";
        assert_eq!(out.stdout, expected, "{}", out.stdout);
        assert_eq!(
            out.warnings,
            ["WARN: `codex` is not on PATH here, so codex panes would not start".to_string()]
        );
        assert!(agents_file.ends_with("agents.toml") && models_file.ends_with("models.toml"));
        unsafe { std::env::remove_var("ORCH_AGENTS_FILE") };
        unsafe { std::env::remove_var("ORCH_MODELS_FILE") };
        match old_path {
            Some(p) => unsafe { std::env::set_var("PATH", p) },
            None => unsafe { std::env::remove_var("PATH") },
        }
    }

    #[test]
    fn coders_n_spreads_the_pick_over_the_tiers() {
        let r =
            agents::registry(agents::DEFAULT_TOML, Path::new("/nonexistent/agents.toml")).unwrap();
        let c =
            models::catalog(models::DEFAULT_TOML, Path::new("/nonexistent/models.toml")).unwrap();
        let out = render_team(&r, &c, "claude", Some(3), None, true).unwrap();
        let body = out.stdout.lines().take(4).collect::<Vec<_>>();
        assert_eq!(
            body,
            [
                "TEAM from claude: 3 coder(s), 1 reviewer",
                "  claude-1     haiku  (small tier)",
                "  claude-2     sonnet  (medium tier)",
                "  claude-3     opus  (high tier)",
            ],
            "{body:?}"
        );
    }

    #[test]
    fn a_models_override_beats_the_pick_and_denied_models_are_refused() {
        let r =
            agents::registry(agents::DEFAULT_TOML, Path::new("/nonexistent/agents.toml")).unwrap();
        let c =
            models::catalog(models::DEFAULT_TOML, Path::new("/nonexistent/models.toml")).unwrap();
        let out = render_team(
            &r,
            &c,
            "codex",
            None,
            Some(vec!["gpt-6-sol".into(), "gpt-6-luna".into()]),
            true,
        )
        .unwrap();
        assert!(
            out.stdout
                .contains("Workers: codex-1 (codex: gpt-6-sol), codex-2 (codex: gpt-6-luna)")
        );
        // `--models a,b` with `--coders N` repeats the list to fill N workers
        let out = render_team(
            &r,
            &c,
            "codex",
            Some(3),
            Some(vec!["gpt-6-luna".into(), "gpt-6-sol".into()]),
            true,
        )
        .unwrap();
        assert!(out.stdout.contains("Workers: codex-1 (codex: gpt-6-luna), codex-2 (codex: gpt-6-sol), codex-3 (codex: gpt-6-luna)"));
        // a model listed for other kinds still runs, without a tier note of its own
        let out = render_team(&r, &c, "codex", None, Some(vec!["opus".into()]), true).unwrap();
        assert!(
            out.stdout.contains("  codex-1      opus\n"),
            "{}",
            out.stdout
        );
        let e = render_team(&r, &c, "codex", None, Some(vec!["fable-5".into()]), true).unwrap_err();
        assert_eq!(
            e,
            "'fable-5' is not allowed by the model policy (pattern '*fable*') — see `orch models`"
        );
    }

    #[test]
    fn unknown_kinds_and_kinds_without_the_coder_role_are_refused() {
        let r = reg("[solo]\nbinary = \"solo\"\nlaunch = \"solo\"\nroles = [\"reviewer\"]\n");
        let c =
            models::catalog(models::DEFAULT_TOML, Path::new("/nonexistent/models.toml")).unwrap();
        let e = render_team(&r, &c, "nope", None, None, true).unwrap_err();
        assert!(
            e.starts_with(
                "unknown agent kind 'nope' — known: agy, amp, claude, codex, hermes, opencode, solo"
            ),
            "{e}"
        );
        let e = render_team(&r, &c, "solo", None, None, true).unwrap_err();
        assert_eq!(e, "solo does not have the coder role in the agent registry");
    }

    #[test]
    fn a_kind_without_a_reviewer_role_gets_none() {
        let r = reg(
            "[solo]\nbinary = \"solo\"\nlaunch = \"solo\"\nmodel_flag = \"-m\"\nroles = [\"lead\", \"coder\"]\n\
[acme]\nbinary = \"acme\"\nlaunch = \"acme\"\nmodel_flag = \"-m\"\nroles = [\"coder\"]\n",
        );
        let c =
            models::catalog(models::DEFAULT_TOML, Path::new("/nonexistent/models.toml")).unwrap();
        let out = render_team(&r, &c, "solo", None, None, true).unwrap();
        assert_eq!(
            out.stdout
                .lines()
                .find(|l| l.starts_with("Reviewer:"))
                .unwrap(),
            "Reviewer: none"
        );
        assert!(!out.stdout.contains(", 1 reviewer"));
        assert!(!out.stdout.contains("  reviewer     "));
        // even with --no-reviewer for a kind that has the role
        let out = render_team(&r, &c, "acme", None, None, false).unwrap();
        assert_eq!(
            out.stdout
                .lines()
                .find(|l| l.starts_with("Reviewer:"))
                .unwrap(),
            "Reviewer: none"
        );
        assert!(out.stdout.starts_with("TEAM from acme: 2 coder(s)\n"));
    }

    #[test]
    fn a_kind_without_a_model_flag_writes_name_kind_and_the_note() {
        let _g = LOCK.lock().unwrap();
        let (a, m) = with_files();
        let r = reg(
            "[acme]\nbinary = \"acme\"\nlaunch = \"acme\"\nroles = [\"lead\", \"coder\", \"reviewer\"]\n",
        );
        let c = cat("");
        let out = render_team(&r, &c, "acme", None, None, true).unwrap();
        let expected = [
            "TEAM from acme: 2 coder(s), 1 reviewer",
            "  acme-1       its default model",
            "  acme-2       its default model",
            "  reviewer     its default model",
            "",
            "Plan header lines:",
            "Lead: lead (acme: <the model your own pane runs>)",
            "Workers: acme-1 (acme), acme-2 (acme)",
            "Reviewer: reviewer (acme)",
            "",
            &format!(
                "NOTE: the model catalog has no allowed model for acme, so every pane starts on its default model (add models in {} to get tiers)",
                m.display()
            ),
            "",
            "NOTE: acme has no model option in the registry: pick each pane's model inside acme",
            "",
            "Check these are models your acme really reaches (`--models a,b` to choose others).",
            "",
            "These panes don't exist yet. After `orch sync` and `orch start`, `orch team up` opens each one in its own folder, on its model, and names it.",
        ]
        .join("\n")
            + "\n";
        assert_eq!(out.stdout, expected, "{}", out.stdout);
        // `--models` on a kind without model_flag keeps `name (kind)` and drops the
        // no-allowed-model note
        let out = render_team(&r, &c, "acme", None, Some(vec!["mystery-1".into()]), true).unwrap();
        assert!(
            out.stdout.contains("  acme-1       mystery-1\n"),
            "{}",
            out.stdout
        );
        assert!(!out.stdout.contains("no allowed model for acme"));
        assert!(a.ends_with("agents.toml") && m.ends_with("models.toml"));
        unsafe { std::env::remove_var("ORCH_AGENTS_FILE") };
        unsafe { std::env::remove_var("ORCH_MODELS_FILE") };
    }

    #[test]
    fn a_kind_whose_strongest_model_is_not_high_tier_warns() {
        let r =
            agents::registry(agents::DEFAULT_TOML, Path::new("/nonexistent/agents.toml")).unwrap();
        let c =
            models::catalog(models::DEFAULT_TOML, Path::new("/nonexistent/models.toml")).unwrap();
        let out = render_team(&r, &c, "codex", None, None, true).unwrap();
        assert!(
            out.stdout.contains(
                "NOTE: the strongest allowed codex model is gpt-6-sol (medium tier): high-tier milestones go to it"
            ),
            "{}",
            out.stdout
        );
        // a kind with a high-tier strongest model does not
        let out = render_team(&r, &c, "claude", None, None, true).unwrap();
        assert!(
            !out.stdout.contains("NOTE: the strongest allowed"),
            "{}",
            out.stdout
        );
    }
}
