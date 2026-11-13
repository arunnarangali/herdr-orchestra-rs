//! `orch doctor`, `orch agents check` and `orch agents sync` against fake agent
//! executables on a temporary PATH, with a temporary HOME. No real agent CLI, herdr or
//! home folder is touched.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ACME: &str = r#"
[acme]
binary = "acme"
launch = "acme --go"
roles = ["coder"]
skills_dirs = ["~/.acme/skills"]
commands_dirs = ["~/.acme/commands"]
commands_from = "claude-commands"
invoke = "/orch-plan <feature>"
verified = true
"#;

struct Env {
    home: tempfile::TempDir,
    bin: tempfile::TempDir,
    herdr_state: tempfile::TempDir,
}

impl Env {
    fn new(registry: &str) -> Env {
        let e = Env {
            home: tempfile::tempdir().unwrap(),
            bin: tempfile::tempdir().unwrap(),
            herdr_state: tempfile::tempdir().unwrap(),
        };
        std::fs::write(e.home.path().join("agents.toml"), registry).unwrap();
        e
    }

    /// An executable fake agent `acme` with a shell body (builtins only: PATH is just this folder).
    fn script(&self, name: &str, body: &str) {
        let p = self.bin.path().join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A well-behaved acme: version, help listing `--go`, `acme auth` exits $ACME_AUTH (default 0).
    fn good_acme(&self) {
        self.script(
            "acme",
            r#"case "$1" in
  --version) echo "acme 1.2.3" ;;
  --help) echo "  --go   start working" ;;
  auth) exit ${ACME_AUTH:-0} ;;
esac"#,
        );
    }

    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_orch"));
        c.args(args)
            .env("HOME", self.home.path())
            .env("PATH", self.bin.path())
            .env("ORCH_AGENTS_FILE", self.home.path().join("agents.toml"))
            .env_remove("ORCH_MODELS_FILE")
            .env("HERDR", "/nonexistent/herdr")
            .env("FAKE_HERDR_DIR", self.herdr_state.path());
        for (k, _) in std::env::vars().filter(|(k, _)| k.starts_with("HERDR_")) {
            c.env_remove(k);
        }
        for (k, v) in extra {
            c.env(k, v);
        }
        c.output().unwrap()
    }

    fn manifest(&self) -> String {
        std::fs::read_to_string(self.home.path().join(".local/share/herdr-orchestra/manifest.txt")).unwrap_or_default()
    }
}

