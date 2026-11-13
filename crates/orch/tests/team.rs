//! `orch team <kind>` against the real binary, with a temporary HOME, a PATH holding a
//! fake `codex` (never run) and user registry/catalog files, diffed against Python
//! v4.10.1's `cmd_team` output. The `team up` engine is tested against the fake herdr
//! backend in the `team` module of the binary itself.

use std::process::{Command, Output};

/// `orch` with a temporary HOME, no user config files and a PATH holding only `bins`.
fn orch_cfg(
    args: &[&str],
    home: &std::path::Path,
    path: &std::path::Path,
    extra: &[(&str, &str)],
) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_orch"));
    c.args(args)
        .env("HOME", home)
        .env("PATH", path)
        .env_remove("ORCH_AGENTS_FILE")
        .env_remove("ORCH_MODELS_FILE")
        .env_remove("ORCH_LOG");
    for (k, v) in extra {
        c.env(k, v);
    }
    c.output().unwrap()
}

/// A temp HOME, a PATH dir with an executable `codex` (never run) in it, and the user
/// config files pointing at `home`. Returns (home, bin).
fn sandbox() -> (tempfile::TempDir, tempfile::TempDir) {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let f = bin.path().join("codex");
    std::fs::write(&f, "#!/bin/sh\nexit 99\n").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    (home, bin)
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn team_codex_prints_the_plan_lines_like_python() {
    let (home, bin) = sandbox();
    let o = orch_cfg(&["team", "codex"], home.path(), bin.path(), &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let out = text(&o.stdout);
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
    assert_eq!(out, expected, "{out}");
    assert!(o.stderr.is_empty(), "{}", text(&o.stderr));
}

#[test]
fn team_codex_spreads_coders_and_drops_the_reviewer() {
    let (home, bin) = sandbox();
    let o = orch_cfg(
        &["team", "codex", "--coders", "3"],
        home.path(),
        bin.path(),
        &[],
    );
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(
        out.contains("TEAM from codex: 3 coder(s), 1 reviewer"),
        "{out}"
    );
    assert!(
        out.contains("  codex-1      gpt-6-luna  (small tier)\n  codex-2      gpt-6-luna  (small tier)\n  codex-3      gpt-6-sol  (medium tier)"),
        "{out}"
    );
    let o = orch_cfg(
        &["team", "codex", "--no-reviewer"],
        home.path(),
        bin.path(),
        &[],
    );
    assert_eq!(o.status.code(), Some(0));
    let out = text(&o.stdout);
    assert!(out.starts_with("TEAM from codex: 2 coder(s)\n"), "{out}");
    assert!(out.contains("Reviewer: none\n"));
    assert!(!out.contains("  reviewer     "), "{out}");
}

#[test]
fn team_models_override_wins_and_denied_models_are_refused() {
    let (home, bin) = sandbox();
    let o = orch_cfg(
        &["team", "codex", "--models", "gpt-6-sol,gpt-6-luna"],
        home.path(),
        bin.path(),
        &[],
    );
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(
        out.contains("Workers: codex-1 (codex: gpt-6-sol), codex-2 (codex: gpt-6-luna)"),
        "{out}"
    );
    let o = orch_cfg(
        &["team", "codex", "--models", "fable-5"],
        home.path(),
        bin.path(),
        &[],
    );
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    assert!(
        text(&o.stderr).starts_with(
            "ERROR: 'fable-5' is not allowed by the model policy (pattern '*fable*') — see `orch models`"
        ),
        "{}",
        text(&o.stderr)
    );
}

#[test]
fn team_usage_errors_and_unknown_kinds() {
    let (home, bin) = sandbox();
    let usage = "usage: orch team <kind> [--coders N] [--models a,b] [--no-reviewer]  |  orch team up [--dry-run]";
    for args in [
        vec!["team"],
        vec!["team", "codex", "extra"],
        vec!["team", "--coders", "2"],
        vec!["team", "codex", "--coders"],
        vec!["team", "codex", "--models"],
    ] {
        let o = orch_cfg(&args, home.path(), bin.path(), &[]);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(
            text(&o.stderr).starts_with(&format!("ERROR: {usage}")),
            "{args:?}: {}",
            text(&o.stderr)
        );
    }
    let o = orch_cfg(
        &["team", "codex", "--coders", "0"],
        home.path(),
        bin.path(),
        &[],
    );
    assert_eq!(o.status.code(), Some(2));
    assert!(
        text(&o.stderr).starts_with("ERROR: --coders needs a number from 1"),
        "{}",
        text(&o.stderr)
    );
    let o = orch_cfg(&["team", "nope"], home.path(), bin.path(), &[]);
    assert_eq!(o.status.code(), Some(2));
    let err = text(&o.stderr);
    assert!(
        err.starts_with(
            "ERROR: unknown agent kind 'nope' — known: agy, amp, claude, codex, hermes, opencode"
        ),
        "{err}"
    );
    assert!(err.contains("(add it in "));
    let o = orch_cfg(
        &[
            "team",
            "codex",
            "--models",
            "fable-5",
            "--no-reviewer",
            "--coders",
            "3",
        ],
        home.path(),
        bin.path(),
        &[],
    );
    assert_eq!(
        o.status.code(),
        Some(2),
        "flags are still read before the kind error"
    );
}

#[test]
fn team_kinds_from_the_user_registry() {
    let (home, bin) = sandbox();
    let f = home.path().join("my-agents.toml");
    std::fs::write(
        &f,
        "[acme]\nbinary = \"codex\"\nlaunch = \"acme\"\nmodel_flag = \"-m\"\nroles = [\"lead\", \"coder\"]\n\
[solo]\nbinary = \"codex\"\nlaunch = \"solo\"\nroles = [\"reviewer\"]\n",
    )
    .unwrap();
    // the catalog must know the acme kind's models too
    let m = home.path().join("my-models.toml");
    std::fs::write(
        &m,
        "[models.acme-luna]\nkinds = [\"acme\"]\ntier = \"small\"\nreasoning = 1\n\
[models.acme-sol]\nkinds = [\"acme\"]\ntier = \"medium\"\nreasoning = 4\n",
    )
    .unwrap();
    let env = [
        ("ORCH_AGENTS_FILE", f.to_str().unwrap()),
        ("ORCH_MODELS_FILE", m.to_str().unwrap()),
    ];
    // acme: installed (the fake codex binary), coder role, no reviewer role
    let o = orch_cfg(&["team", "acme"], home.path(), bin.path(), &env);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(out.starts_with("TEAM from acme: 2 coder(s)\n"), "{out}");
    assert!(
        out.contains("  acme-1       acme-luna  (small tier)"),
        "{out}"
    );
    assert!(!out.contains("  reviewer     "), "{out}");
    assert!(
        out.contains("Workers: acme-1 (acme: acme-luna), acme-2 (acme: acme-sol)"),
        "{out}"
    );
    assert!(out.contains("Reviewer: none"));
    // solo: no coder role is refused
    let o = orch_cfg(&["team", "solo"], home.path(), bin.path(), &env);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        text(&o.stderr)
            .starts_with("ERROR: solo does not have the coder role in the agent registry"),
        "{}",
        text(&o.stderr)
    );
}

#[test]
fn team_amp_has_no_models_and_no_model_flag() {
    let (home, bin) = sandbox();
    let o = orch_cfg(&["team", "amp"], home.path(), bin.path(), &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let out = text(&o.stdout);
    let expected = [
        "TEAM from amp: 2 coder(s), 1 reviewer",
        "  amp-1        its default model",
        "  amp-2        its default model",
        "  reviewer     its default model",
        "",
        "Plan header lines:",
        "Lead: lead (amp: <the model your own pane runs>)",
        "Workers: amp-1 (amp), amp-2 (amp)",
        "Reviewer: reviewer (amp)",
        "",
        &format!(
            "NOTE: the model catalog has no allowed model for amp, so every pane starts on its default model (add models in {} to get tiers)",
            home.path().join(".config/herdr-orchestra/models.toml").display()
        ),
        "",
        "NOTE: amp has no model option in the registry: pick each pane's model inside amp",
        "",
        "Check these are models your amp really reaches (`--models a,b` to choose others).",
        "",
        "These panes don't exist yet. After `orch sync` and `orch start`, `orch team up` opens each one in its own folder, on its model, and names it.",
    ]
    .join("\n")
        + "\n";
    assert_eq!(out, expected, "{out}");
    assert!(
        text(&o.stderr)
            .starts_with("WARN: `amp` is not on PATH here, so amp panes would not start"),
        "{}",
        text(&o.stderr)
    );
}

#[test]
fn team_up_is_still_a_stub() {
    let (home, bin) = sandbox();
    for args in [&["team", "up"][..], &["team", "up", "--dry-run"][..]] {
        let o = orch_cfg(args, home.path(), bin.path(), &[]);
        assert_eq!(o.status.code(), Some(70), "{args:?}");
        assert!(
            text(&o.stderr).contains("not implemented in this build"),
            "{args:?}"
        );
    }
}