fn fake_herdr() -> PathBuf {
    let p = Path::new(env!("CARGO_BIN_EXE_orch")).with_file_name("fake-herdr");
    assert!(p.exists(), "{} is missing: run `cargo build --workspace` first", p.display());
    p
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Sync first so the skills are not what makes a doctor run fail.
fn synced(e: &Env) {
    let o = e.run(&["agents", "sync", "acme"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
}

#[test]
fn a_missing_binary_is_skipped() {
    let e = Env::new(ACME);
    let o = e.run(&["doctor", "acme"], &[]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o), "skip    acme: `acme` is not installed\n");
}

#[test]
fn a_healthy_agent_is_ok_with_one_line_per_check() {
    let e = Env::new(ACME);
    e.good_acme();
    synced(&e);
    let o = e.run(&["doctor", "acme"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    let s = out(&o);
    assert!(s.starts_with("ok      acme\n"), "{s}");
    assert!(s.contains("    ok  version: acme 1.2.3\n"), "{s}");
    assert!(s.contains("    ok  launch flag --go is listed in `acme --help`\n"), "{s}");
    assert_eq!(s.matches("    ok  skill ~/.acme/").count(), 4, "{s}");
    assert!(s.contains("    ok  auth: unknown (no status command configured"), "{s}");
    assert!(!s.contains("BAD"), "{s}");
}

#[test]
fn a_failing_version_command_reports_unknown() {
    let e = Env::new(ACME);
    e.script("acme", "case \"$1\" in --help) echo '--go' ;; *) exit 1 ;; esac");
    synced(&e);
    let o = e.run(&["doctor", "acme"], &[]);
    assert!(out(&o).contains("    ok  version: unknown (`acme --version` failed)\n"), "{}", out(&o));
    assert_eq!(o.status.code(), Some(0), "an unknown version is not broken");
}

#[test]
fn a_custom_version_command_is_used() {
    let e = Env::new(&format!("{ACME}version_cmd = \"acme -V\"\n"));
    e.script("acme", "case \"$1\" in -V) echo 'acme custom 9';; --help) echo '--go';; esac");
    synced(&e);
    assert!(out(&e.run(&["doctor", "acme"], &[])).contains("version: acme custom 9"));
}

#[test]
fn a_launch_flag_missing_from_the_help_is_broken() {
    let e = Env::new(ACME);
    e.script("acme", "case \"$1\" in --version) echo v;; --help) echo '--go-faster only';; esac");
    synced(&e);
    let o = e.run(&["doctor", "acme"], &[]);
    assert_eq!(o.status.code(), Some(1));
    let s = out(&o);
    assert!(s.starts_with("BROKEN  acme\n"), "{s}");
    assert!(s.contains("    BAD launch flag --go is NOT in `acme --help` — the launch command in the registry may be out of date (override it in "), "{s}");
}

#[test]
fn a_help_command_that_does_not_run_is_broken() {
    let e = Env::new(&format!("{ACME}help_cmd = \"/nonexistent/help\"\n"));
    e.good_acme();
    synced(&e);
    let o = e.run(&["doctor", "acme"], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("BAD launch flag --go: `/nonexistent/help` did not run"), "{}", out(&o));
}

#[test]
fn a_hanging_external_call_is_stopped_by_the_timeout() {
    let e = Env::new(ACME);
    e.script("acme", "case \"$1\" in --help) echo '--go';; *) while :; do :; done;; esac");
    synced(&e);
    let started = std::time::Instant::now();
    let o = e.run(&["doctor", "acme"], &[("ORCH_PROBE_TIMEOUT_S", "1")]);
    assert!(started.elapsed().as_secs() < 20);
    assert!(out(&o).contains("version: unknown"), "{}", out(&o));
}

#[test]
fn authentication_outcomes() {
    let e = Env::new(&format!("{ACME}auth_cmd = \"acme auth\"\n"));
    e.good_acme();
    synced(&e);
    // authenticated
    let o = e.run(&["doctor", "acme"], &[("ACME_AUTH", "0")]);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert!(out(&o).contains("    ok  auth: authenticated (`acme auth` exited 0)"), "{}", out(&o));
    // not authenticated is broken
    let o = e.run(&["doctor", "acme"], &[("ACME_AUTH", "1")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("    BAD auth: not authenticated (`acme auth` exited 1)"), "{}", out(&o));
    // unknown: the status command cannot run
    let e2 = Env::new(&format!("{ACME}auth_cmd = \"/nonexistent/status\"\n"));
    e2.good_acme();
    synced(&e2);
    let o = e2.run(&["doctor", "acme"], &[]);
    assert_eq!(o.status.code(), Some(0), "unknown is not broken: {}", out(&o));
    assert!(out(&o).contains("    ok  auth: unknown (`/nonexistent/status` did not run"), "{}", out(&o));
}

#[test]
fn authentication_by_login_state_never_shows_its_content() {
    let e = Env::new(&format!("{ACME}auth_file = \"~/.acme/login.json\"\n"));
    e.good_acme();
    synced(&e);
    let o = e.run(&["doctor", "acme"], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("BAD auth: not authenticated (login state ~/.acme/login.json not found)"), "{}", out(&o));
    std::fs::write(e.home.path().join(".acme/login.json"), "SECRET-TOKEN").unwrap();
    let o = e.run(&["doctor", "acme"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert!(out(&o).contains("auth: authenticated (login state ~/.acme/login.json exists)"));
    assert!(!out(&o).contains("SECRET-TOKEN") && !err(&o).contains("SECRET-TOKEN"));
}

#[test]
fn herdr_not_reachable_is_not_checked_and_not_broken() {
    let e = Env::new(ACME);
    e.good_acme();
    synced(&e);
    let o = e.run(&["doctor", "acme"], &[]);
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).contains("    ok  herdr not reachable here: detection not checked\n"), "{}", out(&o));
}

#[test]
fn herdr_listing_the_kind_is_checked_through_the_adapter() {
    let e = Env::new(ACME);
    e.good_acme();
    synced(&e);
    let fake = fake_herdr();
    let env = [("HERDR", fake.to_str().unwrap())];
    std::fs::write(e.herdr_state.path().join("kinds.txt"), "claude|acme|codex\n").unwrap();
    let o = e.run(&["doctor", "acme"], &env);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert!(out(&o).contains("    ok  herdr knows this kind\n"), "{}", out(&o));
    std::fs::write(e.herdr_state.path().join("kinds.txt"), "claude|codex\n").unwrap();
    let o = e.run(&["doctor", "acme"], &env);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("    BAD herdr does not list this kind (`herdr agent`): set herdr_detect = false for it"));
    // a kind with herdr_detect = false is not asked
    let e2 = Env::new(&format!("{ACME}herdr_detect = false\n"));
    e2.good_acme();
    synced(&e2);
    assert!(!out(&e2.run(&["doctor", "acme"], &env)).contains("herdr"));
}

#[test]
fn doctor_reports_an_unknown_kind_and_missing_skills() {
    let e = Env::new(ACME);
    e.good_acme();
    let o = e.run(&["doctor", "nope", "acme"], &[]);
    assert_eq!(o.status.code(), Some(1));
    let s = out(&o);
    assert!(s.starts_with("BROKEN  nope: not in the agent registry (add it in "), "{s}");
    assert!(s.contains("    BAD skill ~/.acme/skills/orch-plan/SKILL.md missing — run `orch agents sync`"), "{s}");
}

#[test]
fn doctor_with_no_kinds_covers_the_whole_registry() {
    let e = Env::new(ACME);
    e.good_acme();
    synced(&e);
    let s = out(&e.run(&["doctor"], &[]));
    assert!(s.contains("skip    claude: `claude` is not installed"), "{s}");
    assert!(s.contains("ok      acme"), "{s}");
}

#[test]
fn sync_twice_reports_nothing_new_the_second_time() {
    let e = Env::new(ACME);
    e.good_acme();
    let first = e.run(&["agents", "sync"], &[]);
    assert_eq!(first.status.code(), Some(0), "{}", err(&first));
    let s = out(&first);
    assert!(s.contains("  NEW base     skills: ~/.agents/skills  commands: ~/.config/opencode/commands  commands: ~/.config/opencode/command\n"), "{s}");
    assert!(s.contains("  NEW acme     skills: ~/.acme/skills  commands: ~/.acme/commands  /orch-plan <feature>\n"), "{s}");
    assert!(s.contains("synced 2 row(s): 2 new, 0 unchanged — restart running agents so they pick up the skills\n"), "{s}");
    let h = e.home.path();
    assert!(h.join(".agents/skills/orch-plan/SKILL.md").exists());
    assert!(h.join(".config/opencode/commands/orch-run.md").exists());
    assert!(h.join(".config/opencode/command/orch-plan.md").exists());
    assert!(h.join(".acme/commands/orch-plan.md").exists());
    let manifest = e.manifest();
    for want in [".agents/skills/orch-plan", ".agents/skills/orch-run", ".acme/commands/orch-run.md", ".acme/skills/orch-plan"] {
        assert!(manifest.lines().any(|l| l.ends_with(want)), "{want} not in\n{manifest}");
    }
    let second = e.run(&["agents", "sync"], &[]);
    let s = out(&second);
    assert!(!s.contains("NEW"), "{s}");
    assert!(s.contains("  ok   acme "), "{s}");
    assert!(s.contains("synced 2 row(s): 0 new, 2 unchanged"), "{s}");
    assert_eq!(e.manifest(), manifest, "no duplicate manifest lines");
}

#[test]
fn sync_copies_the_shipped_text_exactly() {
    let e = Env::new(ACME);
    e.good_acme();
    e.run(&["agents", "sync"], &[]);
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../orch-config/assets");
    for (rel, dst) in [
        ("skills/orch-plan/SKILL.md", ".acme/skills/orch-plan/SKILL.md"),
        ("skills/orch-run/SKILL.md", ".agents/skills/orch-run/SKILL.md"),
        ("claude-commands/orch-plan.md", ".acme/commands/orch-plan.md"),
        ("opencode-commands/orch-run.md", ".config/opencode/commands/orch-run.md"),
    ] {
        assert_eq!(std::fs::read(crate_dir.join(rel)).unwrap(), std::fs::read(e.home.path().join(dst)).unwrap(), "{rel}");
    }
}

#[test]
fn sync_repairs_an_outdated_file() {
    let e = Env::new(ACME);
    e.good_acme();
    e.run(&["agents", "sync"], &[]);
    std::fs::write(e.home.path().join(".acme/skills/orch-plan/SKILL.md"), "old").unwrap();
    let s = out(&e.run(&["agents", "sync", "acme"], &[]));
    assert!(s.contains("  NEW acme "), "{s}");
}

#[test]
fn sync_of_named_kinds_skips_uninstalled_and_rejects_unknown() {
    let e = Env::new(ACME);
    e.good_acme();
    let o = e.run(&["agents", "sync", "claude"], &[]);
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).starts_with("skip claude    not installed\n"), "{}", out(&o));
    let o = e.run(&["agents", "sync", "bogus"], &[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).starts_with("ERROR: unknown agent kind bogus — known: "), "{}", err(&o));
}

#[test]
fn check_before_and_after_a_sync() {
    let e = Env::new(ACME);
    e.good_acme();
    let o = e.run(&["agents", "check"], &[]);
    assert_eq!(o.status.code(), Some(1));
    let s = out(&o);
    assert!(s.contains("MISSING acme      skill   "), "{s}");
    assert!(s.contains("MISSING acme      command "), "{s}");
    assert!(s.ends_with("4 missing or outdated — run: orch agents sync\n"), "{s}");
    assert!(!s.contains("claude"), "uninstalled kinds are not listed");
    e.run(&["agents", "sync"], &[]);
    let o = e.run(&["agents", "check"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert_eq!(out(&o).lines().count(), 4);
    assert!(out(&o).lines().all(|l| l.starts_with("ok   acme")), "{}", out(&o));
    std::fs::write(e.home.path().join(".acme/commands/orch-run.md"), "stale").unwrap();
    let o = e.run(&["agents", "check"], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("OUTDATED acme      command "), "{}", out(&o));
}

#[test]
fn the_agents_table_points_at_missing_skills() {
    let e = Env::new(ACME);
    e.good_acme();
    let s = out(&e.run(&["agents"], &[]));
    assert!(s.contains("\nacme: orch-plan/orch-run missing — run: orch agents sync\n"), "{s}");
    e.run(&["agents", "sync"], &[]);
    assert!(!out(&e.run(&["agents"], &[])).contains("missing"));
}
